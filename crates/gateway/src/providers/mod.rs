//! Named provider adapters: vendor credentials in, a concrete [`Upstream`] out.
//!
//! ```text
//! UpstreamConfig::{BrightData, Oxylabs, ...}  (config.rs, credentials only)
//!          |  render(session, sticky, country)
//!          v
//! providers::<vendor>(creds, AdapterRequest) -> Upstream { addr, username, password }
//! ```
//!
//! Every adapter is a pure function. Each vendor file cites the public doc it
//! follows and marks anything that could not be confirmed with `unverified:`.
//!
//! Session ids: the selector hands out `hip` + 16 lowercase hex digits. Each
//! adapter maps that id into its vendor's allowed charset and length. When the
//! id does not fit as-is, it is replaced by a deterministic FNV-1a hash of the
//! whole id, so the same `hip...` id always yields the same vendor session and
//! stickiness holds. Shorter vendor ids carry fewer bits (IPRoyal 32, NetNut
//! about 27), so distinct `hip` ids can collide at large session counts.
//!
//! Rotating requests (`sticky: false`) carry no session parameter at all, so
//! the vendor rotates on its own.

pub mod brightdata;
pub mod decodo;
pub mod iproyal;
pub mod netnut;
pub mod oxylabs;
pub mod proxy_cheap;
pub mod rayobyte;

pub use brightdata::{brightdata, BrightData};
pub use decodo::{decodo, Decodo};
pub use iproyal::{iproyal, IpRoyal};
pub use netnut::{netnut, NetNut};
pub use oxylabs::{oxylabs, Oxylabs};
pub use proxy_cheap::{proxy_cheap, ProxyCheap};
pub use rayobyte::{rayobyte, Rayobyte};

use crate::Upstream;
use serde::Deserialize;

/// What the selector wants from one provider connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdapterRequest<'a> {
    /// `hip...` id: stable for sticky routes, fresh per connection otherwise.
    pub session: &'a str,
    /// Keep the same exit across connections. False omits the session param.
    pub sticky: bool,
    /// ISO alpha-2, uppercase, or None for any country.
    pub country: Option<&'a str>,
}

/// Wire protocol to the vendor gateway.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Socks5,
    Http,
}

/// Build the `Upstream` for `protocol`.
pub(crate) fn upstream(
    protocol: Protocol,
    addr: String,
    username: String,
    password: String,
) -> Upstream {
    match protocol {
        Protocol::Socks5 => Upstream::Socks5 {
            addr,
            username,
            password,
        },
        Protocol::Http => Upstream::Http {
            addr,
            username,
            password,
        },
    }
}

/// `endpoint` override when set, else the vendor default for `protocol`.
pub(crate) fn addr(endpoint: &Option<String>, default: &str) -> String {
    endpoint.clone().unwrap_or_else(|| default.to_string())
}

/// Today's behaviour for `kind: socks5 | http`: replace `{session}` and
/// `{country}` (lowercase, "any" when None) in the username template. The
/// template decides the syntax, so `sticky` is not consulted: rotating routes
/// already get a fresh session id per connection.
pub fn generic_template(
    protocol: Protocol,
    addr: &str,
    username: &str,
    password: &str,
    req: &AdapterRequest<'_>,
) -> Upstream {
    let country = req
        .country
        .map_or_else(|| "any".to_string(), |c| c.to_ascii_lowercase());
    let username = username
        .replace("{session}", req.session)
        .replace("{country}", &country);
    upstream(protocol, addr.to_string(), username, password.to_string())
}

