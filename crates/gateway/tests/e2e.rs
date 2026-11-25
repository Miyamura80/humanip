//! End to end: client -> gateway A -> (gateway B as the "provider") -> echo server.
//! Gateway B dials directly (dev mode), so this proves real upstream chaining
//! over both SOCKS5 and HTTP CONNECT without any external network.

use humanip_gateway::accounts_bridge::memory_from_config;
use humanip_gateway::config::parse_yaml;
use humanip_gateway::server::Gateway;
use humanip_gateway::{http, socks5, TargetAddr};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

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

/// Start a gateway on ephemeral ports; returns (socks addr, http addr, handle).
async fn start(yaml: &str) -> (SocketAddr, SocketAddr, Arc<Gateway>) {
    let cfg = parse_yaml(yaml).expect("config");
    let accounts = Arc::new(memory_from_config(&cfg));
    let gw = Gateway::new(cfg, accounts);
    let s = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let h = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (sa, ha) = (s.local_addr().unwrap(), h.local_addr().unwrap());
    tokio::spawn(gw.clone().serve_socks(s));
    tokio::spawn(gw.clone().serve_http(h));
    (sa, ha, gw)
}

/// Gateway B: the stand-in provider. Dials targets directly.
async fn provider() -> (SocketAddr, SocketAddr) {
    let (s, h, _) = start(
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
    .await;
    (s, h)
}

/// Gateway A: the humanip front door, chaining to B over `kind`.
async fn front(kind: &str, b: SocketAddr) -> (SocketAddr, SocketAddr, Arc<Gateway>) {
    start(&format!(
        r#"
api_keys: [{{ name: alice, key: ka }}]
providers:
  - name: chain
    exit_type: isp
    sourcing: verified
    price_per_gb: 2.0
    fraud_score: 5
    max_sticky_minutes: 129600
    latency_ms: 10
    upstream: {{ kind: {kind}, addr: "{b}", username: "user-session_{{session}}", password: kb }}
routes:
  - name: bridge
    mode: sticky
    sticky: {{ ttl: forever, on_pin_loss: fail_closed }}
    exit_types: [isp]
    reliability_floor: 0.5
"#
    ))
    .await
}

async fn roundtrip(mut s: TcpStream) {
    s.write_all(b"hello through humanip").await.unwrap();
    let mut buf = [0u8; 21];
    s.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"hello through humanip");
}

#[tokio::test]
async fn socks5_front_chains_through_socks5_provider() {
    let echo = echo_server().await;
    let (b_socks, _) = provider().await;
    let (a_socks, _, gw) = front("socks5", b_socks).await;
    let mut s = TcpStream::connect(a_socks).await.unwrap();
    socks5::client_connect(&mut s, "user-route_bridge", "ka", &TargetAddr::Ip(echo))
        .await
        .unwrap();
    roundtrip(s).await;
    assert_eq!(gw.selector().sessions().len(), 1, "named sticky route pins");
}

#[tokio::test]
async fn http_front_chains_through_http_provider() {
    let echo = echo_server().await;
    let (_, b_http) = provider().await;
    let (_, a_http, _) = front("http", b_http).await;
    let mut s = TcpStream::connect(a_http).await.unwrap();
    http::client_connect(&mut s, "user-session_r42", "ka", &TargetAddr::Ip(echo))
        .await
        .unwrap();
    roundtrip(s).await;
}

#[tokio::test]
async fn wrong_key_is_rejected_on_both_protocols() {
    let echo = echo_server().await;
    let (b_socks, _) = provider().await;
    let (a_socks, a_http, _) = front("socks5", b_socks).await;
    let mut s = TcpStream::connect(a_socks).await.unwrap();
    assert!(
        socks5::client_connect(&mut s, "user-session_x", "nope", &TargetAddr::Ip(echo))
            .await
            .is_err()
    );
    let mut h = TcpStream::connect(a_http).await.unwrap();
    assert!(
        http::client_connect(&mut h, "user-session_x", "nope", &TargetAddr::Ip(echo))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn sticky_session_reuses_its_pin() {
    let echo = echo_server().await;
    let (b_socks, _) = provider().await;
    let (a_socks, _, gw) = front("socks5", b_socks).await;
    for _ in 0..3 {
        let mut s = TcpStream::connect(a_socks).await.unwrap();
        socks5::client_connect(&mut s, "user-session_same", "ka", &TargetAddr::Ip(echo))
            .await
            .unwrap();
        roundtrip(s).await;
    }
    assert_eq!(gw.selector().sessions().len(), 1);
}

#[tokio::test]
async fn dead_pinned_provider_fails_closed() {
    // Provider address that refuses connections.
    let dead = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap();
    let echo = echo_server().await;
    let (a_socks, _, _) = front("socks5", dead).await;
    let mut s = TcpStream::connect(a_socks).await.unwrap();
    let err =
        socks5::client_connect(&mut s, "user-route_bridge", "ka", &TargetAddr::Ip(echo)).await;
    assert!(
        err.is_err(),
        "fail_closed route must refuse when its exit is down"
    );
}
