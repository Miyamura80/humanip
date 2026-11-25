//! Abuse controls: domain rules that require a minimum KYC tier.
//!
//! Upstream proxy vendors require resellers to keep unverified customers away
//! from sensitive targets (banking, government, payments, ...). A deployment
//! lists [`DomainRule`]s; [`check`] decides whether a target host is allowed
//! for an account at a given [`KycTier`], and [`validate_rules`] rejects rule
//! lists that would not match the way their author expects.
//!
//! Matching:
//! - A plain pattern (`chase.com`) matches that host and any subdomain on a
//!   label boundary: `chase.com`, `x.chase.com`, never `notchase.com`.
//! - A leading-dot pattern (`.gov`) matches hosts ending in it, never the bare
//!   suffix (`gov`).
//! - An IP pattern (`192.0.2.1`, `10.0.0.0/8`, `2001:db8::/32`) matches IP
//!   literal targets only. Domain patterns never match IP literals.
//! - With several matches the strictest `min_kyc` wins; ties go to the rule
//!   listed first.
//!
//! Hosts are normalised before matching: lowercased, one trailing dot removed,
//! a `:port` suffix dropped, fullwidth ASCII and ideographic full stops folded
//! to ASCII (the IDNA mappings most often used for evasion). There is no
//! punycode table here, so patterns must be written in ASCII (A-label,
//! `xn--...`) form. A host that still has non-ASCII or other unexpected
//! characters, or an empty label, fails closed: it needs the strictest tier
//! of any rule.

use crate::accounts::KycTier;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};

/// Requests to domains matching `pattern` need at least `min_kyc`.
/// `pattern` is a domain suffix on a label boundary: "chase.com" matches
/// "chase.com" and "www.chase.com" but not "notchase.com"; ".gov" matches any
/// host ending in ".gov" but not "gov" itself. An IP address or CIDR block
/// ("10.0.0.0/8") matches IP-literal targets in that range.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DomainRule {
    pub pattern: String,
    /// Grouping for reporting, e.g. "banking", "government".
    pub category: String,
    /// `verified` blocks everyone but verified accounts; there is no tier
    /// above it, so use `verified` for "always require review".
    pub min_kyc: KycTier,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct Blocked {
    pub category: String,
    pub pattern: String,
    pub required: KycTier,
}

/// `Blocked::category` for a host that could not be normalised to ASCII
/// (non-ASCII or unexpected characters). It fails closed.
pub const UNRECOGNIZED_HOST: &str = "unrecognized_host";

/// Is `domain` allowed for an account at `tier`? `Err` names the strictest
/// matching rule.
pub fn check(rules: &[DomainRule], domain: &str, tier: KycTier) -> Result<(), Blocked> {
    let host = normalize_host(domain);
    let matches = |r: &&DomainRule| match &host {
        Host::Name(name) => domain_matches(&r.pattern, name),
        Host::Ip(ip) => parse_net(&r.pattern).is_some_and(|net| net.contains(*ip)),
        Host::Unrecognized(_) => true,
    };
    let mut strictest: Option<&DomainRule> = None;
    for rule in rules.iter().filter(matches) {
        if strictest.is_none_or(|s| rule.min_kyc > s.min_kyc) {
            strictest = Some(rule);
        }
    }
    match strictest {
        Some(rule) if tier < rule.min_kyc => Err(match host {
            Host::Unrecognized(h) => Blocked {
                category: UNRECOGNIZED_HOST.into(),
                pattern: h,
                required: rule.min_kyc,
            },
            _ => Blocked {
                category: rule.category.clone(),
                pattern: rule.pattern.clone(),
                required: rule.min_kyc,
            },
        }),
        _ => Ok(()),
    }
}

/// Reject rule lists with empty, malformed, non-lowercase, non-ASCII or
/// duplicate patterns. Uppercase is rejected (not folded) so the file reads
/// exactly as it matches.
pub fn validate_rules(rules: &[DomainRule]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for (i, rule) in rules.iter().enumerate() {
        let p = rule.pattern.as_str();
        let err = |why: &str| format!("blocked_domains[{i}] {p:?}: {why}");
        if rule.category.trim().is_empty() {
            return Err(err("category must not be empty"));
        }
        let key = validate_pattern(p).map_err(err)?;
        if !seen.insert(key) {
            return Err(err("duplicate pattern"));
        }
    }
    Ok(())
}

