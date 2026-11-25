//! Plain-HTTP forward proxying, end to end:
//! client -> gateway A -> (gateway B as the "provider") -> origin server.
//! The origin echoes the exact request head and body it received, so the
//! tests can check what the gateway rewrote and what it never forwards.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use humanip_gateway::config::parse_yaml;
use humanip_gateway::server::Gateway;
use humanip_gateway::{http, TargetAddr};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Reads one request (head plus Content-Length body) and answers 200 with
/// the raw request as the body, then closes.
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
                let head = String::from_utf8_lossy(&req).to_ascii_lowercase();
                let len: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .map_or(0, |v| v.trim().parse().unwrap());
                let mut body = vec![0u8; len];
                s.read_exact(&mut body).await.unwrap();
                req.extend_from_slice(&body);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    req.len()
                );
                s.write_all(resp.as_bytes()).await.unwrap();
                s.write_all(&req).await.unwrap();
            });
        }
    });
    addr
}

/// Start a gateway; returns its HTTP listener address.
async fn start(yaml: &str) -> SocketAddr {
    let gw = Gateway::new(parse_yaml(yaml).expect("config"));
    let h = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = h.local_addr().unwrap();
    tokio::spawn(gw.serve_http(h));
    addr
}

/// Gateway B (provider, dials directly) and gateway A chaining to it over
/// HTTP CONNECT. Returns A's HTTP address.
async fn chain() -> SocketAddr {
    let b = start(
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
    upstream: {{ kind: http, addr: "{b}", username: "user-session_{{session}}", password: kb }}
"#
    ))
    .await
}

fn auth(user: &str, key: &str) -> String {
    format!(
        "Proxy-Authorization: Basic {}\r\n",
        STANDARD.encode(format!("{user}:{key}"))
    )
}

/// Sends `req` to the proxy and returns everything it answers until close.
async fn send(proxy: SocketAddr, req: &str) -> String {
    let mut s = TcpStream::connect(proxy).await.unwrap();
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).await.unwrap();
    String::from_utf8(out).unwrap()
}

/// Splits a proxied response into (status line, body = origin's view).
fn split(resp: &str) -> (&str, &str) {
    let status = resp.split("\r\n").next().unwrap();
    let body = resp.split_once("\r\n\r\n").map_or("", |(_, b)| b);
    (status, body)
}

#[tokio::test]
async fn curl_style_get_is_rewritten_for_origin() {
    let origin = origin_server().await;
    let proxy = chain().await;
    let req = format!(
        "GET http://{origin}/path?q=1 HTTP/1.1\r\nHost: {origin}\r\nUser-Agent: curl/8\r\n\
         Accept: */*\r\n{}Proxy-Connection: Keep-Alive\r\n\r\n",
        auth("user-session_fw1", "ka")
    );
    let resp = send(proxy, &req).await;
    let (status, seen) = split(&resp);
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert!(
        seen.starts_with("GET /path?q=1 HTTP/1.1\r\n"),
        "origin saw: {seen}"
    );
    assert!(seen.contains(&format!("\r\nHost: {origin}\r\n")));
    assert!(seen.contains("\r\nUser-Agent: curl/8\r\n"));
    assert!(seen.contains("\r\nConnection: close\r\n"));
    let lower = seen.to_ascii_lowercase();
    assert!(
        !lower.contains("proxy-authorization"),
        "credentials leaked: {seen}"
    );
    assert!(!lower.contains("proxy-connection"));
    assert!(!seen.contains(&STANDARD.encode("user-session_fw1:ka")));
}

#[tokio::test]
async fn post_body_reaches_origin_intact() {
    let origin = origin_server().await;
    let proxy = chain().await;
    let body = "name=humanip&payload=0123456789";
    let req = format!(
        "POST http://{origin}/submit HTTP/1.1\r\n{}Content-Type: application/x-www-form-urlencoded\r\n\
         Content-Length: {}\r\n\r\n{body}",
        auth("user", "ka"),
        body.len()
    );
    let resp = send(proxy, &req).await;
    let (status, seen) = split(&resp);
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert!(seen.starts_with("POST /submit HTTP/1.1\r\n"), "{seen}");
    assert!(seen.ends_with(&format!("\r\n\r\n{body}")), "{seen}");
}

