//! Accept loops: SOCKS5 and HTTP (CONNECT plus plain-HTTP forwarding) front
//! ends over one selector and one account store.
//!
//! ```text
//! handshake -> authenticate (store; password = API key, username = settings)
//!           -> admit (store: revoked / balance / monthly cap)
//!           -> abuse::check (domain rules vs KYC tier)
//!           -> choose + dial (one retry when policy allows) -> passive health
//!           -> splice through a byte counter -> record_usage on close
//! ```

use crate::accounts_bridge::{self, AbuseCheck, Refusal};
use crate::config::GatewayConfig;
use crate::health::{HealthTracker, Outcome};
use crate::meter::{Billing, Conn};
use crate::metrics::{self, ConnResult, Proto};
use crate::select::{
    target_domain, AuthError, AuthedUser, Choice, DialFailure, SelectError, Selector,
};
use crate::socks5::{self, ProtoError, Reply};
use crate::{http, upstream, TargetAddr};
use humanip_core::abuse;
use humanip_core::accounts::AccountStore;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{copy_bidirectional, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
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

    fn conn_result(&self) -> ConnResult {
        match self {
            OpenError::Select(SelectError::UnknownRoute(_)) => ConnResult::Denied,
            _ => ConnResult::DialFailed,
        }
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

pub struct Gateway {
    selector: Selector,
    health: Arc<HealthTracker>,
    timeout: Duration,
    accounts: Arc<dyn AccountStore>,
    abuse: AbuseCheck,
}

impl Gateway {
    /// Gateway over `accounts` (keys, balances, usage) with the real abuse rules.
    pub fn new(cfg: GatewayConfig, accounts: Arc<dyn AccountStore>) -> Arc<Self> {
        Self::with_abuse_check(cfg, accounts, abuse::check)
    }

    /// Like [`Gateway::new`] with a custom abuse check (tests).
    pub fn with_abuse_check(
        cfg: GatewayConfig,
        accounts: Arc<dyn AccountStore>,
        abuse: AbuseCheck,
    ) -> Arc<Self> {
        let health = Arc::new(HealthTracker::default());
        let timeout = Duration::from_millis(cfg.connect_timeout_ms);
        let selector = Selector::new(Arc::new(cfg), health.clone());
        Arc::new(Gateway {
            selector,
            health,
            timeout,
            accounts,
            abuse,
        })
    }

    pub fn selector(&self) -> &Selector {
        &self.selector
    }

    pub fn health(&self) -> &HealthTracker {
        &self.health
    }

    pub fn accounts(&self) -> &Arc<dyn AccountStore> {
        &self.accounts
    }

    /// Resolve credentials with the account store (off the async runtime).
    pub async fn authenticate(
        &self,
        username: String,
        password: String,
    ) -> Result<AuthedUser, AuthError> {
        let store = self.accounts.clone();
        tokio::task::spawn_blocking(move || {
            accounts_bridge::authenticate(&*store, &username, &password)
        })
        .await
        .unwrap_or_else(|e| Err(AuthError::Store(e.to_string())))
    }

    /// Admission (store) then abuse rules, before any dial.
    pub async fn admit(&self, who: &AuthedUser, target: &TargetAddr) -> Result<(), Refusal> {
        let store = self.accounts.clone();
        let key_id = who.key.key_id.clone();
        let answer = tokio::task::spawn_blocking(move || store.admit(&key_id, now_unix()))
            .await
            .map_err(|e| Refusal::Store(e.to_string()))?;
        accounts_bridge::admission(answer)?;
        let domain = target_domain(&who.user, target);
        let rules = &self.selector.config().blocked_domains;
        (self.abuse)(rules, &domain, who.key.kyc_tier).map_err(Refusal::Blocked)
    }

    fn billing(&self, who: &AuthedUser, choice: &Choice) -> Billing {
        let price = self
            .selector
            .config()
            .provider(&choice.provider)
            .map_or(0.0, |p| p.price_per_gb);
        Billing {
            store: self.accounts.clone(),
            key_id: who.key.key_id.clone(),
            provider: choice.provider.clone(),
            domain: choice.domain.clone(),
            provider_price_per_gb: price,
        }
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
                    metrics::dial_failure(&choice.provider);
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
        let mut conn = Conn::open(Proto::Socks5);
        let (username, password) = socks5::negotiate(&mut client).await?;
        let who = match self.authenticate(username, password).await {
            Ok(who) => {
                socks5::auth_reply(&mut client, true).await?;
                who
            }
            Err(e) => {
                conn.set(auth_result(&e));
                socks5::auth_reply(&mut client, false).await?;
                return Err(ProtoError::Rejected(e.to_string()));
            }
        };
        let target = socks5::read_request(&mut client).await?;
        if let Err(r) = self.admit(&who, &target).await {
            conn.set(r.conn_result());
            socks5::reply(&mut client, r.socks_reply()).await?;
            return Err(ProtoError::Rejected(format!("{r:?}")));
        }
        let (up, choice) = match self.open(&who, &target).await {
            Ok(opened) => opened,
            Err(e) => {
                conn.set(e.conn_result());
                socks5::reply(&mut client, e.socks_reply()).await?;
                return Err(ProtoError::Rejected(format!("{e:?}")));
            }
        };
        let mut up = conn.meter(up, self.billing(&who, &choice));
        conn.set(ConnResult::Ok);
        socks5::reply(&mut client, Reply::Succeeded).await?;
        copy_bidirectional(&mut client, &mut up).await?;
        Ok(())
    }

    async fn handle_http(&self, mut client: TcpStream) -> Result<(), ProtoError> {
        let req = match http::read_request(&mut client).await {
            Ok(r) => r,
            Err(ProtoError::Protocol(msg)) if msg.starts_with("method not supported") => {
                let _conn = Conn::open(Proto::HttpForward);
                http::write_response(&mut client, 501, "Not Implemented", &CLOSE).await?;
                return Err(ProtoError::Protocol(msg));
            }
            Err(e) => {
                let _conn = Conn::open(Proto::HttpForward);
                http::write_response(&mut client, 400, "Bad Request", &CLOSE).await?;
                return Err(e);
            }
        };
        let (target, credentials, forward) = match req {
            http::ProxyRequest::Connect(c) => (c.target, c.credentials, None),
            http::ProxyRequest::Forward(f) => (f.target, f.credentials, Some((f.head, f.body))),
        };
        let mut conn = Conn::open(if forward.is_some() {
            Proto::HttpForward
        } else {
            Proto::HttpConnect
        });
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
            conn.set(ConnResult::AuthFailed);
            deny(&mut client, extra).await?;
            return Err(ProtoError::Rejected("missing credentials".into()));
        };
        let who = match self.authenticate(username, password).await {
            Ok(who) => who,
            Err(e) => {
                conn.set(auth_result(&e));
                if matches!(e, AuthError::Store(_)) {
                    http::write_response(&mut client, 503, "Accounts Unavailable", extra).await?;
                } else {
                    deny(&mut client, extra).await?;
                }
                return Err(ProtoError::Rejected(e.to_string()));
            }
        };
        if let Err(r) = self.admit(&who, &target).await {
            conn.set(r.conn_result());
            let (code, reason) = r.http_status();
            http::write_response(&mut client, code, &reason, extra).await?;
            return Err(ProtoError::Rejected(format!("{r:?}")));
        }
        let (up, choice) = match self.open(&who, &target).await {
            Ok(opened) => opened,
            Err(e) => {
                conn.set(e.conn_result());
                let (code, reason) = e.http_status();
                http::write_response(&mut client, code, reason, extra).await?;
                return Err(ProtoError::Rejected(format!("{e:?}")));
            }
        };
        let mut up = conn.meter(up, self.billing(&who, &choice));
        conn.set(ConnResult::Ok);
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

fn auth_result(e: &AuthError) -> ConnResult {
    match e {
        AuthError::Store(_) => ConnResult::Denied,
        AuthError::BadKey | AuthError::BadUsername(_) => ConnResult::AuthFailed,
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
async fn splice_forward<U: AsyncRead + AsyncWrite + Unpin>(
    client: &mut TcpStream,
    up: &mut U,
    body: http::RequestBody,
) -> io::Result<()> {
    let (mut cr, mut cw) = client.split();
    let (mut ur, mut uw) = tokio::io::split(up);
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

/// The account store for `cfg`: the in-memory dev store seeded from
/// `api_keys`, or (once integrated) the SQLite ledger at `accounts_db`.
pub fn accounts_for(cfg: &GatewayConfig) -> io::Result<Arc<dyn AccountStore>> {
    match &cfg.accounts_db {
        None => Ok(Arc::new(accounts_bridge::memory_from_config(cfg))),
        // INTEGRATION: open the ledger crate's SQLite store here.
        Some(path) => Err(io::Error::other(format!(
            "accounts_db {path:?} is set, but the SQLite ledger is not wired into the gateway \
             yet; remove accounts_db to use the in-memory dev store from api_keys"
        ))),
    }
}

/// Bind both listeners from the config and serve until one fails.
pub async fn run(cfg: GatewayConfig) -> io::Result<()> {
    let accounts = accounts_for(&cfg)?;
    let socks = TcpListener::bind(&cfg.socks_listen).await?;
    let http = TcpListener::bind(&cfg.http_listen).await?;
    eprintln!(
        "humanip-gateway: SOCKS5 on {}, HTTP CONNECT on {}",
        socks.local_addr()?,
        http.local_addr()?
    );
    for p in &cfg.providers {
        if let Some(kind) = p.upstream.unverified_adapter() {
            eprintln!(
                "humanip-gateway: WARNING provider {}: the {kind} adapter's host, port and username \
                 format are unverified. Check them against your {kind} dashboard, or use a generic \
                 socks5/http upstream with the exact values.",
                p.name
            );
        }
    }
    let gw = Gateway::new(cfg, accounts);
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
    // INTEGRATION: spawn the admin server (cfg.admin_listen) here with gw.accounts().
    tokio::try_join!(gw.clone().serve_socks(socks), gw.serve_http(http))?;
    Ok(())
}
