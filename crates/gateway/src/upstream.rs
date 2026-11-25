//! Dialing a target through an [`Upstream`].

use std::time::Duration;

use tokio::net::TcpStream;

use crate::socks5::ProtoError;
use crate::{http, socks5, TargetAddr, Upstream};

/// Dial `target` through `up` within `timeout` (TCP connect plus handshake).
/// Direct dials the target itself. The returned stream is ready for raw
/// byte splicing. Timeout gives [`ProtoError::Timeout`].
pub async fn connect(
    up: &Upstream,
    target: &TargetAddr,
    timeout: Duration,
) -> Result<TcpStream, ProtoError> {
    tokio::time::timeout(timeout, dial(up, target))
        .await
        .unwrap_or(Err(ProtoError::Timeout))
}

async fn dial(up: &Upstream, target: &TargetAddr) -> Result<TcpStream, ProtoError> {
    let stream = match up {
        Upstream::Direct => match target {
            TargetAddr::Ip(a) => TcpStream::connect(a).await?,
            TargetAddr::Domain(h, p) => TcpStream::connect((h.as_str(), *p)).await?,
        },
        Upstream::Socks5 {
            addr,
            username,
            password,
        } => {
            let mut s = TcpStream::connect(addr.as_str()).await?;
            socks5::client_connect(&mut s, username, password, target).await?;
            s
        }
        Upstream::Http {
            addr,
            username,
            password,
        } => {
            let mut s = TcpStream::connect(addr.as_str()).await?;
            http::client_connect(&mut s, username, password, target).await?;
            s
        }
    };
    stream.set_nodelay(true)?;
    Ok(stream)
}
