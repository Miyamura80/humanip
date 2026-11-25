//! Wire protocol tests: SOCKS5, HTTP CONNECT, upstream dialing.

use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use humanip_gateway::socks5::{self, ProtoError, Reply};
use humanip_gateway::{http, upstream, TargetAddr, Upstream};
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn domain(h: &str, p: u16) -> TargetAddr {
    TargetAddr::Domain(h.into(), p)
}

fn ip(s: &str) -> TargetAddr {
    TargetAddr::Ip(s.parse().unwrap())
}

const AUTH: &[u8] = &[5, 1, 2, 1, 5, b'a', b'l', b'i', b'c', b'e', 2, b'p', b'w'];

/// Sends greeting + auth + request bytes and runs the server functions.
async fn server_parse(request: &[u8]) -> (Result<TargetAddr, ProtoError>, Vec<u8>) {
    let (mut client, mut server) = duplex(1024);
    let mut bytes = AUTH.to_vec();
    bytes.extend_from_slice(request);
    client.write_all(&bytes).await.unwrap();
    let creds = socks5::negotiate(&mut server).await.unwrap();
    assert_eq!(creds, ("alice".to_string(), "pw".to_string()));
    socks5::auth_reply(&mut server, true).await.unwrap();
    let res = socks5::read_request(&mut server).await;
    drop(server);
    let mut out = Vec::new();
    client.read_to_end(&mut out).await.unwrap();
    (res, out)
}

#[tokio::test]
async fn socks5_server_domain_target() {
    let mut req = vec![5, 1, 0, 3, 11];
    req.extend_from_slice(b"example.com");
    req.extend_from_slice(&443u16.to_be_bytes());
    let (res, out) = server_parse(&req).await;
    assert_eq!(res.unwrap(), domain("example.com", 443));
    assert_eq!(out, vec![5, 2, 1, 0]);
}

#[tokio::test]
async fn socks5_server_ipv4_target() {
    let (res, _) = server_parse(&[5, 1, 0, 1, 10, 0, 0, 1, 0, 80]).await;
    assert_eq!(res.unwrap(), ip("10.0.0.1:80"));
}

#[tokio::test]
async fn socks5_server_ipv6_target() {
    let mut req = vec![5, 1, 0, 4];
    req.extend_from_slice(&[0; 15]);
    req.push(1);
    req.extend_from_slice(&8080u16.to_be_bytes());
    let (res, _) = server_parse(&req).await;
    assert_eq!(res.unwrap(), ip("[::1]:8080"));
}

#[tokio::test]
async fn socks5_server_ip_literal_domain_becomes_ip() {
    let mut req = vec![5, 1, 0, 3, 9];
    req.extend_from_slice(b"127.0.0.1");
    req.extend_from_slice(&22u16.to_be_bytes());
    let (res, _) = server_parse(&req).await;
    assert_eq!(res.unwrap(), ip("127.0.0.1:22"));
}

#[tokio::test]
async fn socks5_server_refuses_no_userpass_method() {
    let (mut client, mut server) = duplex(64);
    client.write_all(&[5, 1, 0]).await.unwrap();
    let err = socks5::negotiate(&mut server).await.unwrap_err();
    assert!(matches!(err, ProtoError::Protocol(_)), "{err:?}");
    let mut buf = [0u8; 2];
    client.read_exact(&mut buf).await.unwrap();
    assert_eq!(buf, [5, 0xFF]);
}

#[tokio::test]
async fn socks5_server_auth_failure_status() {
    let (mut client, mut server) = duplex(64);
    socks5::auth_reply(&mut server, false).await.unwrap();
    let mut buf = [0u8; 2];
    client.read_exact(&mut buf).await.unwrap();
    assert_eq!(buf, [1, 1]);
}

#[tokio::test]
async fn socks5_server_rejects_non_connect() {
    let (res, out) = server_parse(&[5, 2, 0, 1, 1, 2, 3, 4, 0, 80]).await;
    assert!(matches!(res, Err(ProtoError::Protocol(_))));
    assert_eq!(&out[4..], &[5, 7, 0, 1, 0, 0, 0, 0, 0, 0]);
}