#[tokio::test]
async fn chunked_request_body_is_refused_with_411() {
    let origin = origin_server().await;
    let proxy = chain().await;
    // A pipelined request after a chunked body must never reach the origin.
    let req = format!(
        "POST http://{origin}/up HTTP/1.1\r\n{}Transfer-Encoding: chunked\r\n\r\n\
         5\r\nhello\r\n0\r\n\r\nGET http://{origin}/two HTTP/1.1\r\n{}\r\n",
        auth("user", "ka"),
        auth("user", "ka")
    );
    let resp = send(proxy, &req).await;
    let (status, _) = split(&resp);
    assert_eq!(status, "HTTP/1.1 411 Length Required");
    assert_eq!(
        resp.matches("HTTP/1.1").count(),
        1,
        "nothing reached the origin: {resp}"
    );
}

#[tokio::test]
async fn pipelined_second_request_is_not_forwarded() {
    let origin = origin_server().await;
    let proxy = chain().await;
    let one = format!(
        "GET http://{origin}/one HTTP/1.1\r\n{}\r\n",
        auth("user", "ka")
    );
    let resp = send(proxy, &format!("{one}{one}")).await;
    let (status, seen) = split(&resp);
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert_eq!(resp.matches("HTTP/1.1 200 OK").count(), 1);
    assert!(!seen.to_ascii_lowercase().contains("proxy-authorization"));
}

#[tokio::test]
async fn wrong_or_missing_key_gives_407() {
    let origin = origin_server().await;
    let proxy = chain().await;
    for creds in [auth("user", "nope"), String::new()] {
        let req = format!("GET http://{origin}/ HTTP/1.1\r\n{creds}\r\n");
        let resp = send(proxy, &req).await;
        assert!(
            resp.starts_with("HTTP/1.1 407 Proxy Authentication Required\r\n"),
            "{resp}"
        );
        assert!(resp.contains("Proxy-Authenticate: Basic realm=\"humanip\"\r\n"));
        assert!(resp.contains("Content-Length: 0\r\n"));
    }
}

#[tokio::test]
async fn https_uri_gives_400_and_origin_form_gives_501() {
    let proxy = chain().await;
    let a = auth("user", "ka");
    let resp = send(proxy, &format!("GET https://h.test/ HTTP/1.1\r\n{a}\r\n")).await;
    assert!(resp.starts_with("HTTP/1.1 400 "), "{resp}");
    let resp = send(proxy, &format!("GET / HTTP/1.1\r\nHost: h.test\r\n{a}\r\n")).await;
    assert!(resp.starts_with("HTTP/1.1 501 "), "{resp}");
}

#[tokio::test]
async fn dead_origin_maps_to_502() {
    let dead = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap();
    let proxy = chain().await;
    let req = format!("GET http://{dead}/ HTTP/1.1\r\n{}\r\n", auth("user", "ka"));
    let resp = send(proxy, &req).await;
    assert!(resp.starts_with("HTTP/1.1 502 "), "{resp}");
}

#[tokio::test]
async fn connect_still_works() {
    let origin = origin_server().await;
    let proxy = chain().await;
    let mut s = TcpStream::connect(proxy).await.unwrap();
    http::client_connect(&mut s, "user-session_c1", "ka", &TargetAddr::Ip(origin))
        .await
        .unwrap();
    s.write_all(b"GET /tunnel HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    assert!(out.starts_with("HTTP/1.1 200 OK\r\n"), "{out}");
    assert!(out.ends_with("GET /tunnel HTTP/1.1\r\nHost: x\r\n\r\n"));
}
