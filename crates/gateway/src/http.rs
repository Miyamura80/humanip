//! HTTP proxy protocol: CONNECT tunnels and plain-HTTP forwarding.
//!
//! Server side: [`read_request`] (or [`read_connect`]), [`write_response`].
//! Forwarding v1 limitation: each client connection carries exactly one
//! forwarded request. The rewritten head says `Connection: close`, so the
//! origin closes after its response and response framing is never parsed.
//! Client side (chaining to a provider): [`client_connect`].

use std::net::{IpAddr, Ipv6Addr, SocketAddr};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::socks5::ProtoError;
use crate::TargetAddr;

/// Largest request or response head we accept.
pub const MAX_HEAD: usize = 8 * 1024;

/// A parsed CONNECT request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectRequest {
    pub target: TargetAddr,
    /// From `Proxy-Authorization: Basic`. `None` if absent or malformed.
    pub credentials: Option<(String, String)>,
}

fn proto(msg: impl Into<String>) -> ProtoError {
    ProtoError::Protocol(msg.into())
}

/// Reads one byte at a time up to and including "\r\n\r\n", so nothing past
/// the head is consumed.
async fn read_head<S: AsyncRead + Unpin>(s: &mut S) -> Result<String, ProtoError> {
    let mut buf = Vec::with_capacity(512);
    loop {
        let b = s.read_u8().await?;
        buf.push(b);
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
        if buf.len() >= MAX_HEAD {
            return Err(proto("head larger than 8 KiB"));
        }
    }
    buf.truncate(buf.len() - 4);
    String::from_utf8(buf).map_err(|_| proto("head is not valid utf-8"))
}

/// What a client sent to the HTTP listener.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProxyRequest {
    /// `CONNECT host:port`: open a raw tunnel.
    Connect(ConnectRequest),
    /// Absolute-form `METHOD http://host[:port]/path`: forward one request.
    Forward(ForwardRequest),
}

/// How the client->origin request body is framed after the head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestBody {
    /// No `Content-Length` and no `Transfer-Encoding`: nothing follows.
    None,
    /// Exactly this many bytes follow.
    Length(u64),
    /// `Transfer-Encoding` is set: bytes are spliced until the client stops.
    Streamed,
}

/// A plain-HTTP request to forward to an origin server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForwardRequest {
    pub method: String,
    /// Host and port from the absolute URI (port 80 when omitted).
    pub target: TargetAddr,
    /// From `Proxy-Authorization: Basic`. `None` if absent or malformed.
    pub credentials: Option<(String, String)>,
    /// Request head rewritten for the origin, ending in "\r\n\r\n".
    pub head: Vec<u8>,
    pub body: RequestBody,
}

/// Server side. Reads the request head (max 8 KiB) and parses either
/// `CONNECT host:port HTTP/1.x` or an absolute-form `http://` request, plus an
/// optional Basic `Proxy-Authorization`. Origin-form requests (`GET /`) give
/// `Protocol("method not supported: ...")`; `https://` URIs give `Protocol`.
pub async fn read_request<S: AsyncRead + Unpin>(s: &mut S) -> Result<ProxyRequest, ProtoError> {
    let head = read_head(s).await?;
    let mut lines = head.split("\r\n");
    let line = lines.next().unwrap_or_default();
    let mut parts = line.split_whitespace();
    let (Some(method), Some(uri), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(proto(format!("malformed request line: {line:?}")));
    };
    if !version.starts_with("HTTP/1.") {
        return Err(proto(format!("unsupported version: {version}")));
    }
    let headers: Vec<&str> = lines.collect();
    let credentials = headers
        .iter()
        .filter_map(|h| h.split_once(':'))
        .filter(|(n, _)| n.trim().eq_ignore_ascii_case("proxy-authorization"))
        .filter_map(|(_, v)| parse_basic(v.trim()))
        .next_back();
    if method == "CONNECT" {
        let target = parse_authority(uri)?;
        return Ok(ProxyRequest::Connect(ConnectRequest {
            target,
            credentials,
        }));
    }
    let Some(scheme_end) = uri.find("://") else {
        return Err(proto(format!("method not supported: {method}")));
    };
    let scheme = &uri[..scheme_end];
    if !scheme.eq_ignore_ascii_case("http") {
        return Err(proto(format!(
            "absolute URI scheme {scheme:?} not supported; use CONNECT for https"
        )));
    }
    let rest = &uri[scheme_end + 3..];
    let split = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(split);
    // Drop any userinfo so it never reaches the origin.
    let authority = authority.rsplit_once('@').map_or(authority, |(_, a)| a);
    let target = parse_uri_authority(authority)?;
    let path = path.split('#').next().unwrap_or_default();
    let path = match path.chars().next() {
        Some('/') => path.to_string(),
        _ => format!("/{path}"),
    };
    let (head, body) = rewrite_head(method, &path, version, authority, &headers)?;
    Ok(ProxyRequest::Forward(ForwardRequest {
        method: method.to_string(),
        target,
        credentials,
        head,
        body,
    }))
}

