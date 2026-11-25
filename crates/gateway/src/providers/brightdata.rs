//! Bright Data (residential, ISP and mobile zones share one super proxy).

use super::{addr, alnum_session, check, upstream, AdapterRequest, Protocol};
use crate::Upstream;
use serde::Deserialize;
use std::fmt;

/// HTTP/HTTPS port on the super proxy. The docs also list 22225 (legacy) and
/// 44445; 33335 is the one current "send your first request" examples use.
pub const HTTP_ADDR: &str = "brd.superproxy.io:33335";
/// SOCKS5 port (proxy-side DNS only, "socks5h").
pub const SOCKS5_ADDR: &str = "brd.superproxy.io:22228";
/// Session ids: docs allow "any random string or counter".
/// unverified: no documented charset or length limit; we keep ASCII
/// letters and digits and cap at 32, hashing longer ids.
pub const SESSION_MAX: usize = 32;

/// `kind: brightdata` credentials. `customer` is the id after
/// `brd-customer-` (for example `hl_abc123`), `zone` the zone name.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrightData {
    pub customer: String,
    pub zone: String,
    pub password: String,
    /// Default `http`.
    #[serde(default)]
    pub protocol: Option<Protocol>,
    /// host:port, replaces the default for the chosen protocol.
    #[serde(default)]
    pub endpoint: Option<String>,
}

impl BrightData {
    pub fn validate(&self) -> Result<(), String> {
        check(
            &[
                ("customer", &self.customer),
                ("zone", &self.zone),
                ("password", &self.password),
            ],
            &self.endpoint,
        )
    }
}

impl fmt::Debug for BrightData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BrightData")
            .field("customer", &self.customer)
            .field("zone", &self.zone)
            .field("password", &"<redacted>")
            .field("protocol", &self.protocol)
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

// Docs:
//   https://docs.brightdata.com/proxy-networks/config-options
//   https://docs.brightdata.com/api-reference/proxy/geolocation-targeting
//   https://docs.brightdata.com/proxy-networks/socks5
// Username: brd-customer-<id>-zone-<zone>[-country-<cc>][-session-<id>]
// Country is lowercase ISO alpha-2 in every documented example.
// unverified: the docs show -country- and -session- separately; we emit
// country before session, the order used by third-party integration guides.
pub fn brightdata(c: &BrightData, req: &AdapterRequest<'_>) -> Upstream {
    let protocol = c.protocol.unwrap_or(Protocol::Http);
    let default = match protocol {
        Protocol::Http => HTTP_ADDR,
        Protocol::Socks5 => SOCKS5_ADDR,
    };
    let mut user = format!("brd-customer-{}-zone-{}", c.customer, c.zone);
    if let Some(cc) = req.country {
        user.push_str("-country-");
        user.push_str(&cc.to_ascii_lowercase());
    }
    if req.sticky {
        user.push_str("-session-");
        user.push_str(&alnum_session(req.session, SESSION_MAX));
    }
    upstream(
        protocol,
        addr(&c.endpoint, default),
        user,
        c.password.clone(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds() -> BrightData {
        BrightData {
            customer: "hl_abc123".into(),
            zone: "residential1".into(),
            password: "xyz".into(),
            protocol: None,
            endpoint: None,
        }
    }

    #[test]
    fn sticky_with_country() {
        let req = AdapterRequest {
            session: "hip0123456789abcdef",
            sticky: true,
            country: Some("DE"),
        };
        assert_eq!(
            brightdata(&creds(), &req),
            Upstream::Http {
                addr: "brd.superproxy.io:33335".into(),
                username: "brd-customer-hl_abc123-zone-residential1-country-de-session-hip0123456789abcdef".into(),
                password: "xyz".into(),
            }
        );
    }

    #[test]
    fn debug_hides_password() {
        let d = format!("{:?}", creds());
        assert!(d.contains("residential1") && !d.contains("xyz"));
    }
}
