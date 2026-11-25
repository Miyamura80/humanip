//! Store-backed auth, admission, abuse checks and usage metering, end to end:
//! client -> gateway A -> (gateway B as the "provider") -> echo/origin server.
//! Only gateway A's store is inspected; B has its own.

use humanip_core::abuse::{Blocked, DomainRule};
use humanip_core::accounts::{
    price_usage, AccountStore, KeyRecord, KycTier, MemoryAccounts, MemoryKey, UsageRow,
    MICROS_PER_USD,
};
use humanip_core::TAKE_RATE;
use humanip_gateway::accounts_bridge::{memory_from_config, AbuseCheck};
use humanip_gateway::config::parse_yaml;
use humanip_gateway::server::Gateway;
use humanip_gateway::{http, socks5, TargetAddr};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Provider price on gateway A, USD per GB. High so a few bytes cost micros.
const PRICE: f64 = 1_000_000.0;
const MSG: &[u8] = b"hello through humanip";

async fn echo_server() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                loop {
                    match s.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if s.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
    });
    addr
}

const ORIGIN_RESPONSE: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";

/// Reads a request head, answers a fixed response and closes.
async fn origin_server() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            tokio::spawn(async move {
                let mut req = Vec::new();
                let mut b = [0u8; 1];
                while !req.ends_with(b"\r\n\r\n") {
                    if s.read(&mut b).await.unwrap_or(0) == 0 {
                        return;
                    }
                    req.push(b[0]);
                }
                let _ = s.write_all(ORIGIN_RESPONSE).await;
            });
        }
    });
    addr
}

/// Starts a gateway on ephemeral ports; returns (socks, http).
async fn start(
    yaml: &str,
    accounts: Option<Arc<dyn AccountStore>>,
    check: Option<AbuseCheck>,
) -> (SocketAddr, SocketAddr) {
    let cfg = parse_yaml(yaml).expect("config");
    let accounts = accounts.unwrap_or_else(|| Arc::new(memory_from_config(&cfg)));
    let gw = match check {
        Some(c) => Gateway::with_abuse_check(cfg, accounts, c),
        None => Gateway::new(cfg, accounts),
    };
    let s = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let h = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addrs = (s.local_addr().unwrap(), h.local_addr().unwrap());
    tokio::spawn(gw.clone().serve_socks(s));
    tokio::spawn(gw.serve_http(h));
    addrs
}

/// Gateway B: the stand-in provider. Dials targets directly.
async fn provider() -> (SocketAddr, SocketAddr) {
    start(
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
        None,
        None,
    )
    .await
}

fn front_yaml(keys: &str, kind: &str, b: SocketAddr) -> String {
    format!(
        r#"
api_keys: {keys}
providers:
  - name: chain
    exit_type: isp
    sourcing: verified
    price_per_gb: {PRICE}
    fraud_score: 5
    max_sticky_minutes: 129600
    latency_ms: 10
    upstream: {{ kind: {kind}, addr: "{b}", username: "user-session_{{session}}", password: kb }}
"#
    )
}

/// Gateway A over a store seeded from `keys`; returns (socks, http, store).
async fn front(keys: &str) -> (SocketAddr, SocketAddr, Arc<MemoryAccounts>) {
    front_with(keys, None).await
}

async fn front_with(
    keys: &str,
    check: Option<AbuseCheck>,
) -> (SocketAddr, SocketAddr, Arc<MemoryAccounts>) {
    let (b_socks, b_http) = provider().await;
    let socks_yaml = front_yaml(keys, "socks5", b_socks);
    let http_yaml = front_yaml(keys, "http", b_http);
    // One store, two front gateways: SOCKS front chains over SOCKS5, HTTP over HTTP.
    let store = Arc::new(memory_from_config(&parse_yaml(&socks_yaml).unwrap()));
    let (a_socks, _) = start(&socks_yaml, Some(store.clone()), check).await;
    let (_, a_http) = start(&http_yaml, Some(store.clone()), check).await;
    (a_socks, a_http, store)
}

async fn roundtrip(mut s: TcpStream) {
    s.write_all(MSG).await.unwrap();
    let mut buf = vec![0u8; MSG.len()];
    s.read_exact(&mut buf).await.unwrap();
    assert_eq!(buf, MSG);
}

/// Waits until the store has usage for `acct`, then a little longer so a
/// duplicate charge would show up too.
async fn usage(store: &MemoryAccounts, acct: &str) -> Vec<UsageRow> {
    for _ in 0..250 {
        if !store.usage_since(Some(acct), 0).unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(150)).await;
    store.usage_since(Some(acct), 0).unwrap()
}

async fn send(proxy: SocketAddr, req: &str) -> String {
    let mut s = TcpStream::connect(proxy).await.unwrap();
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).await.unwrap();
    String::from_utf8(out).unwrap()
}

fn basic(user: &str, key: &str) -> String {
    use base64::Engine as _;
    let creds = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{key}"));
    format!("Proxy-Authorization: Basic {creds}\r\n")
}

