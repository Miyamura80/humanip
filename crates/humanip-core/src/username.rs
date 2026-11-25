//! Per-connection settings carried in the proxy username, so any SOCKS5 or
//! HTTP client works without an SDK:
//!
//! ```text
//! user-route_agent-browser-session_r42-risk_40-country_us-exit_mobile
//! ```
//!
//! Parameters are `key_value` pairs joined by `-`. A dash-separated token whose
//! prefix is not a known key continues the previous value, so route names and
//! hosts may contain dashes (`route_agent-browser`, `target_my-site.com`).

use crate::policy::ExitType;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const KEYS: [&str; 8] = [
    "route", "session", "risk", "country", "city", "asn", "exit", "target",
];

/// Settings parsed from a proxy username. Absent fields inherit the route.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, Default, PartialEq)]
pub struct ProxyUser {
    pub route: Option<String>,
    /// Sticky key: same value, same exit. Absent means rotating.
    pub session: Option<String>,
    pub risk: Option<u8>,
    pub country: Option<String>,
    pub city: Option<String>,
    pub asn: Option<u32>,
    pub exit: Option<ExitType>,
    pub target: Option<String>,
}

impl ProxyUser {
    pub fn is_sticky(&self) -> bool {
        self.session.is_some()
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum UsernameError {
    #[error("username must start with \"user-\"")]
    MissingPrefix,
    #[error("unknown parameter {0:?}")]
    UnknownParam(String),
    #[error("parameter {0} given twice")]
    Duplicate(String),
    #[error("parameter {0} is empty")]
    Empty(String),
    #[error("risk must be an integer 0-100, got {0:?}")]
    BadRisk(String),
    #[error("asn must be a number, got {0:?}")]
    BadAsn(String),
    #[error("country must be a 2-letter code, got {0:?}")]
    BadCountry(String),
    #[error("exit must be mobile, isp or residential, got {0:?}")]
    BadExit(String),
}

/// Parse a proxy username into settings.
pub fn parse(username: &str) -> Result<ProxyUser, UsernameError> {
    let body = username
        .strip_prefix("user-")
        .ok_or(UsernameError::MissingPrefix)?;
    let mut pairs: Vec<(&'static str, String)> = Vec::new();
    for token in body.split('-') {
        let key = token
            .split_once('_')
            .and_then(|(k, _)| KEYS.iter().copied().find(|known| *known == k));
        match (key, pairs.last_mut()) {
            (Some(k), _) => {
                if pairs.iter().any(|(seen, _)| *seen == k) {
                    return Err(UsernameError::Duplicate(k.into()));
                }
                pairs.push((k, token[k.len() + 1..].to_string()));
            }
            (None, Some((_, value))) => {
                value.push('-');
                value.push_str(token);
            }
            (None, None) => return Err(UsernameError::UnknownParam(token.into())),
        }
    }

    let mut out = ProxyUser::default();
    for (key, value) in pairs {
        if value.is_empty() {
            return Err(UsernameError::Empty(key.into()));
        }
        match key {
            "route" => out.route = Some(value),
            "session" => out.session = Some(value),
            "risk" => {
                let r: u8 = value
                    .parse()
                    .map_err(|_| UsernameError::BadRisk(value.clone()))?;
                if r > 100 {
                    return Err(UsernameError::BadRisk(value));
                }
                out.risk = Some(r);
            }
            "country" => {
                if value.len() != 2 || !value.bytes().all(|b| b.is_ascii_alphabetic()) {
                    return Err(UsernameError::BadCountry(value));
                }
                out.country = Some(value.to_ascii_uppercase());
            }
            "city" => out.city = Some(value),
            "asn" => {
                out.asn = Some(
                    value
                        .parse()
                        .map_err(|_| UsernameError::BadAsn(value.clone()))?,
                )
            }
            "exit" => {
                out.exit = Some(match value.as_str() {
                    "mobile" => ExitType::Mobile,
                    "isp" => ExitType::Isp,
                    "residential" => ExitType::Residential,
                    _ => return Err(UsernameError::BadExit(value)),
                })
            }
            "target" => out.target = Some(value.to_ascii_lowercase()),
            _ => unreachable!("key comes from KEYS"),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_username() {
        let u = parse("user-route_agent-browser-session_r42-risk_40-country_us-city_berlin-asn_3320-exit_mobile-target_LinkedIn.com")
            .unwrap();
        assert_eq!(u.route.as_deref(), Some("agent-browser"));
        assert_eq!(u.session.as_deref(), Some("r42"));
        assert_eq!(u.risk, Some(40));
        assert_eq!(u.country.as_deref(), Some("US"));
        assert_eq!(u.city.as_deref(), Some("berlin"));
        assert_eq!(u.asn, Some(3320));
        assert_eq!(u.exit, Some(ExitType::Mobile));
        assert_eq!(u.target.as_deref(), Some("linkedin.com"));
        assert!(u.is_sticky());
    }

    #[test]
    fn no_session_means_rotating() {
        let u = parse("user-route_scrape").unwrap();
        assert!(!u.is_sticky());
    }

    #[test]
    fn dashes_continue_the_previous_value() {
        let u = parse("user-target_my-shop.example-session_a-b").unwrap();
        assert_eq!(u.target.as_deref(), Some("my-shop.example"));
        assert_eq!(u.session.as_deref(), Some("a-b"));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(parse("route_x"), Err(UsernameError::MissingPrefix));
        assert_eq!(
            parse("user-colour_red"),
            Err(UsernameError::UnknownParam("colour_red".into()))
        );
        assert_eq!(
            parse("user-risk_1-risk_2"),
            Err(UsernameError::Duplicate("risk".into()))
        );
        assert_eq!(
            parse("user-risk_101"),
            Err(UsernameError::BadRisk("101".into()))
        );
        assert_eq!(
            parse("user-country_usa"),
            Err(UsernameError::BadCountry("usa".into()))
        );
        assert_eq!(
            parse("user-exit_datacenter"),
            Err(UsernameError::BadExit("datacenter".into()))
        );
        assert_eq!(
            parse("user-session_"),
            Err(UsernameError::Empty("session".into()))
        );
    }
}
