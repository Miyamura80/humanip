//! Proxy-Cheap rotating residential proxies.
//!
//! unverified: Proxy-Cheap publishes no gateway host, port or parameter
//! syntax; its help center says to copy all four values from the dashboard's
//! credential generator. Everything below is the most common vendor format,
//! not a confirmed Proxy-Cheap one. If the generated strings differ, set
//! `endpoint`, or use `kind: http | socks5` with a `{session}` template.

use super::{addr, alnum_session, check, upstream, AdapterRequest, Protocol};
use crate::Upstream;
use serde::Deserialize;
use std::fmt;

/// unverified: placeholder host and port, not taken from Proxy-Cheap docs.
pub const ADDR: &str = "proxy.proxy-cheap.com:31112";
/// unverified: no documented charset or length; we keep letters and digits
/// and cap at 32, hashing longer ids.
pub const SESSION_MAX: usize = 32;

/// `kind: proxy_cheap` credentials (the proxy user and password from the
/// dashboard, not the account login).
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyCheap {
    pub username: String,
    pub password: String,
    /// Default `http`. The help center says HTTP and SOCKS5 are offered
    /// "where available".
    #[serde(default)]
    pub protocol: Option<Protocol>,
    #[serde(default)]
    pub endpoint: Option<String>,
}

impl ProxyCheap {
    pub fn validate(&self) -> Result<(), String> {
        check(
            &[("username", &self.username), ("password", &self.password)],
            &self.endpoint,
        )
    }
}

impl fmt::Debug for ProxyCheap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxyCheap")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("protocol", &self.protocol)
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

// Docs:
//   https://support.proxy-cheap.com/hc/en-us/articles/30423207064349-How-to-use-residential-rotating-proxies-Step-by-Step-Tutorial
// unverified: username <user>[-country-<cc>][-session-<id>], lowercase
// country. This is the common username-suffix format other vendors document
// (Decodo, Bright Data), not one Proxy-Cheap publishes.
pub fn proxy_cheap(c: &ProxyCheap, req: &AdapterRequest<'_>) -> Upstream {
    let protocol = c.protocol.unwrap_or(Protocol::Http);
    let mut user = c.username.clone();
    if let Some(cc) = req.country {
        user.push_str("-country-");
        user.push_str(&cc.to_ascii_lowercase());
    }
    if req.sticky {
        user.push_str("-session-");
        user.push_str(&alnum_session(req.session, SESSION_MAX));
    }
    upstream(protocol, addr(&c.endpoint, ADDR), user, c.password.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotating_and_debug() {
        let c = ProxyCheap {
            username: "pc1".into(),
            password: "xyz".into(),
            protocol: None,
            endpoint: None,
        };
        let req = AdapterRequest {
            session: "hip1",
            sticky: false,
            country: None,
        };
        assert_eq!(
            proxy_cheap(&c, &req),
            Upstream::Http {
                addr: ADDR.into(),
                username: "pc1".into(),
                password: "xyz".into(),
            }
        );
        assert!(!format!("{c:?}").contains("xyz"));
    }
}
