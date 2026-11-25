use humanip_core::abuse::{check, validate_rules, Blocked, DomainRule, UNRECOGNIZED_HOST};
use humanip_core::accounts::KycTier::{self, Basic, None as NoKyc, Verified};
use serde::Deserialize;

fn rule(pattern: &str, category: &str, min_kyc: KycTier) -> DomainRule {
    DomainRule {
        pattern: pattern.into(),
        category: category.into(),
        min_kyc,
    }
}

fn allowed(rules: &[DomainRule], host: &str, tier: KycTier) -> bool {
    check(rules, host, tier).is_ok()
}

fn bank() -> Vec<DomainRule> {
    vec![rule("chase.com", "banking", Verified)]
}

#[test]
fn plain_pattern_matches_on_label_boundary() {
    let r = bank();
    assert!(!allowed(&r, "chase.com", NoKyc));
    assert!(!allowed(&r, "x.chase.com", NoKyc));
    assert!(!allowed(&r, "a.b.chase.com", Basic));
    assert!(allowed(&r, "notchase.com", NoKyc));
    assert!(allowed(&r, "chase.com.evil.net", NoKyc));
    assert!(allowed(&r, "hase.com", NoKyc));
    assert!(allowed(&r, "com", NoKyc));
}

#[test]
fn leading_dot_pattern_needs_a_label_before_it() {
    let r = vec![rule(".gov", "government", Verified)];
    assert!(!allowed(&r, "irs.gov", NoKyc));
    assert!(!allowed(&r, "www.irs.gov", NoKyc));
    assert!(allowed(&r, "gov", NoKyc));
    assert!(allowed(&r, "gov.uk", NoKyc));
    assert!(allowed(&r, "notgov", NoKyc));
    assert!(allowed(&r, "gov.example.com", NoKyc));
}

#[test]
fn strictest_tier_wins_across_overlapping_rules() {
    let r = vec![
        rule("example.com", "general", Basic),
        rule("pay.example.com", "payments", Verified),
        rule(".com", "noop", NoKyc),
    ];
    let err = check(&r, "x.pay.example.com", Basic).unwrap_err();
    assert_eq!(
        err,
        Blocked {
            category: "payments".into(),
            pattern: "pay.example.com".into(),
            required: Verified,
        }
    );
    // Order must not matter.
    let mut rev = r.clone();
    rev.reverse();
    assert_eq!(check(&rev, "x.pay.example.com", NoKyc).unwrap_err(), err);
    // Only the looser rule matches here.
    let err = check(&r, "www.example.com", NoKyc).unwrap_err();
    assert_eq!((err.category.as_str(), err.required), ("general", Basic));
    assert!(allowed(&r, "www.example.com", Basic));
    assert!(allowed(&r, "other.com", NoKyc));
}

#[test]
fn ties_report_the_first_listed_rule() {
    let r = vec![
        rule("example.com", "first", Verified),
        rule("a.example.com", "second", Verified),
    ];
    assert_eq!(
        check(&r, "a.example.com", NoKyc).unwrap_err().category,
        "first"
    );
}

#[test]
fn each_tier_allowed_or_denied() {
    let r = vec![
        rule("open.com", "open", NoKyc),
        rule("basic.com", "b", Basic),
        rule("verified.com", "v", Verified),
    ];
    let cases = [
        ("open.com", [true, true, true]),
        ("basic.com", [false, true, true]),
        ("verified.com", [false, false, true]),
        ("unlisted.com", [true, true, true]),
    ];
    for (host, expect) in cases {
        for (tier, ok) in [NoKyc, Basic, Verified].into_iter().zip(expect) {
            assert_eq!(allowed(&r, host, tier), ok, "{host} at {tier:?}");
        }
    }
    let err = check(&r, "basic.com", NoKyc).unwrap_err();
    assert_eq!(err.required, Basic);
}

#[test]
fn host_case_trailing_dot_and_port_are_normalised() {
    let r = bank();
    for host in [
        "CHASE.COM",
        "Www.Chase.Com",
        "chase.com.",
        "x.chase.com.",
        " chase.com ",
        "chase.com:443",
    ] {
        assert!(!allowed(&r, host, NoKyc), "{host}");
    }
    // Only one trailing dot is stripped; empty labels left over fail closed.
    for host in ["chase.com..", ".chase.com", "x..chase.com"] {
        let err = check(&r, host, NoKyc).unwrap_err();
        assert_eq!(err.category, UNRECOGNIZED_HOST, "{host}");
    }
}

#[test]
fn rule_case_and_trailing_dot_are_tolerated_in_check() {
    // validate_rules rejects these, but check stays safe on raw input.
    let r = vec![rule("Chase.COM.", "banking", Verified)];
    assert!(!allowed(&r, "www.chase.com", NoKyc));
}

