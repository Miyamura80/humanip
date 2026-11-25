//! SOCKS5 (RFC 1928) with username/password auth (RFC 1929).
//!
//! Server side: [`negotiate`], [`auth_reply`], [`read_request`], [`reply`].
//! Client side (chaining to a provider): [`client_connect`].
//! [`ProtoError`] is the shared error type for all protocol modules.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::TargetAddr;

const VER: u8 = 0x05;
const AUTH_VER: u8 = 0x01;
const METHOD_NONE: u8 = 0x00;
const METHOD_USERPASS: u8 = 0x02;
const METHOD_REFUSED: u8 = 0xFF;
const CMD_CONNECT: u8 = 0x01;
const ATYP_V4: u8 = 0x01;
const ATYP_DOMAIN: u8 = 0x03;
const ATYP_V6: u8 = 0x04;

/// Error shared by the SOCKS5, HTTP CONNECT and upstream modules.
#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    /// Socket error, including unexpected EOF.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The peer sent something malformed or unsupported.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// The upstream refused auth or the connection.
    #[error("rejected: {0}")]
    Rejected(String),
    /// Dial or handshake did not finish in time.
    #[error("timed out")]
    Timeout,
}

fn proto(msg: impl Into<String>) -> ProtoError {
    ProtoError::Protocol(msg.into())
}

/// SOCKS5 reply codes (RFC 1928 section 6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Reply {
    Succeeded = 0,
    GeneralFailure = 1,
    NotAllowed = 2,
    NetworkUnreachable = 3,
    HostUnreachable = 4,
    ConnectionRefused = 5,
    TtlExpired = 6,
    CommandNotSupported = 7,
    AddrTypeNotSupported = 8,
}

impl Reply {
    /// Decode a wire reply code.
    pub fn from_u8(b: u8) -> Option<Reply> {
        Some(match b {
            0 => Reply::Succeeded,
            1 => Reply::GeneralFailure,
            2 => Reply::NotAllowed,
            3 => Reply::NetworkUnreachable,
            4 => Reply::HostUnreachable,
            5 => Reply::ConnectionRefused,
            6 => Reply::TtlExpired,
            7 => Reply::CommandNotSupported,
            8 => Reply::AddrTypeNotSupported,
            _ => return None,
        })
    }

    /// Human readable name.
    pub fn describe(self) -> &'static str {
        match self {
            Reply::Succeeded => "succeeded",
            Reply::GeneralFailure => "general failure",
            Reply::NotAllowed => "connection not allowed by ruleset",
            Reply::NetworkUnreachable => "network unreachable",
            Reply::HostUnreachable => "host unreachable",
            Reply::ConnectionRefused => "connection refused",
            Reply::TtlExpired => "ttl expired",
            Reply::CommandNotSupported => "command not supported",
            Reply::AddrTypeNotSupported => "address type not supported",
        }
    }

    /// Best reply code to send a client when dialing failed with `err`.
    pub fn for_error(err: &ProtoError) -> Reply {
        use std::io::ErrorKind;
        match err {
            ProtoError::Timeout => Reply::TtlExpired,
            ProtoError::Io(e) => match e.kind() {
                ErrorKind::ConnectionRefused => Reply::ConnectionRefused,
                ErrorKind::NetworkUnreachable => Reply::NetworkUnreachable,
                ErrorKind::HostUnreachable => Reply::HostUnreachable,
                _ => Reply::GeneralFailure,
            },
            ProtoError::Rejected(_) | ProtoError::Protocol(_) => Reply::GeneralFailure,
        }
    }
}

/// Server side. Reads the greeting; requires method 0x02 (user/pass), else
/// replies [5, 0xFF] and errors. Replies [5, 2], then reads the RFC 1929
/// sub-negotiation and returns (username, password). Does not send the auth
/// status: the caller authenticates first, then calls [`auth_reply`].
pub async fn negotiate<S: AsyncRead + AsyncWrite + Unpin>(
    s: &mut S,
) -> Result<(String, String), ProtoError> {
    let ver = s.read_u8().await?;
    if ver != VER {
        return Err(proto(format!("unsupported socks version {ver}")));
    }
    let n = s.read_u8().await? as usize;
    let mut methods = vec![0u8; n];
    s.read_exact(&mut methods).await?;
    if !methods.contains(&METHOD_USERPASS) {
        s.write_all(&[VER, METHOD_REFUSED]).await?;
        s.flush().await?;
        return Err(proto("client does not offer username/password auth"));
    }
    s.write_all(&[VER, METHOD_USERPASS]).await?;
    s.flush().await?;

    let aver = s.read_u8().await?;
    if aver != AUTH_VER {
        return Err(proto(format!("unsupported auth version {aver}")));
    }
    let user = read_str(s, "username").await?;
    let pass = read_str(s, "password").await?;
    Ok((user, pass))
}

async fn read_str<S: AsyncRead + Unpin>(s: &mut S, what: &str) -> Result<String, ProtoError> {
    let len = s.read_u8().await? as usize;
    let mut buf = vec![0u8; len];
    s.read_exact(&mut buf).await?;
    String::from_utf8(buf).map_err(|_| proto(format!("{what} is not valid utf-8")))
}

/// Sends the RFC 1929 status: [1, 0] on success, [1, 1] on failure.
pub async fn auth_reply<S: AsyncWrite + Unpin>(s: &mut S, ok: bool) -> std::io::Result<()> {
    s.write_all(&[AUTH_VER, if ok { 0 } else { 1 }]).await?;
    s.flush().await
}