/// Check one pattern; return its canonical form for duplicate detection.
fn validate_pattern(p: &str) -> Result<String, &'static str> {
    if p.is_empty() {
        return Err("empty pattern");
    }
    if !p.is_ascii() {
        return Err("non-ASCII pattern; write it in punycode (xn--) form");
    }
    if p.contains("://") {
        return Err("pattern must be a host, not a URL (remove the scheme)");
    }
    if p.contains('*') {
        return Err("wildcards are not supported; \"x.com\" already covers subdomains");
    }
    if p.bytes().any(|b| b.is_ascii_whitespace()) {
        return Err("pattern contains whitespace");
    }
    if p.bytes().any(|b| b.is_ascii_uppercase()) {
        return Err("pattern must be lowercase");
    }
    if p.contains('/') || p.contains(':') || p.parse::<IpAddr>().is_ok() {
        return match parse_net(p) {
            Some(Net {
                addr: IpAddr::V6(v6),
                ..
            }) if v6.to_ipv4_mapped().is_some() => Err("write IPv4-mapped blocks in IPv4 form"),
            Some(net) if net.is_canonical() => Ok(net.to_string()),
            Some(_) => Err("CIDR block has host bits set below the prefix"),
            None if p
                .split('/')
                .next()
                .is_some_and(|a| a.parse::<IpAddr>().is_ok()) =>
            {
                Err("invalid CIDR prefix length")
            }
            None if p.contains('/') => Err("pattern must be a host, not a path"),
            None => Err("pattern must not include a port"),
        };
    }
    if p.ends_with('.') {
        return Err("remove the trailing dot");
    }
    let body = p.strip_prefix('.').unwrap_or(p);
    if body.is_empty() || body.len() > 253 {
        return Err("pattern needs at least one label and at most 253 characters");
    }
    for label in body.split('.') {
        let b = label.as_bytes();
        if b.is_empty() || b.len() > 63 {
            return Err("each label must be 1 to 63 characters");
        }
        if !b
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        {
            return Err("labels may contain only a-z, 0-9 and '-'");
        }
        if b[0] == b'-' || b[b.len() - 1] == b'-' {
            return Err("labels must not start or end with '-'");
        }
    }
    if body.rsplit('.').next().is_some_and(is_numeric_label) {
        return Err("last label is numeric; use a full IP address or CIDR block");
    }
    Ok(p.to_string())
}

/// A target host after normalisation.
#[derive(Debug, PartialEq)]
enum Host {
    Name(String),
    Ip(IpAddr),
    /// Still non-ASCII, outside `[a-z0-9._-]`, or with empty labels: fails
    /// closed.
    Unrecognized(String),
}

fn normalize_host(domain: &str) -> Host {
    let folded: String = domain.trim().chars().map(fold_char).collect();
    let mut h = folded.to_lowercase();
    if let Some(inner) = h.strip_prefix('[') {
        // "[v6]" or "[v6]:port"
        if let Some((v6, _)) = inner.split_once(']') {
            h = v6.to_string();
        }
    } else if h.matches(':').count() == 1 {
        // "host:port" (a single colon is never valid IPv6)
        if let Some((name, port)) = h.split_once(':') {
            if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) {
                h = name.to_string();
            }
        }
    }
    if h.ends_with('.') {
        h.pop();
    }
    if let Some(ip) = parse_ip_literal(&h) {
        return Host::Ip(ip);
    }
    let ok = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || "-._".contains(c);
    let empty_label = !h.is_empty() && h.split('.').any(str::is_empty);
    if h.chars().all(ok) && !empty_label {
        Host::Name(h)
    } else {
        Host::Unrecognized(h)
    }
}

/// Fullwidth ASCII (U+FF01..U+FF5E) to ASCII; ideographic and halfwidth full
/// stops to '.'. UTS 46 maps these the same way before DNS lookup.
fn fold_char(c: char) -> char {
    match c {
        '\u{3002}' | '\u{FF61}' => '.',
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        _ => c,
    }
}

fn domain_matches(pattern: &str, host: &str) -> bool {
    let p = pattern.trim_end_matches('.').to_ascii_lowercase();
    if p.is_empty() || parse_net(&p).is_some() {
        return false;
    }
    if p.starts_with('.') {
        host.len() > p.len() && host.ends_with(&p)
    } else {
        host == p || (host.ends_with(&p) && host[..host.len() - p.len()].ends_with('.'))
    }
}

