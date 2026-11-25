//! humanip-core: the routing rules, with no I/O.
//!
//! - [`policy`]: the route policy schema (what a user configures) and how the
//!   0-100 risk dial resolves into concrete limits.
//! - [`pareto`]: offers, eligibility, the Pareto frontier and the
//!   reliability-first default pick.
//! - [`username`]: parsing per-connection settings out of the proxy username.
//! - [`sample`]: a small illustrative offer catalog for demos and tests. The
//!   live price/reliability database is a separate, closed service.

pub mod abuse;
pub mod accounts;
pub mod pareto;
pub mod policy;
pub mod sample;
pub mod username;

/// humanip bills provider cost plus this take rate.
pub const TAKE_RATE: f64 = 0.05;

/// Host part of a URL or bare host, lowercased, without `www.` or port.
pub fn domain_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next().unwrap_or(host);
    host.trim_start_matches("www.").to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::domain_of;

    #[test]
    fn domain_of_strips_scheme_path_port_and_www() {
        assert_eq!(
            domain_of("https://www.LinkedIn.com/in/x?y=1"),
            "linkedin.com"
        );
        assert_eq!(domain_of("web.whatsapp.com"), "web.whatsapp.com");
        assert_eq!(
            domain_of("http://user:pw@example.org:8443/a"),
            "example.org"
        );
    }
}