/// Reads the request. Only CONNECT is supported; other commands get
/// CommandNotSupported. Unknown address types get AddrTypeNotSupported.
/// A domain that is an IP literal is returned as [`TargetAddr::Ip`].
pub async fn read_request<S: AsyncRead + AsyncWrite + Unpin>(
    s: &mut S,
) -> Result<TargetAddr, ProtoError> {
    let mut head = [0u8; 4];
    s.read_exact(&mut head).await?;
    let [ver, cmd, _rsv, atyp] = head;
    if ver != VER {
        return Err(proto(format!("unsupported socks version {ver}")));
    }
    if cmd != CMD_CONNECT {
        reply(s, Reply::CommandNotSupported).await?;
        return Err(proto(format!("command not supported: {cmd}")));
    }
    match read_addr(s, atyp).await? {
        Some(t) => Ok(t),
        None => {
            reply(s, Reply::AddrTypeNotSupported).await?;
            Err(proto(format!("address type not supported: {atyp}")))
        }
    }
}

/// Reads an address of type `atyp` plus port. `None` for an unknown type.
async fn read_addr<S: AsyncRead + Unpin>(
    s: &mut S,
    atyp: u8,
) -> Result<Option<TargetAddr>, ProtoError> {
    let target = match atyp {
        ATYP_V4 => {
            let mut b = [0u8; 4];
            s.read_exact(&mut b).await?;
            let port = s.read_u16().await?;
            TargetAddr::Ip(SocketAddr::new(Ipv4Addr::from(b).into(), port))
        }
        ATYP_V6 => {
            let mut b = [0u8; 16];
            s.read_exact(&mut b).await?;
            let port = s.read_u16().await?;
            TargetAddr::Ip(SocketAddr::new(Ipv6Addr::from(b).into(), port))
        }
        ATYP_DOMAIN => {
            let host = read_str(s, "domain").await?;
            let port = s.read_u16().await?;
            if host.is_empty() {
                return Err(proto("empty domain"));
            }
            match host.parse::<IpAddr>() {
                Ok(ip) => TargetAddr::Ip(SocketAddr::new(ip, port)),
                Err(_) => TargetAddr::Domain(host, port),
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(target))
}

/// Sends a reply with bound address 0.0.0.0:0.
pub async fn reply<S: AsyncWrite + Unpin>(s: &mut S, r: Reply) -> std::io::Result<()> {
    s.write_all(&[VER, r as u8, 0, ATYP_V4, 0, 0, 0, 0, 0, 0])
        .await?;
    s.flush().await
}

/// Client side, for chaining to a provider's SOCKS5 endpoint: offers
/// user/pass auth, authenticates, sends CONNECT, and consumes the reply.
/// A refused auth or non-zero reply code is [`ProtoError::Rejected`].
pub async fn client_connect<S: AsyncRead + AsyncWrite + Unpin>(
    s: &mut S,
    username: &str,
    password: &str,
    target: &TargetAddr,
) -> Result<(), ProtoError> {
    if username.len() > 255 {
        return Err(proto("username longer than 255 bytes"));
    }
    if password.len() > 255 {
        return Err(proto("password longer than 255 bytes"));
    }
    let request = encode_connect(target)?;

    s.write_all(&[VER, 1, METHOD_USERPASS]).await?;
    s.flush().await?;
    let mut sel = [0u8; 2];
    s.read_exact(&mut sel).await?;
    if sel[0] != VER {
        return Err(proto(format!("upstream sent socks version {}", sel[0])));
    }
    match sel[1] {
        METHOD_USERPASS => {
            let mut auth = Vec::with_capacity(3 + username.len() + password.len());
            auth.push(AUTH_VER);
            auth.push(username.len() as u8);
            auth.extend_from_slice(username.as_bytes());
            auth.push(password.len() as u8);
            auth.extend_from_slice(password.as_bytes());
            s.write_all(&auth).await?;
            s.flush().await?;
            let mut status = [0u8; 2];
            s.read_exact(&mut status).await?;
            if status[1] != 0 {
                return Err(ProtoError::Rejected(format!(
                    "upstream socks5 auth failed (status {})",
                    status[1]
                )));
            }
        }
        METHOD_NONE => {}
        METHOD_REFUSED => {
            return Err(ProtoError::Rejected(
                "upstream socks5 refused all auth methods".into(),
            ))
        }
        m => return Err(proto(format!("upstream selected unoffered method {m}"))),
    }

    s.write_all(&request).await?;
    s.flush().await?;
    let mut head = [0u8; 4];
    s.read_exact(&mut head).await?;
    if head[0] != VER {
        return Err(proto(format!("upstream sent socks version {}", head[0])));
    }
    if head[1] != 0 {
        let why = Reply::from_u8(head[1]).map_or("unknown reply", Reply::describe);
        return Err(ProtoError::Rejected(format!(
            "upstream socks5 reply {}: {why}",
            head[1]
        )));
    }
    if read_addr(s, head[3]).await?.is_none() {
        return Err(proto(format!("upstream bound address type {}", head[3])));
    }
    Ok(())
}

fn encode_connect(target: &TargetAddr) -> Result<Vec<u8>, ProtoError> {
    let mut req = vec![VER, CMD_CONNECT, 0];
    match target {
        TargetAddr::Ip(SocketAddr::V4(a)) => {
            req.push(ATYP_V4);
            req.extend_from_slice(&a.ip().octets());
        }
        TargetAddr::Ip(SocketAddr::V6(a)) => {
            req.push(ATYP_V6);
            req.extend_from_slice(&a.ip().octets());
        }
        TargetAddr::Domain(h, _) => {
            if h.is_empty() || h.len() > 255 {
                return Err(proto("domain must be 1 to 255 bytes"));
            }
            req.push(ATYP_DOMAIN);
            req.push(h.len() as u8);
            req.extend_from_slice(h.as_bytes());
        }
    }
    req.extend_from_slice(&target.port().to_be_bytes());
    Ok(req)
}
