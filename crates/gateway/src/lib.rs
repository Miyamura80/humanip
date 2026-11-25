//! humanip gateway: the data plane.
//!
//! ```text
//! client --SOCKS5/HTTP CONNECT--> server --> select (policy, sticky table, health)
//!                                          --> upstream (provider SOCKS5/HTTP, or direct in dev)
//! ```
//!
//! Module ownership:
//! - [`socks5`], [`http`], [`upstream`]: wire protocols and dialing.
//! - [`config`], [`session`], [`select`]: configuration, auth, sticky pins, exit choice.
//! - [`health`]: passive per-(provider, domain) success tracking.
//! - [`server`]: accept loops that wire the above together.

pub mod accounts_bridge;
pub mod admin;
pub mod config;
pub mod health;
pub mod http;
pub mod meter;
pub mod metrics;
pub mod providers;
pub mod select;
pub mod server;
pub mod session;
pub mod socks5;
pub mod upstream;
pub mod verify;

use std::fmt;
use std::net::SocketAddr;

/// Where the client wants to go.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TargetAddr {
    Ip(SocketAddr),
    Domain(String, u16),
}

impl TargetAddr {
    /// Host part as a string (IP literal or domain).
    pub fn host(&self) -> String {
        match self {
            TargetAddr::Ip(a) => a.ip().to_string(),
            TargetAddr::Domain(h, _) => h.clone(),
        }
    }

    pub fn port(&self) -> u16 {
        match self {
            TargetAddr::Ip(a) => a.port(),
            TargetAddr::Domain(_, p) => *p,
        }
    }
}

impl fmt::Display for TargetAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TargetAddr::Ip(a) => write!(f, "{a}"),
            TargetAddr::Domain(h, p) => write!(f, "{h}:{p}"),
        }
    }
}

/// Where the gateway sends a connection next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Upstream {
    /// Dial the target from this machine. Dev and tests only; this is a
    /// datacenter exit, so configs must opt in with `allow_direct: true`.
    Direct,
    /// A provider's SOCKS5 endpoint.
    Socks5 {
        addr: String,
        username: String,
        password: String,
    },
    /// A provider's HTTP CONNECT endpoint.
    Http {
        addr: String,
        username: String,
        password: String,
    },
}

/// Observed share of requests that succeed for a (domain, provider) pair.
/// Implemented by [`health::HealthTracker`]; consumed by [`select`].
pub trait SuccessSource: Send + Sync {
    /// `None` when there is not enough data yet.
    fn success_rate(&self, domain: &str, provider: &str) -> Option<f64>;
}

/// Success rate assumed for a pair with no data.
pub const NEUTRAL_SUCCESS: f64 = 0.8;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_display_and_parts() {
        let d = TargetAddr::Domain("example.com".into(), 443);
        assert_eq!(d.to_string(), "example.com:443");
        assert_eq!((d.host(), d.port()), ("example.com".to_string(), 443));
        let i = TargetAddr::Ip("127.0.0.1:80".parse().unwrap());
        assert_eq!(i.to_string(), "127.0.0.1:80");
    }
}
