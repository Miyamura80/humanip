//! Plain HTTP/1.1 GET over a raw tunnel opened through an [`Upstream`], plus
//! parsers for the IP-echo and geo replies.
//!
//! JSON replies are read with `serde_saphyr`: JSON is valid YAML 1.2, and the
//! crate already depends on it, so no JSON crate is needed.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{upstream, TargetAddr, Upstream};

/// Largest response we read.
const MAX_RESPONSE: u64 = 64 * 1024;

/// An `http://host[:port][/path][?query]` URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpUrl {
    pub host: String,
    pub port: u16,
    /// Origin-form path including any query, always starting with `/`.
    pub path: String,
}

impl HttpUrl {
    /// Parse an `http://` URL. HTTPS is refused: the checks speak plain HTTP
    /// over the raw tunnel so they need no TLS stack.
    pub fn parse(s: &str) -> Result<HttpUrl, String> {
        let rest = s
            .strip_prefix("http://")
            .ok_or_else(|| format!("{s:?}: only http:// URLs are supported"))?;
        let (authority, path) = match rest.find(['/', '?']) {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let path = if path.starts_with('?') {
            format!("/{path}")
        } else {
            path.to_string()
        };
        let (host, port) = match authority.strip_prefix('[') {
            Some(r) => {
                let (h, p) = r
                    .split_once(']')
                    .ok_or_else(|| format!("{s:?}: unclosed [ in host"))?;
                (h, p.strip_prefix(':'))
            }
            None => match authority.rsplit_once(':') {
                Some((h, p)) => (h, Some(p)),
                None => (authority, None),
            },
        };
        if host.is_empty() {
            return Err(format!("{s:?}: missing host"));
        }
        let port = match port {
            None => 80,
            Some(p) => p
                .parse::<u16>()
                .ok()
                .filter(|p| *p > 0)
                .ok_or_else(|| format!("{s:?}: bad port {p:?}"))?,
        };
        Ok(HttpUrl {
            host: host.to_string(),
            port,
            path,
        })
    }

    /// Where the tunnel goes. IP literals become [`TargetAddr::Ip`].
    pub fn target(&self) -> TargetAddr {
        match self.host.parse::<IpAddr>() {
            Ok(ip) => TargetAddr::Ip(SocketAddr::new(ip, self.port)),
            Err(_) => TargetAddr::Domain(self.host.clone(), self.port),
        }
    }

    fn host_header(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        if self.port == 80 {
            host
        } else {
            format!("{host}:{}", self.port)
        }
    }
}

/// Why a fetch failed: the tunnel never opened, or the HTTP exchange broke.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchError {
    Tunnel(String),
    Http(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Tunnel(m) => write!(f, "tunnel: {m}"),
            FetchError::Http(m) => write!(f, "http: {m}"),
        }
    }
}

/// A successful GET.
#[derive(Clone, Debug)]
pub struct Fetched {
    pub body: String,
    /// Time to open the tunnel (TCP plus proxy handshake).
    pub connect_ms: u64,
    /// Time for the whole request.
    pub total_ms: u64,
}

pub(crate) fn ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Open a tunnel to `url` through `up`, send one GET, return the body.
/// The tunnel and the response each get `timeout`.
pub async fn get(up: &Upstream, url: &HttpUrl, timeout: Duration) -> Result<Fetched, FetchError> {
    let start = Instant::now();
    let mut s = upstream::connect(up, &url.target(), timeout)
        .await
        .map_err(|e| FetchError::Tunnel(e.to_string()))?;
    let connect_ms = ms(start);
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: humanip-verify\r\n\
         Accept: application/json\r\nConnection: close\r\n\r\n",
        url.path,
        url.host_header()
    );
    let exchange = async {
        s.write_all(req.as_bytes()).await?;
        s.flush().await?;
        let mut raw = Vec::new();
        (&mut s).take(MAX_RESPONSE).read_to_end(&mut raw).await?;
        Ok::<_, std::io::Error>(raw)
    };
    let raw = tokio::time::timeout(timeout, exchange)
        .await
        .map_err(|_| FetchError::Http("timed out waiting for the response".into()))?
        .map_err(|e| FetchError::Http(e.to_string()))?;
    let body = parse_response(&raw).map_err(FetchError::Http)?;
    Ok(Fetched {
        body,
        connect_ms,
        total_ms: ms(start),
    })
}

/// Status line, headers, body. Requires 2xx; handles Content-Length and
/// chunked bodies.
pub fn parse_response(raw: &[u8]) -> Result<String, String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("response has no complete head")?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| "response head is not utf-8")?;
    let mut body = &raw[split + 4..];
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let mut parts = status_line.split_whitespace();
    let version = parts.next().unwrap_or_default();
    let code: u16 = parts
        .next()
        .and_then(|c| c.parse().ok())
        .filter(|_| version.starts_with("HTTP/1."))
        .ok_or_else(|| format!("bad status line {status_line:?}"))?;
    if !(200..300).contains(&code) {
        return Err(format!("status {code}"));
    }
    let mut chunked = false;
    for line in lines {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        if k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked")
        {
            chunked = true;
        } else if k.eq_ignore_ascii_case("content-length") {
            let n: usize = v.parse().map_err(|_| format!("bad content-length {v:?}"))?;
            if body.len() < n {
                return Err("body shorter than content-length".into());
            }
            body = &body[..n];
        }
    }
    let bytes = if chunked {
        dechunk(body)?
    } else {
        body.to_vec()
    };
    String::from_utf8(bytes).map_err(|_| "body is not utf-8".into())
}

