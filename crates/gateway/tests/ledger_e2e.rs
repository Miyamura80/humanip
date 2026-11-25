//! The production path: keys issued in the SQLite ledger authenticate at the
//! gateway (`accounts_db`), and finished connections are charged back into the
//! same ledger file. client -> gateway A (ledger) -> gateway B (provider) -> echo.

use humanip_core::accounts::{price_usage, AccountStore, KycTier, MICROS_PER_USD};
use humanip_core::TAKE_RATE;
use humanip_gateway::accounts_bridge::memory_from_config;
use humanip_gateway::config::parse_yaml;
use humanip_gateway::server::{accounts_for, Gateway};
use humanip_gateway::{socks5, TargetAddr};
use humanip_ledger::SqliteAccounts;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const PRICE: f64 = 1_000_000.0; // USD per GB: high so a few bytes cost micros
const MSG: &[u8] = b"billed through the ledger";

fn temp_db(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("humanip-{tag}-{}-{nanos}.db", std::process::id()))
}

async fn echo_server() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                while let Ok(n) = s.read(&mut buf).await {
                    if n == 0 || s.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    addr
}

/// Gateway B: the stand-in provider, dialing directly, with an in-memory store.
async fn provider() -> SocketAddr {
    let cfg = parse_yaml(
        r#"
allow_direct: true
api_keys: [{ name: provider, key: kb }]
providers:
  - name: local
    exit_type: residential
    sourcing: verified
    price_per_gb: 1.0
    fraud_score: 5
    max_sticky_minutes: 129600
    latency_ms: 10
    upstream: { kind: direct }
"#,
    )
    .unwrap();
    let accounts = Arc::new(memory_from_config(&cfg));
    let gw = Gateway::new(cfg, accounts);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(gw.serve_socks(l));
    addr
}

/// Gateway A backed by the ledger at `db`; returns its SOCKS address.
async fn front(db: &std::path::Path, b: SocketAddr) -> SocketAddr {
    let cfg = parse_yaml(&format!(
        r#"
accounts_db: "{}"
api_keys: []
providers:
  - name: chain
    exit_type: isp
    sourcing: verified
    price_per_gb: {PRICE}
    fraud_score: 5
    max_sticky_minutes: 129600
    latency_ms: 10
    upstream: {{ kind: socks5, addr: "{b}", username: "user-session_{{session}}", password: kb }}
"#,
        db.display()
    ))
    .unwrap();
    let accounts = accounts_for(&cfg).expect("ledger opens");
    let gw = Gateway::new(cfg, accounts);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(gw.serve_socks(l));
    addr
}

#[tokio::test]
async fn ledger_key_is_admitted_and_charged_in_the_ledger() {
    let db = temp_db("e2e");
    // An operator issues a key and tops it up, as humanip-admin would.
    let admin = SqliteAccounts::open(&db).unwrap();
    let acct = admin.create_account("alice").unwrap();
    let (key, secret) = admin
        .issue_key(&acct, "laptop", KycTier::Basic, None)
        .unwrap();
    assert!(admin.credit(&acct, 10 * MICROS_PER_USD, "seed").unwrap());
    let broke = admin.create_account("bob").unwrap();
    let (_, broke_secret) = admin.issue_key(&broke, "k", KycTier::None, None).unwrap();

    let echo = echo_server().await;
    let a = front(&db, provider().await).await;

    let mut s = TcpStream::connect(a).await.unwrap();
    socks5::client_connect(&mut s, "user-session_x", &secret, &TargetAddr::Ip(echo))
        .await
        .unwrap();
    s.write_all(MSG).await.unwrap();
    let mut buf = vec![0u8; MSG.len()];
    s.read_exact(&mut buf).await.unwrap();
    assert_eq!(buf, MSG);
    drop(s);

    // Usage is recorded when the connection closes; read it back from the file.
    let mut rows = Vec::new();
    for _ in 0..250 {
        rows = admin.usage_since(Some(&acct), 0).unwrap();
        if !rows.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(rows.len(), 1, "one usage row");
    assert_eq!(rows[0].key_id, key.key_id);
    assert_eq!(rows[0].bytes, 2 * MSG.len() as u64);
    let expected = price_usage(rows[0].bytes, PRICE, TAKE_RATE).total_micro;
    assert_eq!(rows[0].total_micro, expected);
    assert_eq!(
        admin.balance(&acct).unwrap(),
        10 * MICROS_PER_USD - expected
    );

    // An unfunded ledger key is refused before any dial.
    let mut s = TcpStream::connect(a).await.unwrap();
    let r = socks5::client_connect(&mut s, "user", &broke_secret, &TargetAddr::Ip(echo)).await;
    assert!(r.is_err(), "zero balance must be refused");
    let _ = std::fs::remove_file(&db);
}

#[test]
fn config_rejects_a_malformed_blocklist() {
    let yaml = r#"
api_keys: [{ name: a, key: k }]
providers:
  - name: p
    exit_type: isp
    sourcing: verified
    price_per_gb: 1
    fraud_score: 1
    max_sticky_minutes: 10
    latency_ms: 1
    upstream: { kind: socks5, addr: "127.0.0.1:1", username: u, password: p }
blocked_domains:
  - { pattern: "https://chase.com/", category: banking, min_kyc: verified }
"#;
    let err = parse_yaml(yaml).unwrap_err().to_string();
    assert!(err.contains("blocked_domains[0]"), "{err}");
}

#[test]
fn bad_accounts_db_path_fails_at_startup() {
    let cfg = parse_yaml(
        r#"
accounts_db: "/nonexistent-dir/humanip/ledger.db"
api_keys: []
providers:
  - name: p
    exit_type: isp
    sourcing: verified
    price_per_gb: 1
    fraud_score: 1
    max_sticky_minutes: 10
    latency_ms: 1
    upstream: { kind: socks5, addr: "127.0.0.1:1", username: u, password: p }
"#,
    )
    .unwrap();
    assert!(accounts_for(&cfg).is_err());
}