fn get(target: &str, user: &str, key: &str) -> String {
    format!(
        "GET http://{target}/ HTTP/1.1\r\nHost: {target}\r\n{}\r\n",
        basic(user, key)
    )
}

async fn socks_err(a: SocketAddr, user: &str, key: &str, t: &TargetAddr) -> String {
    let mut s = TcpStream::connect(a).await.unwrap();
    socks5::client_connect(&mut s, user, key, t)
        .await
        .expect_err("socks connect should fail")
        .to_string()
}

async fn connect_err(a: SocketAddr, user: &str, key: &str, t: &TargetAddr) -> String {
    let mut s = TcpStream::connect(a).await.unwrap();
    http::client_connect(&mut s, user, key, t)
        .await
        .expect_err("CONNECT should fail")
        .to_string()
}

const ALICE: &str = "[{ name: alice, key: ka }]";

fn assert_charged_once(store: &MemoryAccounts, rows: &[UsageRow], bytes: u64) {
    assert_eq!(rows.len(), 1, "{rows:?}");
    let r = &rows[0];
    assert_eq!(
        (r.key_id.as_str(), r.provider.as_str()),
        ("key_alice", "chain")
    );
    assert_eq!(r.bytes, bytes);
    let charge = price_usage(bytes, PRICE, TAKE_RATE);
    assert!(charge.total_micro > 0);
    assert_eq!(r.total_micro, charge.total_micro);
    assert_eq!(
        store.balance("acct_alice").unwrap(),
        100 * MICROS_PER_USD - charge.total_micro
    );
}

#[tokio::test]
async fn socks5_tunnel_is_charged_exactly_once() {
    let echo = echo_server().await;
    let (a_socks, _, store) = front(ALICE).await;
    let mut s = TcpStream::connect(a_socks).await.unwrap();
    socks5::client_connect(&mut s, "user", "ka", &TargetAddr::Ip(echo))
        .await
        .unwrap();
    roundtrip(s).await;
    let rows = usage(&store, "acct_alice").await;
    assert_charged_once(&store, &rows, 2 * MSG.len() as u64);
    assert_eq!(rows[0].domain, "127.0.0.1");
}

#[tokio::test]
async fn http_connect_tunnel_is_charged_exactly_once() {
    let echo = echo_server().await;
    let (_, a_http, store) = front(ALICE).await;
    let mut s = TcpStream::connect(a_http).await.unwrap();
    http::client_connect(
        &mut s,
        "user-target_example.com",
        "ka",
        &TargetAddr::Ip(echo),
    )
    .await
    .unwrap();
    roundtrip(s).await;
    let rows = usage(&store, "acct_alice").await;
    assert_charged_once(&store, &rows, 2 * MSG.len() as u64);
    assert_eq!(rows[0].domain, "example.com", "billed under the target");
}

#[tokio::test]
async fn forward_request_is_metered_both_ways() {
    let origin = origin_server().await;
    let (_, a_http, store) = front(ALICE).await;
    let resp = send(a_http, &get(&origin.to_string(), "user", "ka")).await;
    assert_eq!(resp.as_bytes(), ORIGIN_RESPONSE);
    let rows = usage(&store, "acct_alice").await;
    assert_eq!(rows.len(), 1);
    // Up: the rewritten head; down: exactly the origin's response.
    let up = rows[0].bytes - ORIGIN_RESPONSE.len() as u64;
    assert!(up > "GET / HTTP/1.1\r\n\r\n".len() as u64, "{rows:?}");
    assert_charged_once(&store, &rows, rows[0].bytes);
}

#[tokio::test]
async fn zero_balance_is_refused_with_402() {
    let echo = echo_server().await;
    let origin = origin_server().await;
    let (a_socks, a_http, store) = front("[{ name: alice, key: ka, balance_usd: 0 }]").await;
    let t = TargetAddr::Ip(echo);
    assert!(socks_err(a_socks, "user", "ka", &t)
        .await
        .contains("reply 2"));
    assert!(connect_err(a_http, "user", "ka", &t).await.contains("402"));
    let resp = send(a_http, &get(&origin.to_string(), "user", "ka")).await;
    assert!(
        resp.starts_with("HTTP/1.1 402 Payment Required\r\n"),
        "{resp}"
    );
    assert!(resp.contains("Content-Length: 0\r\n") && resp.contains("Connection: close\r\n"));
    assert!(store.usage_since(None, 0).unwrap().is_empty());
}

#[tokio::test]
async fn reaching_the_monthly_cap_refuses_the_next_connection() {
    let echo = echo_server().await;
    // One roundtrip costs 42 bytes * $1e6/GB * 1.05 = $0.0441, over the cap.
    let (a_socks, a_http, store) = front("[{ name: alice, key: ka, monthly_cap_usd: 0.01 }]").await;
    let t = TargetAddr::Ip(echo);
    let mut s = TcpStream::connect(a_socks).await.unwrap();
    socks5::client_connect(&mut s, "user", "ka", &t)
        .await
        .unwrap();
    roundtrip(s).await;
    assert_eq!(usage(&store, "acct_alice").await.len(), 1);
    assert!(socks_err(a_socks, "user", "ka", &t)
        .await
        .contains("reply 2"));
    assert!(connect_err(a_http, "user", "ka", &t).await.contains("402"));
}

