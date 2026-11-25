//! Oxylabs residential proxies.

use super::{addr, alnum_session, check, upstream, AdapterRequest, Protocol};
use crate::Upstream;
use serde::Deserialize;
use std::fmt;

/// Residential entry node. HTTP, HTTPS and SOCKS5 all use port 7777.
pub const ADDR: &str = "pr.oxylabs.io:7777";
/// Docs: "randomly created alphanumeric string".
/// unverified: no documented length limit; we cap at 32, hashing longer ids.
pub const SESSION_MAX: usize = 32;

/// `kind: oxylabs` credentials. `username` is the proxy user without the
/// `customer-` prefix (a leading `customer-` is accepted and not doubled).
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Oxylabs {
    pub username: String,
    pub password: String,
    /// Default `http`.
    #[serde(default)]
    pub protocol: Option<Protocol>,
    /// host:port, for example a country entry node.
    #[serde(default)]
    pub endpoint: Option<String>,
}

impl Oxylabs {
    pub fn validate(&self) -> Result<(), String> {
        check(
            &[("username", &self.username), ("password", &self.password)],
            &self.endpoint,
        )
    }
}

impl fmt::Debug for Oxylabs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Oxylabs")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("protocol", &self.protocol)
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

// Docs:
//   https://developers.oxylabs.io/products/proxies/residential-proxies/session-control
//   https://developers.oxylabs.io/products/proxies/residential-proxies/protocols
//   https://developers.oxylabs.io/get-started/quick-start-proxies
// Username: customer-<user>[-cc-<CC>][-sessid-<id>]
// Country is uppercase in the docs ("customer-USERNAME-cc-DE").
// No -sesstime- is sent: the documented default sticky time is 10 minutes
// (up to 1440 with -sesstime-), and the gateway re-pins on loss.
pub fn oxylabs(c: &Oxylabs, req: &AdapterRequest<'_>) -> Upstream {
    let protocol = c.protocol.unwrap_or(Protocol::Http);
    let base = c.username.strip_prefix("customer-").unwrap_or(&c.username);
    let mut user = format!("customer-{base}");
    if let Some(cc) = req.country {
        user.push_str("-cc-");
        user.push_str(&cc.to_ascii_uppercase());
    }
    if req.sticky {
        user.push_str("-sessid-");
        user.push_str(&alnum_session(req.session, SESSION_MAX));
    }
    upstream(protocol, addr(&c.endpoint, ADDR), user, c.password.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_not_doubled() {
        let c = Oxylabs {
            username: "customer-myuser".into(),
            password: "xyz".into(),
            protocol: None,
            endpoint: None,
        };
        let req = AdapterRequest {
            session: "hip0123456789abcdef",
            sticky: false,
            country: None,
        };
        match oxylabs(&c, &req) {
            Upstream::Http { username, .. } => assert_eq!(username, "customer-myuser"),
            other => panic!("{other:?}"),
        }
        assert!(!format!("{c:?}").contains("xyz"));
    }
}
