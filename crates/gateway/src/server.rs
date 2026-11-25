//! Accept loops: SOCKS5 and HTTP (CONNECT plus plain-HTTP forwarding) front
//! ends over one selector.
//!
//! Per connection: handshake, authenticate (password = API key, username =
//! settings), choose an exit, dial the upstream (one retry when policy
//! allows), report the outcome to passive health, then splice bytes.

use crate::config::GatewayConfig;
use crate::health::{HealthTracker, Outcome};
use crate::select::{authenticate, AuthedUser, Choice, DialFailure, SelectError, Selector};
use crate::socks5::{self, ProtoError, Reply};
use crate::{http, upstream, TargetAddr};
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{copy_bidirectional, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Why a connection could not be opened, mapped to protocol replies.
#[derive(Debug)]
pub enum OpenError {
    Select(SelectError),
    /// A pinned exit failed and the route says fail closed.
    PinLost(String),
    Dial(ProtoError),
}

impl OpenError {
    fn socks_reply(&self) -> Reply {
        match self {
            OpenError::Select(SelectError::UnknownRoute(_)) | OpenError::PinLost(_) => {
                Reply::NotAllowed
            }
            OpenError::Select(SelectError::PinLost { .. }) => Reply::NotAllowed,
            OpenError::Select(SelectError::NoEligibleExit { .. }) => Reply::NetworkUnreachable,
            OpenError::Dial(ProtoError::Timeout) => Reply::TtlExpired,
            OpenError::Dial(_) => Reply::HostUnreachable,
        }
    }

    fn http_status(&self) -> (u16, &'static str) {
        match self {
            OpenError::Select(SelectError::UnknownRoute(_)) => (403, "Unknown Route"),
            OpenError::Select(SelectError::PinLost { .. }) | OpenError::PinLost(_) => {
                (502, "Pinned Exit Lost")
            }
            OpenError::Select(SelectError::NoEligibleExit { .. }) => (502, "No Eligible Exit"),
            OpenError::Dial(ProtoError::Timeout) => (504, "Upstream Timeout"),
            OpenError::Dial(_) => (502, "Upstream Failed"),
        }
    }
}

pub struct Gateway {
    selector: Selector,
    health: Arc<HealthTracker>,
    timeout: Duration,
}

impl Gateway {
    pub fn new(cfg: GatewayConfig) -> Arc<Self> {
        let health = Arc::new(HealthTracker::default());
        let timeout = Duration::from_millis(cfg.connect_timeout_ms);
        let selector = Selector::new(Arc::new(cfg), health.clone());
        Arc::new(Gateway {
            selector,
            health,
            timeout,
        })
    }

    pub fn selector(&self) -> &Selector {
        &self.selector
    }

    pub fn health(&self) -> &HealthTracker {
        &self.health
    }

    /// Choose and dial an exit. Retries once when a failed pin may fail over.
    pub async fn open(
        &self,
        who: &AuthedUser,
        target: &TargetAddr,
    ) -> Result<(TcpStream, Choice), OpenError> {
        let mut last = None;
        for _ in 0..2 {
            let choice = self
                .selector
                .choose(who, target, Instant::now())
                .map_err(OpenError::Select)?;
            match upstream::connect(&choice.upstream, target, self.timeout).await {
                Ok(stream) => {
                    self.health.record(
                        &choice.provider,
                        &choice.domain,
                        Outcome::Success,
                        Instant::now(),
                    );
                    return Ok((stream, choice));
                }
                Err(e) => {
                    let outcome = if matches!(e, ProtoError::Timeout) {
                        Outcome::Timeout
                    } else {
                        Outcome::ConnectFailed
                    };
                    self.health
                        .record(&choice.provider, &choice.domain, outcome, Instant::now());
                    tracing::warn!(provider = %choice.provider, target = %target, error = %e, "upstream dial failed");
                    match self.selector.report_dial_failure(&choice) {
                        DialFailure::FailClosed => {
                            tracing::warn!(provider = %choice.provider, "pinned exit lost; failing closed");
                            return Err(OpenError::PinLost(choice.provider));
                        }
                        DialFailure::Retry => last = Some(e),
                    }
                }
            }
        }
        Err(OpenError::Dial(last.unwrap_or(ProtoError::Timeout)))
    }

    pub async fn serve_socks(self: Arc<Self>, listener: TcpListener) -> io::Result<()> {
        loop {
            let (stream, peer) = listener.accept().await?;
            let gw = self.clone();
            tokio::spawn(async move {
                if let Err(e) = gw.handle_socks(stream).await {
                    tracing::debug!(%peer, error = %e, "socks connection ended");
                }
            });
        }
    }

    pub async fn serve_http(self: Arc<Self>, listener: TcpListener) -> io::Result<()> {
        loop {
            let (stream, peer) = listener.accept().await?;
            let gw = self.clone();
            tokio::spawn(async move {
                if let Err(e) = gw.handle_http(stream).await {
                    tracing::debug!(%peer, error = %e, "http connection ended");
                }
            });
        }
    }

    async fn handle_socks(&self, mut client: TcpStream) -> Result<(), ProtoError> {
        let (username, password) = socks5::negotiate(&mut client).await?;
        let who = match authenticate(self.selector.config(), &username, &password) {
            Ok(who) => {
                socks5::auth_reply(&mut client, true).await?;
                who
            }
            Err(e) => {
                socks5::auth_reply(&mut client, false).await?;
                return Err(ProtoError::Rejected(e.to_string()));
            }
        };
        let target = socks5::read_request(&mut client).await?;
        match self.open(&who, &target).await {
            Ok((mut up, _choice)) => {
                socks5::reply(&mut client, Reply::Succeeded).await?;
                copy_bidirectional(&mut client, &mut up).await?;
                Ok(())
            }
            Err(e) => {
                socks5::reply(&mut client, e.socks_reply()).await?;
                Err(ProtoError::Rejected(format!("{e:?}")))
            }
        }
    }

    async fn handle_http(&self, mut client: TcpStream) -> Result<(), ProtoError> {
        let req = match http::read_request(&mut client).await {
            Ok(r) => r,
            Err(ProtoError::Protocol(msg)) if msg.starts_with("method not supported") => {
                http::write_response(&mut client, 501, "Not Implemented", &CLOSE).await?;
                return Err(ProtoError::Protocol(msg));
            }
            Err(e) => {
                http::write_response(&mut client, 400, "Bad Request", &CLOSE).await?;
                return Err(e);
            }
        };
        let (target, credentials, forward) = match req {
            http::ProxyRequest::Connect(c) => (c.target, c.credentials, None),
            http::ProxyRequest::Forward(f) => (f.target, f.credentials, Some((f.head, f.body))),
        };
        // Chunked request bodies are not parsed yet, so the end of the body is
        // unknown and a pipelined request after it could reach the origin with
        // Proxy-Authorization intact. Refuse them until a chunked parser lands;
        // clients retry with Content-Length.
        if matches!(forward, Some((_, http::RequestBody::Streamed))) {
            http::write_response(&mut client, 411, "Length Required", &CLOSE).await?;
            close_gracefully(&mut client).await;
            return Err(ProtoError::Protocol("chunked request body".into()));
        }
        // Forwarded requests get explicit framing on gateway-made responses so
        // clients do not wait for a body; CONNECT replies stay as before.
        let extra: &[(&str, &str)] = if forward.is_some() { &CLOSE } else { &[] };
        let Some((username, password)) = credentials else {
            deny(&mut client, extra).await?;
            return Err(ProtoError::Rejected("missing credentials".into()));
        };
        let who = match authenticate(self.selector.config(), &username, &password) {
            Ok(who) => who,
            Err(e) => {
                deny(&mut client, extra).await?;
                return Err(ProtoError::Rejected(e.to_string()));
            }
        };
        let mut up = match self.open(&who, &target).await {
            Ok((up, _choice)) => up,
            Err(e) => {
                let (code, reason) = e.http_status();
                http::write_response(&mut client, code, reason, extra).await?;
                return Err(ProtoError::Rejected(format!("{e:?}")));
            }
        };
        match forward {
            None => {
                http::write_response(&mut client, 200, "Connection Established", &[]).await?;
                copy_bidirectional(&mut client, &mut up).await?;
            }
            Some((head, body)) => {
                up.write_all(&head).await?;
                splice_forward(&mut client, &mut up, body).await?;
            }
        }
        Ok(())
    }
}

/// Send FIN, then drain what the client already sent (bounded) before the
/// socket drops. Closing with unread input makes the kernel send RST, which
/// can destroy a response the client has not read yet.
async fn close_gracefully(client: &mut TcpStream) {
    let _ = client.shutdown().await;
    let mut sink = [0u8; 4096];
    let mut left: usize = 64 * 1024;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    while left > 0 {
        match tokio::time::timeout_at(deadline, client.read(&mut sink)).await {
            Ok(Ok(n)) if n > 0 => left = left.saturating_sub(n),
            _ => break,
        }
    }
}

/// Framing headers for responses the gateway itself makes.
const CLOSE: [(&str, &str); 2] = [("Content-Length", "0"), ("Connection", "close")];

/// 407 with a Basic challenge, plus `extra` headers.
async fn deny(client: &mut TcpStream, extra: &[(&str, &str)]) -> io::Result<()> {
    let mut headers = vec![("Proxy-Authenticate", "Basic realm=\"humanip\"")];
    headers.extend_from_slice(extra);
    http::write_response(client, 407, "Proxy Authentication Required", &headers).await
}

/// After the rewritten head: the origin's response is copied to the client
/// until the origin closes (it was sent `Connection: close`). Client bytes go
/// to the origin only within the declared body, so a second pipelined request
/// (which would still carry `Proxy-Authorization`) is never forwarded.
/// `Transfer-Encoding` bodies are spliced unparsed.
async fn splice_forward(
    client: &mut TcpStream,
    up: &mut TcpStream,
    body: http::RequestBody,
) -> io::Result<()> {
    let (mut cr, mut cw) = client.split();
    let (mut ur, mut uw) = up.split();
    let upload = async {
        match body {
            http::RequestBody::None => {}
            http::RequestBody::Length(n) => {
                tokio::io::copy(&mut (&mut cr).take(n), &mut uw).await?;
            }
            http::RequestBody::Streamed => {
                tokio::io::copy(&mut cr, &mut uw).await?;
            }
        }
        uw.flush().await?;
        std::future::pending::<io::Result<()>>().await
    };
    let download = async {
        tokio::io::copy(&mut ur, &mut cw).await?;
        cw.shutdown().await
    };
    tokio::select! {
        r = download => r,
        r = upload => r,
    }
}

/// Bind both listeners from the config and serve until one fails.
pub async fn run(cfg: GatewayConfig) -> io::Result<()> {
    let socks = TcpListener::bind(&cfg.socks_listen).await?;
    let http = TcpListener::bind(&cfg.http_listen).await?;
    eprintln!(
        "humanip-gateway: SOCKS5 on {}, HTTP CONNECT on {}",
        socks.local_addr()?,
        http.local_addr()?
    );
    let gw = Gateway::new(cfg);
    // Drop idle-expired sticky pins once a minute.
    let sweeper = gw.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            let n = sweeper.selector().sessions().purge_expired(Instant::now());
            if n > 0 {
                tracing::debug!(purged = n, "expired sticky pins");
            }
        }
    });
    tokio::try_join!(gw.clone().serve_socks(socks), gw.serve_http(http))?;
    Ok(())
}
