//! IPRoyal residential proxies. Targeting goes in the password.

use super::{addr, check, fixed_alnum_session, upstream, AdapterRequest, Protocol};
use crate::Upstream;
use serde::Deserialize;
use std::fmt;

/// HTTP/HTTPS gateway.
pub const HTTP_ADDR: &str = "geo.iproyal.com:12321";
/// SOCKS5 gateway (same host, separate port).
pub const SOCKS5_ADDR: &str = "geo.iproyal.com:32325";
/// Docs: the session is an 8-character code of letters and digits.
pub const SESSION_LEN: usize = 8;
/// Sticky lifetime we request. Docs allow 1 second to 7 days, one unit only.
/// This value is our choice, not a vendor default: long enough for sticky
/// routes, and the gateway re-pins if the vendor drops the exit earlier.
pub const LIFETIME: &str = "24h";

/// `kind: iproyal` credentials (the proxy user and password from the dashboard).
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IpRoyal {
    pub username: String,
    pub password: String,
    /// Default `http`.
    #[serde(default)]
    pub protocol: Option<Protocol>,
    #[serde(default)]
    pub endpoint: Option<String>,
}

impl IpRoyal {
    pub fn validate(&self) -> Result<(), String> {
        check(
            &[("username", &self.username), ("password", &self.password)],
            &self.endpoint,
        )
    }
}

impl fmt::Debug for IpRoyal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IpRoyal")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("protocol", &self.protocol)
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

// Docs:
//   https://docs.iproyal.com/proxies/residential/proxy/rotation
//   https://docs.iproyal.com/proxies/residential/proxy/protocols
//   https://docs.iproyal.com/proxies/residential/proxy/making-requests
// Username is passed through unchanged. Password:
//   <password>[_country-<cc>][_session-<8 alnum>_lifetime-<n><unit>]
// Country is lowercase in the docs. A `hip` id (19 chars) never fits 8, so
// it becomes the first 8 hex digits of its FNV-1a hash (32 bits).
pub fn iproyal(c: &IpRoyal, req: &AdapterRequest<'_>) -> Upstream {
    let protocol = c.protocol.unwrap_or(Protocol::Http);
    let default = match protocol {
        Protocol::Http => HTTP_ADDR,
        Protocol::Socks5 => SOCKS5_ADDR,
    };
    let mut pass = c.password.clone();
    if let Some(cc) = req.country {
        pass.push_str("_country-");
        pass.push_str(&cc.to_ascii_lowercase());
    }
    if req.sticky {
        pass.push_str("_session-");
        pass.push_str(&fixed_alnum_session(req.session, SESSION_LEN));
        pass.push_str("_lifetime-");
        pass.push_str(LIFETIME);
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
    fn eight_char_session_kept() {
        let c = IpRoyal {
            username: "u".into(),
            password: "pw".into(),
            protocol: None,
            endpoint: None,
        };
        let req = AdapterRequest {
            session: "abcd1234",
            sticky: true,
            country: None,
        };
        match iproyal(&c, &req) {
            Upstream::Http { password, .. } => {
                assert_eq!(password, "pw_session-abcd1234_lifetime-24h")
            }
            other => panic!("{other:?}"),
        }
        assert!(!format!("{c:?}").contains("pw"));
    }
}
