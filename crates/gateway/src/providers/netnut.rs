//! NetNut rotating residential proxies.

use super::{addr, check, numeric_session, upstream, AdapterRequest, Protocol};
use crate::Upstream;
use serde::Deserialize;
use std::fmt;

/// unverified: help.netnut.io could not be fetched. Third-party guides
/// disagree on the host (gw.netnut.net, regional gw-am.netnut.net, older
/// gw.ntnt.io) but agree on port 5959. Use `endpoint` for the host your
/// dashboard's endpoint generator shows.
pub const ADDR: &str = "gw.netnut.net:5959";
/// Guides: the sticky id is "any number between 1-99999999".
pub const SESSION_MAX: u64 = 99_999_999;

/// `kind: netnut` credentials (the proxy user and password from the dashboard).
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetNut {
    pub username: String,
    pub password: String,
    /// Default `http`.
    /// unverified: SOCKS5 on the same port is reported by reviews only.
    #[serde(default)]
    pub protocol: Option<Protocol>,
    #[serde(default)]
    pub endpoint: Option<String>,
}

impl NetNut {
    pub fn validate(&self) -> Result<(), String> {
        check(
            &[("username", &self.username), ("password", &self.password)],
            &self.endpoint,
        )
    }
}

impl fmt::Debug for NetNut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetNut")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("protocol", &self.protocol)
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

// Docs:
//   https://help.netnut.io/netnut-documentation/netnut-proxies/residential-proxies/endpoint-generator
//   unverified: that page could not be fetched; the syntax below follows
//   https://scrapfly.io/blog/posts/how-to-optimize-netnut-proxies and
//   https://proxyway.com/reviews/netnut-proxies
// Username: <user>-res-<cc|any>[-sid-<1..99999999>]
// `res` selects the rotating residential pool and is always followed by a
// country, lowercase, or `any`. A `hip` id is hashed into 1..=99999999
// (about 27 bits).
pub fn netnut(c: &NetNut, req: &AdapterRequest<'_>) -> Upstream {
    let protocol = c.protocol.unwrap_or(Protocol::Http);
    let cc = req
        .country
        .map_or_else(|| "any".to_string(), |c| c.to_ascii_lowercase());
    let mut user = format!("{}-res-{cc}", c.username);
    if req.sticky {
        user.push_str("-sid-");
        user.push_str(&numeric_session(req.session, SESSION_MAX));
    }
    upstream(protocol, addr(&c.endpoint, ADDR), user, c.password.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_session_kept() {
        let c = NetNut {
            username: "u".into(),
            password: "xyz".into(),
            protocol: None,
            endpoint: None,
        };
        let req = AdapterRequest {
            session: "12345",
            sticky: true,
            country: None,
        };
        match netnut(&c, &req) {
            Upstream::Http { username, .. } => assert_eq!(username, "u-res-any-sid-12345"),
            other => panic!("{other:?}"),
        }
        assert!(!format!("{c:?}").contains("xyz"));
    }
}