#[tokio::test]
async fn revoked_key_gives_403() {
    let echo = echo_server().await;
    let origin = origin_server().await;
    let (b_socks, b_http) = provider().await;
    let store = Arc::new(MemoryAccounts::new(TAKE_RATE));
    store.add_key(
        MemoryKey {
            record: KeyRecord {
                key_id: "key_old".into(),
                account_id: "acct_old".into(),
                name: "old".into(),
                kyc_tier: KycTier::Verified,
                monthly_cap_micro: None,
                revoked: true,
            },
            secret: "ko".into(),
        },
        50 * MICROS_PER_USD,
    );
    let (a_socks, _) = start(
        &front_yaml(ALICE, "socks5", b_socks),
        Some(store.clone()),
        None,
    )
    .await;
    let (_, a_http) = start(
        &front_yaml(ALICE, "http", b_http),
        Some(store.clone()),
        None,
    )
    .await;
    let t = TargetAddr::Ip(echo);
    assert!(socks_err(a_socks, "user", "ko", &t)
        .await
        .contains("reply 2"));
    assert!(connect_err(a_http, "user", "ko", &t).await.contains("403"));
    let resp = send(a_http, &get(&origin.to_string(), "user", "ko")).await;
    assert!(resp.starts_with("HTTP/1.1 403 Key Revoked\r\n"), "{resp}");
    assert!(resp.contains("Content-Length: 0\r\n") && resp.contains("Connection: close\r\n"));
    assert_eq!(store.balance("acct_old").unwrap(), 50 * MICROS_PER_USD);
}

/// Fake rules: `blocked.test` needs a verified account.
fn fake_check(_rules: &[DomainRule], domain: &str, tier: KycTier) -> Result<(), Blocked> {
    if domain == "blocked.test" && tier < KycTier::Verified {
        return Err(Blocked {
            category: "banking".into(),
            pattern: "blocked.test".into(),
            required: KycTier::Verified,
        });
    }
    Ok(())
}

const KYC_KEYS: &str = "[{ name: alice, key: ka }, { name: vera, key: kv, kyc_tier: verified }]";

#[tokio::test]
async fn abuse_rules_block_before_dialing() {
    let echo = echo_server().await;
    let (a_socks, a_http, store) = front_with(KYC_KEYS, Some(fake_check)).await;
    // `blocked.test` does not resolve, so a 403 (not 502) means no dial was tried.
    let blocked = TargetAddr::Domain("blocked.test".into(), 80);
    assert!(socks_err(a_socks, "user", "ka", &blocked)
        .await
        .contains("reply 2"));
    assert!(connect_err(a_http, "user", "ka", &blocked)
        .await
        .contains("403"));
    let resp = send(a_http, &get("blocked.test", "user", "ka")).await;
    assert!(
        resp.starts_with("HTTP/1.1 403 Blocked: banking\r\n"),
        "{resp}"
    );
    assert!(resp.contains("Content-Length: 0\r\n") && resp.contains("Connection: close\r\n"));
    // The username target is the checked domain too (same rule as select).
    let echo_t = TargetAddr::Ip(echo);
    assert!(
        socks_err(a_socks, "user-target_blocked.test", "ka", &echo_t)
            .await
            .contains("reply 2")
    );
    assert!(store.usage_since(None, 0).unwrap().is_empty());
    // A verified key passes the same rule.
    let mut s = TcpStream::connect(a_socks).await.unwrap();
    socks5::client_connect(&mut s, "user-target_blocked.test", "kv", &echo_t)
        .await
        .unwrap();
    roundtrip(s).await;
    let rows = usage(&store, "acct_vera").await;
    assert_eq!((rows.len(), rows[0].domain.as_str()), (1, "blocked.test"));
}

#[tokio::test]
async fn auth_failure_is_unchanged() {
    let echo = echo_server().await;
    let origin = origin_server().await;
    let (a_socks, a_http, store) = front(ALICE).await;
    let t = TargetAddr::Ip(echo);
    assert!(socks_err(a_socks, "user", "nope", &t)
        .await
        .contains("auth failed"));
    assert!(connect_err(a_http, "user", "nope", &t)
        .await
        .contains("407"));
    let resp = send(a_http, &get(&origin.to_string(), "user", "nope")).await;
    assert!(
        resp.starts_with("HTTP/1.1 407 Proxy Authentication Required\r\n"),
        "{resp}"
    );
    assert!(resp.contains("Proxy-Authenticate: Basic realm=\"humanip\"\r\n"));
    assert!(store.usage_since(None, 0).unwrap().is_empty());
}