#[tokio::test]
async fn socks5_server_rejects_unknown_atyp() {
    let (res, out) = server_parse(&[5, 1, 0, 9]).await;
    assert!(matches!(res, Err(ProtoError::Protocol(_))));
    assert_eq!(out[5], Reply::AddrTypeNotSupported as u8);
}

#[tokio::test]
async fn socks5_server_truncated_input_is_io_error() {
    let (mut client, mut server) = duplex(64);
    client.write_all(&[5, 3, 2]).await.unwrap();
    drop(client);
    let err = socks5::negotiate(&mut server).await.unwrap_err();
    assert!(matches!(err, ProtoError::Io(_)), "{err:?}");
}

#[tokio::test]
async fn socks5_client_rejects_oversized_fields() {
    let (mut a, _b) = duplex(64);
    let long = "x".repeat(256);
    let t = domain("example.com", 443);
    for (u, p) in [(long.as_str(), "pw"), ("alice", long.as_str())] {
        let err = socks5::client_connect(&mut a, u, p, &t).await.unwrap_err();
        assert!(matches!(err, ProtoError::Protocol(_)), "{err:?}");
    }
    let err = socks5::client_connect(&mut a, "u", "p", &domain(&long, 443))
        .await
        .unwrap_err();
    assert!(matches!(err, ProtoError::Protocol(_)), "{err:?}");
}

/// Runs the server functions on one end of a duplex and answers with `r`.
async fn socks5_round_trip(target: TargetAddr, accept: bool, r: Reply) -> Result<(), ProtoError> {
    let (mut c, mut s) = duplex(1024);
    let expected = target.clone();
    let server = tokio::spawn(async move {
        let (u, p) = socks5::negotiate(&mut s).await.unwrap();
        assert_eq!((u.as_str(), p.as_str()), ("bob", "secret"));
        socks5::auth_reply(&mut s, accept).await.unwrap();
        if !accept {
            return;
        }
        assert_eq!(socks5::read_request(&mut s).await.unwrap(), expected);
        socks5::reply(&mut s, r).await.unwrap();
        if r == Reply::Succeeded {
            s.write_all(b"payload").await.unwrap();
        }
    });
    let res = socks5::client_connect(&mut c, "bob", "secret", &target).await;
    if res.is_ok() {
        let mut buf = [0u8; 7];
        c.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"payload");
    }
    server.await.unwrap();
    res
}

#[tokio::test]
async fn socks5_client_server_round_trip() {
    for t in [
        domain("example.com", 443),
        ip("1.2.3.4:80"),
        ip("[2001:db8::1]:443"),
    ] {
        socks5_round_trip(t, true, Reply::Succeeded).await.unwrap();
    }
}

#[tokio::test]
async fn socks5_client_auth_rejected() {
    let err = socks5_round_trip(domain("a.com", 1), false, Reply::Succeeded)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, ProtoError::Rejected(m) if m.contains("auth")),
        "{err:?}"
    );
}

#[tokio::test]
async fn socks5_client_reply_rejected() {
    let err = socks5_round_trip(domain("a.com", 1), true, Reply::HostUnreachable)
        .await
        .unwrap_err();
    assert!(matches!(&err, ProtoError::Rejected(m) if m.contains("host unreachable")));
}

#[test]
fn reply_for_error_mapping() {
    use std::io::{Error, ErrorKind};
    let refused = ProtoError::Io(Error::from(ErrorKind::ConnectionRefused));
    assert_eq!(Reply::for_error(&refused), Reply::ConnectionRefused);
    assert_eq!(Reply::for_error(&ProtoError::Timeout), Reply::TtlExpired);
    let rej = ProtoError::Rejected("x".into());
    assert_eq!(Reply::for_error(&rej), Reply::GeneralFailure);
}

async fn http_parse(head: &[u8]) -> Result<http::ConnectRequest, ProtoError> {
    let (mut c, mut s) = duplex(16 * 1024);
    c.write_all(head).await.unwrap();
    drop(c);
    http::read_connect(&mut s).await
}

