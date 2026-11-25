//! Rayobyte residential proxies. Targeting goes in the password.

use super::{addr, alnum_session, check, upstream, AdapterRequest, Protocol};
use crate::Upstream;
use serde::Deserialize;
use std::fmt;

/// HTTP gateway from the support portal's configuration guide.
/// unverified: newer docs.rayobyte.com examples are reported to use a
/// different host (us-east.gw.rayobyte.com:8000); use `endpoint` if your
/// dashboard shows another one.
pub const HTTP_ADDR: &str = "la.residential.rayobyte.com:8000";
/// Docs: residential proxies accept SOCKS5 on port 1080.
/// unverified: that the SOCKS5 port is served on the same host as HTTP.
pub const SOCKS5_ADDR: &str = "la.residential.rayobyte.com:1080";
/// unverified: no documented charset or length; examples are short
/// alphanumeric strings ("ABCDEFG", "DonUarN5"). We keep letters and digits
/// and cap at 32, hashing longer ids.
pub const SESSION_MAX: usize = 32;

/// `kind: rayobyte` credentials (the proxy user and password from the dashboard).
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rayobyte {
    pub username: String,
    pub password: String,
    /// Default `http`.
    #[serde(default)]
    pub protocol: Option<Protocol>,
    #[serde(default)]
    pub endpoint: Option<String>,
}

impl Rayobyte {
    pub fn validate(&self) -> Result<(), String> {
        check(
            &[("username", &self.username), ("password", &self.password)],
            &self.endpoint,
        )
    }
}

impl fmt::Debug for Rayobyte {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rayobyte")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("protocol", &self.protocol)
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

// Docs:
//   https://portal.rayobyte.com/en/support/solutions/articles/64000307556-residential-proxy-configuration
//   https://portal.rayobyte.com/en/support/solutions/articles/64000311728-residential-proxies-advanced-targeting-socks
//   https://docs.rayobyte.com/proxies/targeting/
// Username is passed through unchanged. Password:
//   <password>[-country-<CC>][-session-<id>]
// Geo parameters must come before session parameters. Country is uppercase
// in the docs ("password123-country-US"). No -duration- is sent; the
// documented range is 1 to 60 minutes and the gateway re-pins on loss.
pub fn rayobyte(c: &Rayobyte, req: &AdapterRequest<'_>) -> Upstream {
    let protocol = c.protocol.unwrap_or(Protocol::Http);
    let default = match protocol {
        Protocol::Http => HTTP_ADDR,
        Protocol::Socks5 => SOCKS5_ADDR,
    };
    let mut pass = c.password.clone();
    if let Some(cc) = req.country {
        pass.push_str("-country-");
        pass.push_str(&cc.to_ascii_uppercase());
    }
    if req.sticky {
        pass.push_str("-session-");
        pass.push_str(&alnum_session(req.session, SESSION_MAX));
    }
    upstream(
        protocol,
        addr(&c.endpoint, default),
        c.username.clone(),
        pass,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn username_untouched_and_debug_redacts() {
        let c = Rayobyte {
            username: "username123".into(),
            password: "password123".into(),
            protocol: None,
            endpoint: None,
        };
        let req = AdapterRequest {
            session: "DonUarN5",
            sticky: true,
            country: Some("us"),
        };
        assert_eq!(
            rayobyte(&c, &req),
            Upstream::Http {
                addr: HTTP_ADDR.into(),
                username: "username123".into(),
                password: "password123-country-US-session-DonUarN5".into(),
            }
        );
        assert!(!format!("{c:?}").contains("password123"));
    }
}
