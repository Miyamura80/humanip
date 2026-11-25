//! Admin HTTP listener: liveness, Prometheus metrics and a token-guarded
//! JSON API for the operator dashboard.
//!
//! ```text
//! GET /healthz                          200 "ok"           no auth
//! GET /metrics                          Prometheus text    no auth (keep private)
//! GET /admin/v1/status                  JSON               Bearer token
//! GET /admin/v1/usage?since=U[&account=ID]  JSON           Bearer token, 503 without a store
//! anything else                         404
//! ```
//!
//! A deliberately small HTTP/1.1 server: one request per connection, request
//! head capped at 8 KiB and 5 s, every response carries `Content-Length` and
//! `Connection: close`. It never serves proxy traffic.

use crate::metrics;
use crate::server::Gateway;
use humanip_core::accounts::AccountStore;
use serde_json::json;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const MAX_HEAD: usize = 8 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const PROM_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// Shared state for one admin listener.
struct Admin {
    gw: Arc<Gateway>,
    token: String,
    accounts: Option<Arc<dyn AccountStore>>,
    started: Instant,
}

/// Serve the admin API on `listener` until it fails. `uptime_secs` counts
/// from this call. An empty `token` rejects every `/admin/v1/*` request.
pub async fn serve_admin(
    listener: TcpListener,
    gw: Arc<Gateway>,
    token: String,
    accounts: Option<Arc<dyn AccountStore>>,
) -> io::Result<()> {
    let admin = Arc::new(Admin {
        gw,
        token,
        accounts,
        started: Instant::now(),
    });
    loop {
        let (stream, peer) = listener.accept().await?;
        let admin = admin.clone();
        tokio::spawn(async move {
            if let Err(e) = admin.handle(stream).await {
                tracing::debug!(%peer, error = %e, "admin connection ended");
            }
        });
    }
}

/// A parsed request head: what routing needs, nothing more.
#[derive(Debug, PartialEq)]
struct Request {
    method: String,
    path: String,
    query: String,
    authorization: Option<String>,
}

struct Response {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
    challenge: bool,
}

impl Response {
    fn text(status: u16, reason: &'static str, body: &str) -> Self {
        Response {
            status,
            reason,
            content_type: "text/plain; charset=utf-8",
            body: body.as_bytes().to_vec(),
            challenge: false,
        }
    }

    fn json(status: u16, reason: &'static str, v: &serde_json::Value) -> Self {
        Response {
            status,
            reason,
            content_type: "application/json",
            body: serde_json::to_vec(v).unwrap_or_default(),
            challenge: false,
        }
    }

    fn error(status: u16, reason: &'static str, msg: &str) -> Self {
        Self::json(status, reason, &json!({ "error": msg }))
    }

    async fn write(&self, s: &mut TcpStream) -> io::Result<()> {
        let mut head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\
             Cache-Control: no-store\r\nConnection: close\r\n",
            self.status,
            self.reason,
            self.content_type,
            self.body.len()
        );
        if self.challenge {
            head.push_str("WWW-Authenticate: Bearer realm=\"humanip-admin\"\r\n");
        }
        head.push_str("\r\n");
        s.write_all(head.as_bytes()).await?;
        s.write_all(&self.body).await?;
        s.flush().await?;
        s.shutdown().await
    }
}

impl Admin {
    async fn handle(&self, mut stream: TcpStream) -> io::Result<()> {
        let resp = match tokio::time::timeout(READ_TIMEOUT, read_head(&mut stream)).await {
            Ok(Ok(head)) => match parse_head(&head) {
                Some(req) => self.route(&req).await,
                None => Response::text(400, "Bad Request", "bad request\n"),
            },
            Ok(Err(e)) if e.kind() == io::ErrorKind::InvalidData => {
                Response::text(431, "Request Header Fields Too Large", "too large\n")
            }
            Ok(Err(e)) => return Err(e),
            Err(_) => Response::text(408, "Request Timeout", "timeout\n"),
        };
        resp.write(&mut stream).await
    }

    async fn route(&self, req: &Request) -> Response {
        if req.method != "GET" {
            return not_found();
        }
        match req.path.as_str() {
            "/healthz" => Response::text(200, "OK", "ok\n"),
            "/metrics" => Response {
                content_type: PROM_CONTENT_TYPE,
                ..Response::text(200, "OK", &metrics::render())
            },
            "/admin/v1/status" | "/admin/v1/usage" if !self.authorized(req) => Response {
                challenge: true,
                ..Response::error(401, "Unauthorized", "missing or invalid bearer token")
            },
            "/admin/v1/status" => Response::json(200, "OK", &self.status()),
            "/admin/v1/usage" => self.usage(&req.query).await,
            _ => not_found(),
        }
    }