#[tokio::test]
async fn http_connect_without_auth() {
    let req = http_parse(b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n")
        .await
        .unwrap();
    assert_eq!(req.target, domain("example.com", 443));
    assert_eq!(req.credentials, None);
}

#[tokio::test]
async fn http_connect_with_auth_lowercase_header() {
    let token = STANDARD.encode("alice:p:w");
    for name in ["Proxy-Authorization", "proxy-authorization"] {
        let head = format!("CONNECT 1.2.3.4:8443 HTTP/1.0\r\n{name}: Basic {token}\r\n\r\n");
        let req = http_parse(head.as_bytes()).await.unwrap();
        assert_eq!(req.target, ip("1.2.3.4:8443"));
        assert_eq!(req.credentials, Some(("alice".into(), "p:w".into())));
    }
}

#[tokio::test]
async fn http_connect_ipv6_target() {
    let req = http_parse(b"CONNECT [::1]:443 HTTP/1.1\r\n\r\n")
        .await
        .unwrap();
    assert_eq!(req.target, ip("[::1]:443"));
}

#[tokio::test]
async fn http_connect_rejects_other_methods_and_bad_targets() {
    let err = http_parse(b"GET http://x/ HTTP/1.1\r\n\r\n")
        .await
        .unwrap_err();
    assert!(matches!(&err, ProtoError::Protocol(m) if m == "method not supported: GET"));
    for bad in [
        &b"CONNECT example.com HTTP/1.1\r\n\r\n"[..],
        b"CONNECT ::1:443 HTTP/1.1\r\n\r\n",
        b"CONNECT a.com:99999 HTTP/1.1\r\n\r\n",
        b"CONNECT a.com:443 HTTP/2\r\n\r\n",
    ] {
        let err = http_parse(bad).await.unwrap_err();
        assert!(matches!(err, ProtoError::Protocol(_)), "{err:?}");
    }
}

#[tokio::test]
async fn http_connect_oversize_head() {
    let mut head = b"CONNECT a.com:443 HTTP/1.1\r\nX-Pad: ".to_vec();
    head.extend(std::iter::repeat_n(b'a', 9000));
    head.extend_from_slice(b"\r\n\r\n");
    let err = http_parse(&head).await.unwrap_err();
    assert!(matches!(err, ProtoError::Protocol(_)), "{err:?}");
}

#[tokio::test]
async fn http_connect_does_not_consume_past_head() {
    let (mut c, mut s) = duplex(1024);
    c.write_all(b"CONNECT a.com:443 HTTP/1.1\r\n\r\nhello")
        .await
        .unwrap();
    drop(c);
    http::read_connect(&mut s).await.unwrap();
    let mut rest = String::new();
    s.read_to_string(&mut rest).await.unwrap();
    assert_eq!(rest, "hello");
}

#[tokio::test]
async fn http_write_response_format() {
    let (mut c, mut s) = duplex(1024);
    let hdr = [("Proxy-Authenticate", "Basic realm=\"humanip\"")];
    http::write_response(&mut s, 407, "Proxy Authentication Required", &hdr)
        .await
        .unwrap();
    drop(s);
    let mut out = String::new();
    c.read_to_string(&mut out).await.unwrap();
    assert_eq!(
        out,
        "HTTP/1.1 407 Proxy Authentication Required\r\n\
         Proxy-Authenticate: Basic realm=\"humanip\"\r\n\r\n"
    );
}

/// Runs http::client_connect against a scripted upstream response.
async fn http_client_scripted(response: &'static str) -> (Result<(), ProtoError>, String) {
    let (mut c, mut s) = duplex(4096);
    let upstream = tokio::spawn(async move {
        let req = http::read_connect(&mut s).await.unwrap();
        s.write_all(response.as_bytes()).await.unwrap();
        req
    });
    let res = http::client_connect(&mut c, "bob", "pw", &ip("[::1]:443")).await;
    let req = upstream.await.unwrap();
    assert_eq!(req.target, ip("[::1]:443"));
    assert_eq!(req.credentials, Some(("bob".into(), "pw".into())));
    let mut rest = String::new();
    c.read_to_string(&mut rest).await.unwrap();
    (res, rest)
}

#[tokio::test]
async fn http_client_ok() {
    let (res, rest) =
        http_client_scripted("HTTP/1.1 200 Connection Established\r\n\r\ntunnel").await;
    res.unwrap();
    assert_eq!(rest, "tunnel");
}

#[tokio::test]
async fn http_client_rejected_407() {
    let (res, _) = http_client_scripted(
        "HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic\r\n\r\n",
    )
    .await;
    let err = res.unwrap_err();
    assert!(matches!(&err, ProtoError::Rejected(m) if m == "upstream http status 407"));
}

async fn echo_listener() -> std::net::SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            tokio::spawn(async move {
                let (mut r, mut w) = s.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    addr
}

async fn assert_echo(s: &mut TcpStream) {
    s.write_all(b"ping").await.unwrap();
    let mut buf = [0u8; 4];
    s.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"ping");
}

const T: Duration = Duration::from_secs(5);

#[tokio::test]
async fn upstream_direct_to_echo() {
    let echo = echo_listener().await;
    let mut s = upstream::connect(&Upstream::Direct, &TargetAddr::Ip(echo), T)
        .await
        .unwrap();
    assert_echo(&mut s).await;
    let t = domain("localhost", echo.port());
    let mut s = upstream::connect(&Upstream::Direct, &t, T).await.unwrap();
    assert_echo(&mut s).await;
}

/// Minimal SOCKS5 proxy built from the server functions.
async fn tiny_socks5() -> std::net::SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut c, _)) = l.accept().await {
            tokio::spawn(async move {
                let (u, p) = socks5::negotiate(&mut c).await.unwrap();
                let ok = u == "u" && p == "p";
                socks5::auth_reply(&mut c, ok).await.unwrap();
                if !ok {
                    return;
                }
                let target = socks5::read_request(&mut c).await.unwrap();
                match upstream::connect(&Upstream::Direct, &target, T).await {
                    Ok(mut up) => {
                        socks5::reply(&mut c, Reply::Succeeded).await.unwrap();
                        let _ = tokio::io::copy_bidirectional(&mut c, &mut up).await;
                    }
                    Err(e) => socks5::reply(&mut c, Reply::for_error(&e)).await.unwrap(),
                }
            });
        }
    });
    addr
}