/// "123" or "0x7f": a label that makes the name look like an IPv4 literal.
fn is_numeric_label(l: &str) -> bool {
    match l.strip_prefix("0x") {
        Some(hex) => hex.bytes().all(|b| b.is_ascii_hexdigit()),
        None => !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit()),
    }
}

/// An IP literal, including the legacy IPv4 forms resolvers still accept
/// (`127.1`, `0x7f.0.0.1`, `2130706433`), so CIDR rules cannot be dodged by
/// spelling. IPv4-mapped IPv6 becomes IPv4.
fn parse_ip_literal(h: &str) -> Option<IpAddr> {
    let ip = match h.parse::<IpAddr>() {
        Ok(ip) => ip,
        Err(_) => IpAddr::V4(parse_legacy_v4(h)?),
    };
    Some(unmap(ip))
}

fn unmap(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

/// inet_aton(3): 1 to 4 parts, each decimal, octal (leading 0) or hex (0x);
/// the last part fills the remaining bytes.
fn parse_legacy_v4(h: &str) -> Option<Ipv4Addr> {
    let parts: Vec<&str> = h.split('.').collect();
    if parts.len() > 4 {
        return None;
    }
    let mut nums = Vec::with_capacity(parts.len());
    for part in &parts {
        let (digits, radix) = if let Some(x) = part.strip_prefix("0x") {
            (x, 16)
        } else if part.len() > 1 && part.starts_with('0') {
            (&part[1..], 8)
        } else {
            (*part, 10)
        };
        if digits.is_empty() && radix != 16 {
            return None;
        }
        let n = if digits.is_empty() {
            0
        } else {
            u32::from_str_radix(digits, radix).ok()?
        };
        nums.push(n);
    }
    let (last, head) = nums.split_last()?;
    if head.iter().any(|&n| n > 255) {
        return None;
    }
    let tail_bits = 8 * (4 - head.len() as u32);
    if tail_bits < 32 && *last >= 1u32 << tail_bits {
        return None;
    }
    let value = head
        .iter()
        .enumerate()
        .fold(*last, |acc, (i, &n)| acc | (n << (24 - 8 * i as u32)));
    Some(Ipv4Addr::from(value))
}

/// An exact address or CIDR block from a rule pattern.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Net {
    addr: IpAddr,
    prefix: u8,
}

impl Net {
    fn bits(ip: IpAddr) -> (u128, u8) {
        match ip {
            IpAddr::V4(v4) => (u32::from(v4) as u128, 32),
            IpAddr::V6(v6) => (u128::from(v6), 128),
        }
    }

    fn mask(&self) -> u128 {
        let (_, width) = Net::bits(self.addr);
        let full = if width == 32 {
            u32::MAX as u128
        } else {
            u128::MAX
        };
        if self.prefix == 0 {
            0
        } else {
            full & !((1u128 << (width - self.prefix)).wrapping_sub(1))
        }
    }

    fn contains(&self, ip: IpAddr) -> bool {
        let ((net, w1), (host, w2)) = (Net::bits(self.addr), Net::bits(ip));
        w1 == w2 && (net & self.mask()) == (host & self.mask())
    }

    fn is_canonical(&self) -> bool {
        let (net, _) = Net::bits(self.addr);
        net & !self.mask() == 0
    }
}

impl std::fmt::Display for Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix)
    }
}

/// Parse "1.2.3.4", "10.0.0.0/8", "2001:db8::/32" (strict std forms only).
fn parse_net(p: &str) -> Option<Net> {
    let (addr, prefix) = match p.split_once('/') {
        Some((a, len)) => (a, Some(len)),
        None => (p, None),
    };
    let parsed = addr.parse::<IpAddr>().ok()?;
    // A mapped-v6 pattern without a prefix is the same as its IPv4 form.
    let addr = if prefix.is_none() {
        unmap(parsed)
    } else {
        parsed
    };
    let width = if addr.is_ipv4() { 32 } else { 128 };
    let prefix = match prefix {
        None => width,
        Some(len) if !len.is_empty() && len.bytes().all(|b| b.is_ascii_digit()) => {
            len.parse::<u8>().ok().filter(|&n| n <= width)?
        }
        Some(_) => return None,
    };
    Some(Net { addr, prefix })
}