/// Server side, CONNECT only. Any other request gives
/// `Protocol("method not supported: ...")`.
pub async fn read_connect<S: AsyncRead + Unpin>(s: &mut S) -> Result<ConnectRequest, ProtoError> {
    match read_request(s).await? {
        ProxyRequest::Connect(c) => Ok(c),
        ProxyRequest::Forward(f) => Err(proto(format!("method not supported: {}", f.method))),
    }
}

/// Hop-by-hop headers (RFC 9110 section 7.6.1) that a proxy must not forward.
/// `Transfer-Encoding` is kept because the body is spliced through unchanged.
const HOP_BY_HOP: [&str; 5] = ["connection", "keep-alive", "te", "trailer", "upgrade"];

/// Builds the origin-form head: drops `Proxy-*`, hop-by-hop headers and any
/// header named in `Connection`, replaces `Host` with the URI authority and
/// appends `Connection: close`.
fn rewrite_head(
    method: &str,
    path: &str,
    version: &str,
    authority: &str,
    headers: &[&str],
) -> Result<(Vec<u8>, RequestBody), ProtoError> {
    let mut parsed = Vec::with_capacity(headers.len());
    for h in headers {
        let (name, value) = h
            .split_once(':')
            .ok_or_else(|| proto(format!("malformed header: {h:?}")))?;
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(proto(format!("malformed header: {h:?}")));
        }
        parsed.push((name.to_ascii_lowercase(), name, value.trim()));
    }
    let named: Vec<String> = parsed
        .iter()
        .filter(|(n, _, _)| n == "connection")
        .flat_map(|(_, _, v)| v.split(','))
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();

    let mut length: Option<u64> = None;
    let mut chunked = false;
    let mut out = format!("{method} {path} {version}\r\nHost: {authority}\r\n");
    for (lower, name, value) in &parsed {
        if lower == "content-length" {
            let n: u64 = value
                .parse()
                .map_err(|_| proto(format!("bad Content-Length: {value:?}")))?;
            if length.is_some_and(|l| l != n) {
                return Err(proto("conflicting Content-Length headers"));
            }
            length = Some(n);
        }
        if lower == "transfer-encoding" {
            chunked = true;
        }
        let drop = lower == "host"
            || lower.starts_with("proxy-")
            || HOP_BY_HOP.contains(&lower.as_str())
            || named.contains(lower);
        if !drop {
            out.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    out.push_str("Connection: close\r\n\r\n");
    let body = match (chunked, length) {
        (true, Some(_)) => {
            return Err(proto("both Transfer-Encoding and Content-Length"));
        }
        (true, None) => RequestBody::Streamed,
        (false, Some(n)) => RequestBody::Length(n),
        (false, None) => RequestBody::None,
    };
    Ok((out.into_bytes(), body))
}

/// Parses the authority of an `http://` URI; port defaults to 80.
fn parse_uri_authority(a: &str) -> Result<TargetAddr, ProtoError> {
    let has_port = match a.strip_prefix('[') {
        Some(rest) => rest.contains("]:"),
        None => a.contains(':'),
    };
    if has_port {
        parse_authority(a)
    } else {
        parse_authority(&format!("{a}:80"))
    }
}

/// Parses `host:port` or `[v6]:port`.
fn parse_authority(a: &str) -> Result<TargetAddr, ProtoError> {
    let bad = || proto(format!("bad CONNECT target: {a:?}"));
    if let Some(rest) = a.strip_prefix('[') {
        let (ip, port) = rest.split_once("]:").ok_or_else(bad)?;
        let ip: Ipv6Addr = ip.parse().map_err(|_| bad())?;
        let port: u16 = port.parse().map_err(|_| bad())?;
        return Ok(TargetAddr::Ip(SocketAddr::new(ip.into(), port)));
    }
    let (host, port) = a.rsplit_once(':').ok_or_else(bad)?;
    let port: u16 = port.parse().map_err(|_| bad())?;
    if host.is_empty() || host.contains(':') {
        return Err(bad());
    }
    if host.len() > 255 {
        return Err(proto("domain longer than 255 bytes"));
    }
    Ok(match host.parse::<IpAddr>() {
        Ok(ip) => TargetAddr::Ip(SocketAddr::new(ip, port)),
        Err(_) => TargetAddr::Domain(host.to_string(), port),
    })
}

/// Decodes `Basic <b64(user:pass)>`; `None` for other schemes or bad input.
fn parse_basic(value: &str) -> Option<(String, String)> {
    let (scheme, token) = value.split_once(char::is_whitespace)?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let raw = STANDARD.decode(token.trim()).ok()?;
    let text = String::from_utf8(raw).ok()?;
    let (user, pass) = text.split_once(':')?;
    Some((user.to_string(), pass.to_string()))
}

/// Writes `HTTP/1.1 {status} {reason}`, the headers, and the blank line.
pub async fn write_response<S: AsyncWrite + Unpin>(
    s: &mut S,
    status: u16,
    reason: &str,
    headers: &[(&str, &str)],
) -> std::io::Result<()> {
    let mut out = format!("HTTP/1.1 {status} {reason}\r\n");
    for (k, v) in headers {
        out.push_str(k);
        out.push_str(": ");
        out.push_str(v);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    s.write_all(out.as_bytes()).await?;
    s.flush().await
}

/// Client side, for chaining to a provider's HTTP proxy. Sends CONNECT with
/// Host and Basic Proxy-Authorization, then requires a 2xx response.
pub async fn client_connect<S: AsyncRead + AsyncWrite + Unpin>(
    s: &mut S,
    username: &str,
    password: &str,
    target: &TargetAddr,
) -> Result<(), ProtoError> {
    if username.contains(':') {
        return Err(proto("username must not contain ':' for basic auth"));
    }
    let authority = target.to_string();
    let token = STANDARD.encode(format!("{username}:{password}"));
    let req = format!(
        "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\
         Proxy-Authorization: Basic {token}\r\n\r\n"
    );
    s.write_all(req.as_bytes()).await?;
    s.flush().await?;

    let head = read_head(s).await?;
    let line = head.split("\r\n").next().unwrap_or_default();
    let mut parts = line.split_whitespace();
    let version = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/1.") {
        return Err(proto(format!("bad upstream status line: {line:?}")));
    }
    let status: u16 = parts
        .next()
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| proto(format!("bad upstream status line: {line:?}")))?;
    if !(200..300).contains(&status) {
        return Err(ProtoError::Rejected(format!(
            "upstream http status {status}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn parse(head: &str) -> Result<ProxyRequest, ProtoError> {
        let (mut c, mut s) = tokio::io::duplex(16 * 1024);
        c.write_all(head.as_bytes()).await.unwrap();
        drop(c);
        read_request(&mut s).await
    }

    async fn forward(head: &str) -> (ForwardRequest, String) {
        match parse(head).await.unwrap() {
            ProxyRequest::Forward(f) => {
                let text = String::from_utf8(f.head.clone()).unwrap();
                (f, text)
            }
            other => panic!("expected forward, got {other:?}"),
        }
    }

    fn header_names(head: &str) -> Vec<String> {
        head.split("\r\n")
            .skip(1)
            .filter_map(|l| l.split_once(':'))
            .map(|(n, _)| n.to_ascii_lowercase())
            .collect()
    }

    #[tokio::test]
    async fn absolute_get_with_query() {
        let (f, head) =
            forward("GET http://example.com/a/b?q=1&r=2 HTTP/1.1\r\nHost: example.com\r\n\r\n")
                .await;
        assert_eq!(f.target, TargetAddr::Domain("example.com".into(), 80));
        assert_eq!(f.method, "GET");
        assert_eq!(f.body, RequestBody::None);
        assert_eq!(
            head,
            "GET /a/b?q=1&r=2 HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n"
        );
    }

    #[tokio::test]
    async fn non_default_port_and_bare_query() {
        let (f, head) = forward("GET http://example.com:8080?x=1 HTTP/1.1\r\n\r\n").await;
        assert_eq!(f.target, TargetAddr::Domain("example.com".into(), 8080));
        assert!(head.starts_with("GET /?x=1 HTTP/1.1\r\nHost: example.com:8080\r\n"));
    }

    #[tokio::test]
    async fn ipv6_host() {
        let (f, head) = forward("GET http://[::1]:8081/ HTTP/1.1\r\n\r\n").await;
        assert_eq!(f.target, TargetAddr::Ip("[::1]:8081".parse().unwrap()));
        assert!(head.contains("\r\nHost: [::1]:8081\r\n"), "{head}");
        let (f, _) = forward("GET http://[::1]/ HTTP/1.1\r\n\r\n").await;
        assert_eq!(f.target, TargetAddr::Ip("[::1]:80".parse().unwrap()));
    }

    #[tokio::test]
    async fn missing_host_is_added_and_userinfo_dropped() {
        let (_, head) = forward("GET http://u:p@h.test/ HTTP/1.1\r\nAccept: */*\r\n\r\n").await;
        assert_eq!(
            head,
            "GET / HTTP/1.1\r\nHost: h.test\r\nAccept: */*\r\nConnection: close\r\n\r\n"
        );
    }

    #[tokio::test]
    async fn hop_by_hop_and_proxy_headers_stripped() {
        let token = STANDARD.encode("alice:key");
        let (f, head) = forward(&format!(
            "POST http://h.test/x HTTP/1.1\r\nHost: h.test\r\n\
             Proxy-Authorization: Basic {token}\r\nProxy-Connection: keep-alive\r\n\
             Connection: keep-alive, Upgrade\r\nKeep-Alive: timeout=5\r\nTE: trailers\r\n\
             Trailer: X-Sum\r\nUpgrade: h2c\r\nContent-Length: 3\r\nX-Keep: yes\r\n\r\n"
        ))
        .await;
        assert_eq!(f.credentials, Some(("alice".into(), "key".into())));
        assert_eq!(f.body, RequestBody::Length(3));
        assert_eq!(
            header_names(&head),
            ["host", "content-length", "x-keep", "connection"]
        );
        assert!(!head.to_ascii_lowercase().contains("proxy-"));
        assert!(!head.contains(&token));
    }

    #[tokio::test]
    async fn connection_close_present_once() {
        let (_, head) = forward("GET http://h.test/ HTTP/1.1\r\nConnection: close\r\n\r\n").await;
        assert_eq!(head.matches("Connection: close").count(), 1);
        assert!(head.ends_with("Connection: close\r\n\r\n"));
    }

    #[tokio::test]
    async fn headers_named_in_connection_stripped() {
        let (_, head) = forward(
            "GET http://h.test/ HTTP/1.1\r\nConnection: X-Secret, x-other\r\n\
             X-Secret: 1\r\nX-Other: 2\r\nX-Public: 3\r\n\r\n",
        )
        .await;
        assert_eq!(header_names(&head), ["host", "x-public", "connection"]);
    }

    #[tokio::test]
    async fn chunked_body_is_streamed() {
        let (f, head) =
            forward("POST http://h.test/ HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n").await;
        assert_eq!(f.body, RequestBody::Streamed);
        assert!(head.contains("Transfer-Encoding: chunked\r\n"));
        let both = "POST http://h.test/ HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\
                    Content-Length: 4\r\n\r\n";
        assert!(matches!(parse(both).await, Err(ProtoError::Protocol(_))));
    }

    #[tokio::test]
    async fn https_absolute_uri_rejected() {
        let err = parse("GET https://h.test/ HTTP/1.1\r\n\r\n")
            .await
            .unwrap_err();
        assert!(
            matches!(&err, ProtoError::Protocol(m) if !m.starts_with("method not supported")),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn origin_form_rejected_as_unsupported() {
        let err = parse("GET / HTTP/1.1\r\nHost: h.test\r\n\r\n")
            .await
            .unwrap_err();
        assert!(matches!(&err, ProtoError::Protocol(m) if m == "method not supported: GET"));
    }

    #[tokio::test]
    async fn connect_still_parses() {
        let req = parse("CONNECT h.test:443 HTTP/1.1\r\n\r\n").await.unwrap();
        assert_eq!(
            req,
            ProxyRequest::Connect(ConnectRequest {
                target: TargetAddr::Domain("h.test".into(), 443),
                credentials: None,
            })
        );
    }
}