#[tokio::test]
async fn upstream_socks5_chain_to_echo() {
    let echo = echo_listener().await;
    let proxy = tiny_socks5().await;
    let up = Upstream::Socks5 {
        addr: proxy.to_string(),
        username: "u".into(),
        password: "p".into(),
    };
    let mut s = upstream::connect(&up, &TargetAddr::Ip(echo), T)
        .await
        .unwrap();
    assert_echo(&mut s).await;

    let bad = Upstream::Socks5 {
        addr: proxy.to_string(),
        username: "u".into(),
        password: "wrong".into(),
    };
    let err = upstream::connect(&bad, &TargetAddr::Ip(echo), T)
        .await
        .unwrap_err();
    assert!(matches!(err, ProtoError::Rejected(_)), "{err:?}");
}

#[tokio::test]
async fn upstream_timeout_when_proxy_is_silent() {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((s, _)) = l.accept().await {
            held.push(s);
        }
    });
    for up in [
        Upstream::Socks5 {
            addr: addr.to_string(),
            username: "u".into(),
            password: "p".into(),
        },
        Upstream::Http {
            addr: addr.to_string(),
            username: "u".into(),
            password: "p".into(),
        },
    ] {
        let err = upstream::connect(&up, &domain("a.com", 443), Duration::from_millis(100))
            .await
            .unwrap_err();
        assert!(matches!(err, ProtoError::Timeout), "{err:?}");
    }
}

#[tokio::test]
async fn upstream_direct_refused_is_io_error() {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    let err = upstream::connect(&Upstream::Direct, &TargetAddr::Ip(addr), T)
        .await
        .unwrap_err();
    assert_eq!(Reply::for_error(&err), Reply::ConnectionRefused);
}
