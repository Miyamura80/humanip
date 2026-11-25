//! HTTP CONNECT proxy protocol.
//!
//! Server side: [`read_connect`], [`write_response`].
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

/// Server side. Reads the request head (max 8 KiB) and parses
/// `CONNECT host:port HTTP/1.x` plus an optional Basic `Proxy-Authorization`.
pub async fn read_connect<S: AsyncRead + Unpin>(s: &mut S) -> Result<ConnectRequest, ProtoError> {
    let head = read_head(s).await?;
    let mut lines = head.split("\r\n");
    let line = lines.next().unwrap_or_default();
    let mut parts = line.split_whitespace();
    let (Some(method), Some(authority), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(proto(format!("malformed request line: {line:?}")));
    };
    if method != "CONNECT" {
        return Err(proto(format!("method not supported: {method}")));
    }
    if !version.starts_with("HTTP/1.") {
        return Err(proto(format!("unsupported version: {version}")));
    }
    let target = parse_authority(authority)?;

    let mut credentials = None;
    for header in lines {
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("proxy-authorization") {
            credentials = parse_basic(value.trim());
        }
    }
    Ok(ConnectRequest {
        target,
        credentials,
    })
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