/// 64-bit FNV-1a.
pub(crate) fn fnv64(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// ASCII letters and digits only, at most `max` chars. An id that is empty
/// after filtering, or too long, becomes the first `max` hex digits of its hash.
pub(crate) fn alnum_session(id: &str, max: usize) -> String {
    let kept: String = id.chars().filter(char::is_ascii_alphanumeric).collect();
    if !kept.is_empty() && kept.len() <= max {
        return kept;
    }
    hashed_hex(id, max)
}

/// Exactly `len` ASCII letters and digits. A filtered id of any other length
/// becomes `len` hex digits of its hash (`len` up to 16).
pub(crate) fn fixed_alnum_session(id: &str, len: usize) -> String {
    let kept: String = id.chars().filter(char::is_ascii_alphanumeric).collect();
    if kept.len() == len {
        return kept;
    }
    hashed_hex(id, len)
}

/// Decimal id in `1..=max`: kept when `id` already is one, else hashed into range.
pub(crate) fn numeric_session(id: &str, max: u64) -> String {
    match id.parse::<u64>() {
        Ok(n) if (1..=max).contains(&n) && !id.starts_with('0') => n.to_string(),
        _ => (fnv64(id) % max + 1).to_string(),
    }
}

fn hashed_hex(id: &str, max: usize) -> String {
    let mut hex = format!("{:016x}", fnv64(id));
    hex.truncate(max);
    hex
}

/// Shared check for adapter fields: required strings non-empty, endpoint is host:port.
pub(crate) fn check(required: &[(&str, &str)], endpoint: &Option<String>) -> Result<(), String> {
    if let Some((field, _)) = required.iter().find(|(_, v)| v.trim().is_empty()) {
        return Err(format!("{field} must not be empty"));
    }
    if let Some(e) = endpoint {
        let ok = e
            .rsplit_once(':')
            .is_some_and(|(h, p)| !h.is_empty() && p.parse::<u16>().is_ok_and(|p| p > 0));
        if !ok {
            return Err(format!("endpoint {e:?} must be host:port"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HIP: &str = "hip0123456789abcdef";

    #[test]
    fn alnum_keeps_fitting_ids_and_hashes_the_rest() {
        assert_eq!(alnum_session(HIP, 32), HIP);
        assert_eq!(alnum_session("a-b_c", 32), "abc");
        let long = alnum_session(HIP, 8);
        assert_eq!(long.len(), 8);
        assert_eq!(long, alnum_session(HIP, 8));
        assert_ne!(long, alnum_session("hip0123456789abcdee", 8));
        assert_eq!(alnum_session("--", 8).len(), 8);
    }

    #[test]
    fn fixed_and_numeric_sessions() {
        assert_eq!(fixed_alnum_session("abcd1234", 8), "abcd1234");
        let f = fixed_alnum_session(HIP, 8);
        assert_eq!(f.len(), 8);
        assert!(f.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(numeric_session("42", 99_999_999), "42");
        assert_ne!(numeric_session("042", 99_999_999), "042");
        let n: u64 = numeric_session(HIP, 99_999_999).parse().unwrap();
        assert!((1..=99_999_999).contains(&n));
        assert!(numeric_session("0", 10).parse::<u64>().unwrap() <= 10);
    }

    #[test]
    fn generic_template_substitutes() {
        let req = AdapterRequest {
            session: "s1",
            sticky: true,
            country: Some("DE"),
        };
        let up = generic_template(Protocol::Http, "h:1", "u-{session}-{country}", "pw", &req);
        assert_eq!(
            up,
            Upstream::Http {
                addr: "h:1".into(),
                username: "u-s1-de".into(),
                password: "pw".into()
            }
        );
        let any = AdapterRequest {
            country: None,
            ..req
        };
        match generic_template(Protocol::Socks5, "h:1", "{country}", "pw", &any) {
            Upstream::Socks5 { username, .. } => assert_eq!(username, "any"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn check_rejects_empty_and_bad_endpoints() {
        assert!(check(&[("zone", "z")], &None).is_ok());
        assert_eq!(
            check(&[("zone", " ")], &None).unwrap_err(),
            "zone must not be empty"
        );
        for bad in ["nohost", ":80", "h:", "h:0", "h:99999"] {
            assert!(check(&[], &Some(bad.into())).is_err(), "{bad}");
        }
        assert!(check(&[], &Some("eu.example:9000".into())).is_ok());
    }
}