#[test]
fn fullwidth_and_ideographic_dots_fold_to_ascii() {
    let r = bank();
    assert!(!allowed(&r, "chase\u{3002}com", NoKyc));
    assert!(!allowed(
        &r,
        "\u{FF43}\u{FF48}\u{FF41}\u{FF53}\u{FF45}.com",
        NoKyc
    ));
    assert!(!allowed(&r, "www.chase\u{FF0E}com", NoKyc));
}

#[test]
fn non_ascii_hosts_fail_closed() {
    let r = vec![
        rule("chase.com", "banking", Verified),
        rule("coinbase.com", "crypto", Basic),
    ];
    let err = check(&r, "b\u{fc}cher.example", Basic).unwrap_err();
    assert_eq!(err.category, UNRECOGNIZED_HOST);
    assert_eq!(err.required, Verified);
    assert!(allowed(&r, "b\u{fc}cher.example", Verified));
    assert!(!allowed(&r, "chase.com/x", NoKyc));
    // No rules, nothing to fail closed against.
    assert!(allowed(&[], "b\u{fc}cher.example", NoKyc));
    // Punycode is plain ASCII and matches normally.
    assert!(allowed(&r, "xn--bcher-kva.example", NoKyc));
}

#[test]
fn ip_literals_never_match_domain_rules() {
    let r = vec![
        rule(".gov", "government", Verified),
        rule("1.2.3.4", "weird", Verified),
        rule("4", "weird", Verified),
    ];
    for host in ["1.2.3.4", "10.0.0.1", "[::1]", "::1", "[2001:db8::1]:443"] {
        // "1.2.3.4" is an exact IP rule, so it alone is blocked.
        let expect = host != "1.2.3.4";
        assert_eq!(allowed(&r, host, NoKyc), expect, "{host}");
    }
    let r = vec![rule("4", "suffix", Verified)];
    assert!(allowed(&r, "1.2.3.4", NoKyc));
}

#[test]
fn ip_and_cidr_rules() {
    let r = vec![
        rule("192.0.2.7", "exact", Verified),
        rule("10.0.0.0/8", "v4net", Basic),
        rule("2001:db8::/32", "v6net", Verified),
        rule("0.0.0.0/0", "all_v4", NoKyc),
    ];
    assert!(!allowed(&r, "192.0.2.7", Basic));
    assert!(allowed(&r, "192.0.2.8", NoKyc));
    assert!(!allowed(&r, "10.255.1.2", NoKyc));
    assert!(allowed(&r, "10.255.1.2", Basic));
    assert!(allowed(&r, "11.0.0.1", NoKyc));
    assert!(!allowed(&r, "[2001:db8:ffff::1]", Basic));
    assert!(allowed(&r, "2001:db9::1", NoKyc));
    // IPv4-mapped IPv6 and legacy IPv4 spellings land in the IPv4 rules.
    assert!(!allowed(&r, "::ffff:10.1.2.3", NoKyc));
    assert!(!allowed(&r, "[::ffff:192.0.2.7]:80", Basic));
    assert!(!allowed(&r, "10.1", NoKyc));
    assert!(!allowed(&r, "0xa.0.0.1", NoKyc));
    assert!(!allowed(&r, "167772161", NoKyc));
    assert!(!allowed(&r, "012.0.0.1", NoKyc));
    // IP rules never match names.
    assert!(allowed(&r, "ten.example.com", NoKyc));
}

#[test]
fn validate_accepts_good_rules() {
    let r = vec![
        rule("chase.com", "banking", Verified),
        rule(".chase.com", "banking", Verified),
        rule(".gov", "government", Verified),
        rule("xn--bcher-kva.example", "idn", Basic),
        rule("10.0.0.0/8", "net", Basic),
        rule("2001:db8::/32", "net", Basic),
        rule("192.0.2.1", "ip", Basic),
    ];
    assert_eq!(validate_rules(&r), Ok(()));
    assert_eq!(validate_rules(&[]), Ok(()));
}

#[test]
fn validate_rejects_bad_patterns() {
    let bad = [
        ("", "empty"),
        ("https://chase.com", "scheme"),
        ("chase.com/login", "path"),
        ("chase.com:443", "port"),
        ("*.chase.com", "wildcard"),
        ("Chase.com", "lowercase"),
        ("b\u{fc}cher.de", "punycode"),
        ("chase.com.", "trailing dot"),
        (".", "trailing dot"),
        ("chase..com", "label"),
        ("-chase.com", "'-'"),
        ("chase_bank.com", "a-z"),
        ("chase .com", "whitespace"),
        ("1.2.3", "numeric"),
        ("10.0.0.1/8", "host bits"),
        ("10.0.0.0/33", "prefix length"),
        ("10.0.0.0/", "prefix length"),
        ("::ffff:10.0.0.0/104", "IPv4 form"),
    ];
    for (pattern, why) in bad {
        let err = validate_rules(&[rule(pattern, "c", Basic)]).unwrap_err();
        assert!(err.contains(why), "{pattern:?}: {err}");
    }
    let long = format!("{}.com", "a".repeat(64));
    assert!(validate_rules(&[rule(&long, "c", Basic)]).is_err());
    assert!(validate_rules(&[rule("ok.com", " ", Basic)])
        .unwrap_err()
        .contains("category"));
}

