//! Decodo (formerly Smartproxy) residential proxies.

use super::{addr, alnum_session, check, upstream, AdapterRequest, Protocol};
use crate::Upstream;
use serde::Deserialize;
use std::fmt;

/// Backconnect gateway. HTTP(S) and SOCKS5 share port 7000; country hosts
/// such as us.decodo.com do not serve SOCKS5, so targeting goes in the username.
pub const ADDR: &str = "gate.decodo.com:7000";
/// Docs: "the session ID can be defined by any string of your choice".
/// unverified: no documented charset or length; we keep ASCII letters and
/// digits (a `-` would split the parameter list) and cap at 32.
pub const SESSION_MAX: usize = 32;

/// `kind: decodo` credentials. `username` without the `user-` prefix (a
/// leading `user-` is accepted and not doubled).
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decodo {
    pub username: String,
    pub password: String,
    /// Default `http`.
    #[serde(default)]
    pub protocol: Option<Protocol>,
    #[serde(default)]
    pub endpoint: Option<String>,
}

impl Decodo {
    pub fn validate(&self) -> Result<(), String> {
        check(
            &[("username", &self.username), ("password", &self.password)],
            &self.endpoint,
        )
    }
}

impl fmt::Debug for Decodo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Decodo")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("protocol", &self.protocol)
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

// Docs:
//   https://help.decodo.com/docs/residential-proxy-quick-start
//   https://help.decodo.com/docs/residential-proxy-custom-sticky-sessions
//   https://help.decodo.com/docs/mobile-proxy-protocols
// Username: user-<user>[-country-<cc>][-session-<id>]
// Country is lowercase in the docs ("user-username-country-us").
// No -sessionduration- is sent: the documented default is 10 minutes
// (1 to 1440 allowed), and the gateway re-pins on loss.
pub fn decodo(c: &Decodo, req: &AdapterRequest<'_>) -> Upstream {
    let protocol = c.protocol.unwrap_or(Protocol::Http);
    let base = c.username.strip_prefix("user-").unwrap_or(&c.username);
    let mut user = format!("user-{base}");
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
    fn prefix_not_doubled_and_debug_redacts() {
        let c = Decodo {
            username: "user-sp1".into(),
            password: "xyz".into(),
            protocol: Some(Protocol::Socks5),
            endpoint: None,
        };
        let req = AdapterRequest {
            session: "s",
            sticky: true,
            country: Some("US"),
        };
        assert_eq!(
            decodo(&c, &req),
            Upstream::Socks5 {
                addr: ADDR.into(),
                username: "user-sp1-country-us-session-s".into(),
                password: "xyz".into(),
            }
        );
        assert!(!format!("{c:?}").contains("xyz"));
    }
}