fn dechunk(mut b: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let eol = b
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("truncated chunk size")?;
        let line = std::str::from_utf8(&b[..eol]).map_err(|_| "bad chunk size")?;
        let hex = line.split(';').next().unwrap_or_default().trim();
        let n = usize::from_str_radix(hex, 16).map_err(|_| format!("bad chunk size {hex:?}"))?;
        b = &b[eol + 2..];
        if n == 0 {
            return Ok(out);
        }
        if b.len() < n + 2 {
            return Err("truncated chunk".into());
        }
        out.extend_from_slice(&b[..n]);
        b = &b[n + 2..];
    }
}

#[derive(Deserialize)]
struct IpReply {
    ip: String,
}

/// `{"ip": "203.0.113.7"}` to an address.
pub fn parse_ip(body: &str) -> Result<IpAddr, String> {
    let r: IpReply = serde_saphyr::from_str(body.trim())
        .map_err(|_| format!("expected {{\"ip\": ...}}, got {:?}", clip(body)))?;
    r.ip.trim()
        .parse()
        .map_err(|_| format!("{:?} is not an IP address", r.ip))
}

/// The ip-api.com fields the checks read. Missing fields are `None`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct Geo {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default, rename = "countryCode")]
    pub country_code: Option<String>,
    #[serde(default)]
    pub hosting: Option<bool>,
    #[serde(default)]
    pub proxy: Option<bool>,
    #[serde(default)]
    pub mobile: Option<bool>,
}

/// Parse an ip-api style reply; `"status": "fail"` is an error.
pub fn parse_geo(body: &str) -> Result<Geo, String> {
    let g: Geo = serde_saphyr::from_str(body.trim())
        .map_err(|_| format!("unexpected geo reply {:?}", clip(body)))?;
    if g.status.as_deref() == Some("fail") {
        let why = g.message.as_deref().unwrap_or("no message");
        return Err(format!("geo lookup failed: {why}"));
    }
    Ok(g)
}

fn clip(s: &str) -> String {
    s.trim().chars().take(80).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_parse() {
        let u = HttpUrl::parse("http://api.ipify.org/?format=json").unwrap();
        assert_eq!(
            (u.host.as_str(), u.port, u.path.as_str()),
            ("api.ipify.org", 80, "/?format=json")
        );
        let u = HttpUrl::parse("http://127.0.0.1:9000?x=1").unwrap();
        assert_eq!((u.port, u.path.as_str()), (9000, "/?x=1"));
        assert!(matches!(u.target(), TargetAddr::Ip(_)));
        assert_eq!(u.host_header(), "127.0.0.1:9000");
        let u = HttpUrl::parse("http://[::1]:81/a").unwrap();
        assert_eq!(
            (u.host.as_str(), u.host_header()),
            ("::1", "[::1]:81".into())
        );
        for bad in [
            "https://x/",
            "http://",
            "http://h:0/",
            "http://h:x/",
            "http://[::1/",
        ] {
            assert!(HttpUrl::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn responses_parse() {
        let r = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nbodyextra";
        assert_eq!(parse_response(r).unwrap(), "body");
        let c = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;x\r\nde\r\n0\r\n\r\n";
        assert_eq!(parse_response(c).unwrap(), "abcde");
        assert_eq!(
            parse_response(b"HTTP/1.1 200 OK\r\n\r\nall").unwrap(),
            "all"
        );
        assert!(parse_response(b"HTTP/1.1 429 Too Many\r\n\r\n")
            .unwrap_err()
            .contains("429"));
        assert!(parse_response(b"garbage").is_err());
    }

    #[test]
    fn json_replies_parse() {
        assert_eq!(
            parse_ip(r#"{"ip":"1.2.3.4"}"#).unwrap().to_string(),
            "1.2.3.4"
        );
        assert_eq!(
            parse_ip("{\"ip\": \"2001:db8::1\"}\n").unwrap().to_string(),
            "2001:db8::1"
        );
        assert!(parse_ip(r#"{"ip":"nope"}"#).is_err());
        assert!(parse_ip("<html>").is_err());
        let g = parse_geo(r#"{"status":"success","countryCode":"DE","hosting":true,"proxy":false,"mobile":false}"#).unwrap();
        assert_eq!(g.country_code.as_deref(), Some("DE"));
        assert_eq!(g.hosting, Some(true));
        let f = parse_geo(r#"{"status":"fail","message":"private range"}"#).unwrap_err();
        assert!(f.contains("private range"));
    }
}
