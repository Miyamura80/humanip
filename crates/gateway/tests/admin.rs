//! Admin listener over a real loopback socket.

use humanip_core::accounts::{
    AccountStore, KeyRecord, KycTier, MemoryAccounts, MemoryKey, UsageEvent,
};
use humanip_gateway::accounts_bridge::memory_from_config;
use humanip_gateway::admin::serve_admin;
use humanip_gateway::config::parse_yaml;
use humanip_gateway::health::Outcome;
use humanip_gateway::server::Gateway;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const TOKEN: &str = "admin-token-123";

const CONFIG: &str = r#"
api_keys: [{ name: alice, key: ka }]
providers:
  - name: chain
    exit_type: isp
    sourcing: verified
    price_per_gb: 2.0
    fraud_score: 5
    max_sticky_minutes: 129600
    latency_ms: 10
    countries: [DE]
    upstream: { kind: socks5, addr: "127.0.0.1:9", username: "{session}", password: p }
  - name: bright
    exit_type: residential
    sourcing: standard
    price_per_gb: 4.0
    fraud_score: 15
    max_sticky_minutes: 30
    latency_ms: 400
    upstream: { kind: brightdata, customer: hl_abc, zone: resi, password: x }
routes:
  - name: bridge
    mode: sticky
    sticky: { ttl: forever, on_pin_loss: fail_closed }
    exit_types: [isp]
"#;

async fn start(accounts: Option<Arc<dyn AccountStore>>) -> (SocketAddr, Arc<Gateway>) {
    let cfg = parse_yaml(CONFIG).expect("config");
    let store: Arc<dyn AccountStore> = Arc::new(memory_from_config(&cfg));
    let gw = Gateway::new(cfg, store);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(serve_admin(l, gw.clone(), TOKEN.into(), accounts));
    (addr, gw)
}

struct Resp {
    status: u16,
    head: String,
    body: String,
}

impl Resp {
    fn header(&self, name: &str) -> Option<String> {
        self.head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    }

    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).expect("json body")
    }
}

async fn get(addr: SocketAddr, path: &str, token: Option<&str>) -> Resp {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let auth = token.map_or(String::new(), |t| format!("Authorization: Bearer {t}\r\n"));
    let req = format!("GET {path} HTTP/1.1\r\nHost: admin\r\n{auth}\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8(raw).unwrap();
    let (head, body) = text.split_once("\r\n\r\n").expect("head");
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    let resp = Resp {
        status,
        head: head.to_string(),
        body: body.to_string(),
    };
    let len: usize = resp.header("content-length").unwrap().parse().unwrap();
    assert_eq!(len, resp.body.len(), "content-length matches body");
    resp
}

#[tokio::test]
async fn healthz_needs_no_auth() {
    let (addr, _) = start(None).await;
    let r = get(addr, "/healthz", None).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.body, "ok\n");
}

#[tokio::test]
async fn metrics_is_prometheus_text() {
    let (addr, _) = start(None).await;
    humanip_gateway::metrics::dial_failure("admin-test-provider");
    let r = get(addr, "/metrics", None).await;
    assert_eq!(r.status, 200);
    assert_eq!(
        r.header("content-type").as_deref(),
        Some("text/plain; version=0.0.4; charset=utf-8")
    );
    assert!(r.body.contains("# TYPE humanip_connections_total counter"));
    assert!(r
        .body
        .contains("humanip_dial_failures_total{provider=\"admin-test-provider\"}"));
}

#[tokio::test]
async fn status_requires_the_token() {
    let (addr, _) = start(None).await;
    for token in [None, Some("wrong"), Some("admin-token-1234")] {
        let r = get(addr, "/admin/v1/status", token).await;
        assert_eq!(r.status, 401, "{token:?}");
        assert!(r.header("www-authenticate").unwrap().starts_with("Bearer"));
    }
    let r = get(addr, "/admin/v1/usage?since=0", None).await;
    assert_eq!(r.status, 401);
}