#[test]
fn validate_rejects_duplicates() {
    let dup = vec![
        rule("chase.com", "banking", Verified),
        rule("chase.com", "other", Basic),
    ];
    let err = validate_rules(&dup).unwrap_err();
    assert!(err.contains("duplicate") && err.contains("[1]"), "{err}");
    // Same network spelled two ways is still a duplicate.
    let dup = vec![
        rule("2001:db8::/32", "a", Basic),
        rule("2001:0db8::/32", "b", Basic),
    ];
    assert!(validate_rules(&dup).unwrap_err().contains("duplicate"));
}

#[derive(Deserialize)]
struct BlocklistFile {
    blocked_domains: Vec<DomainRule>,
}

fn example_rules() -> Vec<DomainRule> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/blocklist.yaml");
    let yaml = std::fs::read_to_string(path).expect("read examples/blocklist.yaml");
    let file: BlocklistFile = serde_saphyr::from_str(&yaml).expect("parse blocklist");
    file.blocked_domains
}

#[test]
fn example_blocklist_parses_and_validates() {
    let rules = example_rules();
    assert!(rules.len() > 50, "only {} rules", rules.len());
    assert_eq!(validate_rules(&rules), Ok(()));
    let mut cats: Vec<&str> = rules.iter().map(|r| r.category.as_str()).collect();
    cats.sort_unstable();
    cats.dedup();
    assert_eq!(
        cats,
        [
            "banking",
            "crypto_exchange",
            "elections",
            "government",
            "healthcare",
            "payments"
        ]
    );
    for r in &rules {
        let want = if r.category == "crypto_exchange" {
            Basic
        } else {
            Verified
        };
        assert_eq!(r.min_kyc, want, "{}", r.pattern);
    }
}

#[test]
fn example_blocklist_table() {
    let rules = example_rules();
    // (host, tier, expected blocked category or "" for allowed)
    let cases: &[(&str, KycTier, &str)] = &[
        ("irs.gov", NoKyc, "government"),
        ("www.irs.gov", Basic, "government"),
        ("irs.gov", Verified, ""),
        ("army.mil", Basic, "government"),
        ("www.gov.uk", NoKyc, "government"),
        ("gov.uk", NoKyc, "government"),
        ("impots.gouv.fr", Basic, "government"),
        ("gouv.fr", NoKyc, ""),
        ("www.bund.de", NoKyc, "government"),
        ("ec.europa.eu", NoKyc, "government"),
        ("canada.ca", NoKyc, "government"),
        ("my.gov.au", NoKyc, "government"),
        ("www.chase.com", NoKyc, "banking"),
        ("secure.chase.com", Basic, "banking"),
        ("chase.com", Verified, ""),
        ("notchase.com", NoKyc, ""),
        ("online.bankofamerica.com", Basic, "banking"),
        ("connect.secure.wellsfargo.com", NoKyc, "banking"),
        ("www.barclays.co.uk", NoKyc, "banking"),
        ("app.monzo.com", Basic, "banking"),
        ("www.deutsche-bank.de", NoKyc, "banking"),
        ("www.paypal.com", NoKyc, "payments"),
        ("dashboard.stripe.com", Basic, "payments"),
        ("wise.com", NoKyc, "payments"),
        ("api.stripe.com", Verified, ""),
        ("www.coinbase.com", NoKyc, "crypto_exchange"),
        ("www.coinbase.com", Basic, ""),
        ("api.binance.com", NoKyc, "crypto_exchange"),
        ("api.kraken.com", Verified, ""),
        ("crypto.com", NoKyc, "crypto_exchange"),
        ("mychart.com", Basic, "healthcare"),
        ("www.nhs.uk", NoKyc, "healthcare"),
        ("healthy.kp.org", NoKyc, "healthcare"),
        ("vote.gov", NoKyc, "government"),
        ("register.vote", NoKyc, "elections"),
        ("www.elections.ca", Basic, "elections"),
        ("example.com", NoKyc, ""),
        ("github.com", NoKyc, ""),
        ("en.wikipedia.org", NoKyc, ""),
        ("gemini.google.com", NoKyc, ""),
        ("government.example.com", NoKyc, ""),
        ("WWW.CHASE.COM.", NoKyc, "banking"),
        ("8.8.8.8", NoKyc, ""),
    ];
    assert!(cases.len() >= 30);
    for &(host, tier, want) in cases {
        let got = match check(&rules, host, tier) {
            Ok(()) => String::new(),
            Err(b) => b.category,
        };
        assert_eq!(got, want, "{host} at {tier:?}");
    }
}