    fn authorized(&self, req: &Request) -> bool {
        let Some(h) = req.authorization.as_deref() else {
            return false;
        };
        let Some((scheme, presented)) = h.trim().split_once(' ') else {
            return false;
        };
        scheme.eq_ignore_ascii_case("bearer")
            && !self.token.is_empty()
            && ct_eq(presented.trim().as_bytes(), self.token.as_bytes())
    }

    fn status(&self) -> serde_json::Value {
        let cfg = self.gw.selector().config();
        let providers: Vec<_> = cfg
            .providers
            .iter()
            .map(|p| {
                json!({
                    "name": p.name,
                    "exit_type": p.exit_type,
                    "sourcing": p.sourcing,
                    "price_per_gb": p.price_per_gb,
                    "fraud_score": p.fraud_score,
                    "countries": p.countries,
                    "unverified_adapter": p.upstream.unverified_adapter(),
                })
            })
            .collect();
        let health: Vec<_> = self
            .gw
            .health()
            .snapshot(Instant::now())
            .into_iter()
            .map(|h| {
                json!({
                    "provider": h.provider,
                    "domain": h.domain,
                    "success_rate": h.success_rate,
                    "samples": h.samples,
                })
            })
            .collect();
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_secs": self.started.elapsed().as_secs(),
            "sessions": self.gw.selector().sessions().len(),
            "providers": providers,
            "routes": cfg.routes,
            "health": health,
        })
    }

    async fn usage(&self, query: &str) -> Response {
        let Some(store) = self.accounts.clone() else {
            return Response::error(503, "Service Unavailable", "no account store configured");
        };
        let mut since = None;
        let mut account = None;
        for (k, v) in query_pairs(query) {
            match k.as_str() {
                "since" => since = v.parse::<i64>().ok(),
                "account" if !v.is_empty() => account = Some(v),
                _ => {}
            }
        }
        let Some(since) = since else {
            return Response::error(400, "Bad Request", "since=<unix seconds> is required");
        };
        let rows =
            tokio::task::spawn_blocking(move || store.usage_since(account.as_deref(), since)).await;
        match rows {
            Ok(Ok(rows)) => Response::json(200, "OK", &json!(rows)),
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "usage query failed");
                Response::error(500, "Internal Server Error", "usage query failed")
            }
            Err(_) => Response::error(500, "Internal Server Error", "usage query panicked"),
        }
    }
}

fn not_found() -> Response {
    Response::error(404, "Not Found", "not found")
}

/// Read until the blank line ending the head. `InvalidData` past [`MAX_HEAD`].
async fn read_head(s: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(end) = find(&buf, b"\r\n\r\n") {
            buf.truncate(end);
            return Ok(buf);
        }
        if buf.len() > MAX_HEAD {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "head too large"));
        }
        let n = s.read(&mut chunk).await?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "eof in head"));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn parse_head(head: &[u8]) -> Option<Request> {
    let text = std::str::from_utf8(head).ok()?;
    let mut lines = text.split("\r\n");
    let mut parts = lines.next()?.split(' ');
    let (method, target, version) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || !version.starts_with("HTTP/1.") {
        return None;
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut authorization = None;
    for line in lines.filter(|l| !l.is_empty()) {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("authorization") {
            authorization = Some(value.trim().to_string());
        }
    }
    Some(Request {
        method: method.to_string(),
        path: path.to_string(),
        query: query.to_string(),
        authorization,
    })
}

/// `a=1&b=x%20y` into decoded pairs. `+` means space.
fn query_pairs(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match std::str::from_utf8(&b[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Constant time in the length of the longer input.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= (*a.get(i).unwrap_or(&0) ^ *b.get(i).unwrap_or(&0)) as usize;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_request_head() {
        let r = parse_head(
            b"GET /admin/v1/usage?since=5 HTTP/1.1\r\nHost: x\r\nauthorization: Bearer t\r\n",
        )
        .unwrap();
        assert_eq!(r.method, "GET");
        assert_eq!(r.path, "/admin/v1/usage");
        assert_eq!(r.query, "since=5");
        assert_eq!(r.authorization.as_deref(), Some("Bearer t"));
        assert!(parse_head(b"GET / SPDY/3\r\n").is_none());
        assert!(parse_head(b"GET\r\n").is_none());
    }

    #[test]
    fn decodes_query() {
        let q = query_pairs("since=10&account=acct%2F1&x=a+b&bad=%zz&tail=%4");
        assert_eq!(q[0], ("since".into(), "10".into()));
        assert_eq!(q[1], ("account".into(), "acct/1".into()));
        assert_eq!(q[2], ("x".into(), "a b".into()));
        assert_eq!(q[3], ("bad".into(), "%zz".into()));
        assert_eq!(q[4], ("tail".into(), "%4".into()));
    }

    #[test]
    fn constant_time_compare() {
        assert!(ct_eq(b"secret", b"secret"));
        assert!(!ct_eq(b"secret", b"secreT"));
        assert!(!ct_eq(b"secret", b"secret2"));
        assert!(!ct_eq(b"", b"x"));
    }
}