#[tokio::test]
async fn status_json_shape() {
    let (addr, gw) = start(None).await;
    gw.health()
        .record("chain", "example.com", Outcome::Success, Instant::now());
    let r = get(addr, "/admin/v1/status", Some(TOKEN)).await;
    assert_eq!(r.status, 200);
    assert_eq!(
        r.header("content-type").as_deref(),
        Some("application/json")
    );
    let v = r.json();
    for k in [
        "version",
        "uptime_secs",
        "sessions",
        "providers",
        "routes",
        "health",
    ] {
        assert!(v.get(k).is_some(), "missing {k}");
    }
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    assert!(v["uptime_secs"].is_u64());
    assert_eq!(v["sessions"], 0);
    let p = &v["providers"][0];
    assert_eq!(p["name"], "chain");
    assert_eq!(p["exit_type"], "isp");
    assert_eq!(p["sourcing"], "verified");
    assert_eq!(p["price_per_gb"], 2.0);
    assert_eq!(p["fraud_score"], 5);
    assert_eq!(p["countries"], serde_json::json!(["DE"]));
    assert!(p["unverified_adapter"].is_null());
    assert_eq!(v["providers"][1]["unverified_adapter"], "brightdata");
    assert_eq!(v["routes"][0]["name"], "bridge");
    assert_eq!(v["routes"][0]["mode"], "sticky");
    let h = &v["health"][0];
    assert_eq!(h["provider"], "chain");
    assert_eq!(h["domain"], "example.com");
    assert_eq!(h["samples"], 1);
    assert!(h["success_rate"].is_f64());
}

#[tokio::test]
async fn usage_without_store_is_503() {
    let (addr, _) = start(None).await;
    let r = get(addr, "/admin/v1/usage?since=0", Some(TOKEN)).await;
    assert_eq!(r.status, 503);
}

fn key(id: &str, account: &str) -> MemoryKey {
    MemoryKey {
        record: KeyRecord {
            key_id: id.into(),
            account_id: account.into(),
            name: id.into(),
            kyc_tier: KycTier::None,
            monthly_cap_micro: None,
            revoked: false,
        },
        secret: format!("secret-{id}"),
    }
}

fn event(key_id: &str, at_unix: i64) -> UsageEvent {
    UsageEvent {
        key_id: key_id.into(),
        provider: "chain".into(),
        domain: "example.com".into(),
        bytes_up: 1_000,
        bytes_down: 9_000,
        provider_price_per_gb: 2.0,
        at_unix,
    }
}

#[tokio::test]
async fn usage_from_memory_store() {
    let store = Arc::new(MemoryAccounts::new(0.2));
    store.add_key(key("k1", "acct_a"), 1_000_000);
    store.add_key(key("k2", "acct_b"), 1_000_000);
    store.record_usage(&event("k1", 100)).unwrap();
    store.record_usage(&event("k1", 200)).unwrap();
    store.record_usage(&event("k2", 300)).unwrap();
    let (addr, _) = start(Some(store)).await;

    let r = get(addr, "/admin/v1/usage?since=150", Some(TOKEN)).await;
    assert_eq!(r.status, 200);
    let rows = r.json();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["key_id"], "k1");
    assert_eq!(rows[0]["bytes"], 10_000);
    for k in ["key_id", "provider", "domain", "bytes", "total_micro"] {
        assert!(rows[0].get(k).is_some(), "missing {k}");
    }

    let r = get(addr, "/admin/v1/usage?since=0&account=acct_b", Some(TOKEN)).await;
    let rows = r.json();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["key_id"], "k2");

    let r = get(addr, "/admin/v1/usage", Some(TOKEN)).await;
    assert_eq!(r.status, 400, "since is required");
}

#[tokio::test]
async fn unknown_paths_are_404() {
    let (addr, _) = start(None).await;
    for path in ["/", "/admin/v1/nope", "/healthz/x", "/admin/v1"] {
        assert_eq!(get(addr, path, Some(TOKEN)).await.status, 404, "{path}");
    }
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(b"POST /healthz HTTP/1.1\r\n\r\n")
        .await
        .unwrap();
    let mut raw = String::new();
    s.read_to_string(&mut raw).await.unwrap();
    assert!(raw.starts_with("HTTP/1.1 404 "), "{raw}");
}

#[tokio::test]
async fn malformed_request_is_400() {
    let (addr, _) = start(None).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(b"garbage\r\n\r\n").await.unwrap();
    let mut raw = String::new();
    s.read_to_string(&mut raw).await.unwrap();
    assert!(raw.starts_with("HTTP/1.1 400 "), "{raw}");
}
