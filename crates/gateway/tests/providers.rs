//! Named provider adapters: exact rendered upstreams and YAML parsing per vendor.

use humanip_gateway::config::{parse_yaml, ConfigError, UpstreamConfig};
use humanip_gateway::Upstream;

const HIP: &str = "hip0123456789abcdef";
/// Filters to 45 alnum chars, over every 32-char cap, so it is hashed.
const LONG: &str = "hip-Weird_ID!!0123456789abcdefXYZ0123456789abcdef";
/// FNV-1a of HIP and LONG: full hex, first 8 hex, and NetNut's 1..=99999999.
const HIP_HEX8: &str = "a4dbbc2e";
const HIP_NUM: &str = "62420267";
const LONG_HEX: &str = "440fcb2be3d15c83";
const LONG_HEX8: &str = "440fcb2b";
const LONG_NUM: &str = "39560060";

/// Parse one provider whose `upstream:` flow mapping is `up`.
fn parse(up: &str) -> Result<UpstreamConfig, ConfigError> {
    let yaml = format!(
        r#"
api_keys: [{{ name: a, key: k }}]
providers:
  - name: p
    exit_type: residential
    sourcing: verified
    price_per_gb: 3.0
    fraud_score: 10
    max_sticky_minutes: 60
    latency_ms: 300
    upstream: {{ {up} }}
"#
    );
    parse_yaml(&yaml).map(|c| c.providers[0].upstream.clone())
}

fn up(yaml: &str) -> UpstreamConfig {
    parse(yaml).unwrap_or_else(|e| panic!("{yaml}: {e}"))
}

fn http(addr: &str, username: &str, password: &str) -> Upstream {
    Upstream::Http {
        addr: addr.into(),
        username: username.into(),
        password: password.into(),
    }
}

fn socks(addr: &str, username: &str, password: &str) -> Upstream {
    Upstream::Socks5 {
        addr: addr.into(),
        username: username.into(),
        password: password.into(),
    }
}

/// One vendor's expected renders, as (username, password) pairs.
struct Case {
    yaml: &'static str,
    /// Expected (username, password) for: sticky+DE, sticky none, rotating+US, sticky LONG.
    sticky_de: (&'static str, &'static str),
    sticky_any: (&'static str, &'static str),
    rotating_us: (&'static str, &'static str),
    long: (String, String),
    http_addr: &'static str,
    socks_addr: &'static str,
    /// Fields that must be present; each is removed in turn.
    required: &'static [&'static str],
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            yaml: "kind: brightdata, customer: hl_abc123, zone: residential1, password: xyz",
            sticky_de: (
                "brd-customer-hl_abc123-zone-residential1-country-de-session-hip0123456789abcdef",
                "xyz",
            ),
            sticky_any: (
                "brd-customer-hl_abc123-zone-residential1-session-hip0123456789abcdef",
                "xyz",
            ),
            rotating_us: ("brd-customer-hl_abc123-zone-residential1-country-us", "xyz"),
            long: (
                format!("brd-customer-hl_abc123-zone-residential1-session-{LONG_HEX}"),
                "xyz".into(),
            ),
            http_addr: "brd.superproxy.io:33335",
            socks_addr: "brd.superproxy.io:22228",
            required: &["customer", "zone", "password"],
        },
        Case {
            yaml: "kind: oxylabs, username: myuser, password: xyz",
            sticky_de: ("customer-myuser-cc-DE-sessid-hip0123456789abcdef", "xyz"),
            sticky_any: ("customer-myuser-sessid-hip0123456789abcdef", "xyz"),
            rotating_us: ("customer-myuser-cc-US", "xyz"),
            long: (format!("customer-myuser-sessid-{LONG_HEX}"), "xyz".into()),
            http_addr: "pr.oxylabs.io:7777",
            socks_addr: "pr.oxylabs.io:7777",
            required: &["username", "password"],
        },
        Case {
            yaml: "kind: decodo, username: sp123, password: xyz",
            sticky_de: ("user-sp123-country-de-session-hip0123456789abcdef", "xyz"),
            sticky_any: ("user-sp123-session-hip0123456789abcdef", "xyz"),
            rotating_us: ("user-sp123-country-us", "xyz"),
            long: (format!("user-sp123-session-{LONG_HEX}"), "xyz".into()),
            http_addr: "gate.decodo.com:7000",
            socks_addr: "gate.decodo.com:7000",
            required: &["username", "password"],
        },
        Case {
            yaml: "kind: iproyal, username: iproy, password: xyz",
            sticky_de: ("iproy", "xyz_country-de_session-a4dbbc2e_lifetime-24h"),
            sticky_any: ("iproy", "xyz_session-a4dbbc2e_lifetime-24h"),
            rotating_us: ("iproy", "xyz_country-us"),
            long: (
                "iproy".into(),
                format!("xyz_session-{LONG_HEX8}_lifetime-24h"),
            ),
            http_addr: "geo.iproyal.com:12321",
            socks_addr: "geo.iproyal.com:32325",
            required: &["username", "password"],
        },
        Case {
            yaml: "kind: netnut, username: nnuser, password: xyz",
            sticky_de: ("nnuser-res-de-sid-62420267", "xyz"),
            sticky_any: ("nnuser-res-any-sid-62420267", "xyz"),
            rotating_us: ("nnuser-res-us", "xyz"),
            long: (format!("nnuser-res-any-sid-{LONG_NUM}"), "xyz".into()),
            http_addr: "gw.netnut.net:5959",
            socks_addr: "gw.netnut.net:5959",
            required: &["username", "password"],
        },
        Case {
            yaml: "kind: rayobyte, username: rayo, password: xyz",
            sticky_de: ("rayo", "xyz-country-DE-session-hip0123456789abcdef"),
            sticky_any: ("rayo", "xyz-session-hip0123456789abcdef"),
            rotating_us: ("rayo", "xyz-country-US"),
            long: ("rayo".into(), format!("xyz-session-{LONG_HEX}")),
            http_addr: "la.residential.rayobyte.com:8000",
            socks_addr: "la.residential.rayobyte.com:1080",
            required: &["username", "password"],
        },
        Case {
            yaml: "kind: proxy_cheap, username: pcuser, password: xyz",
            sticky_de: ("pcuser-country-de-session-hip0123456789abcdef", "xyz"),
            sticky_any: ("pcuser-session-hip0123456789abcdef", "xyz"),
            rotating_us: ("pcuser-country-us", "xyz"),
            long: (format!("pcuser-session-{LONG_HEX}"), "xyz".into()),
            http_addr: "proxy.proxy-cheap.com:31112",
            socks_addr: "proxy.proxy-cheap.com:31112",
            required: &["username", "password"],
        },
    ]
}

#[test]
fn hash_constants_are_consistent() {
    assert!(LONG_HEX.starts_with(LONG_HEX8));
    assert_eq!(HIP_HEX8.len(), 8);
    assert!(HIP_NUM.parse::<u64>().is_ok());
}

#[test]
fn sticky_with_country() {
    for c in cases() {
        let (u, p) = c.sticky_de;
        assert_eq!(
            up(c.yaml).render(HIP, true, Some("DE")),
            http(c.http_addr, u, p),
            "{}",
            c.yaml
        );
    }
}

#[test]
fn sticky_without_country() {
    for c in cases() {
        let (u, p) = c.sticky_any;
        assert_eq!(
            up(c.yaml).render(HIP, true, None),
            http(c.http_addr, u, p),
            "{}",
            c.yaml
        );
    }
}

#[test]
fn rotating_omits_session() {
    for c in cases() {
        let (u, p) = c.rotating_us;
        let got = up(c.yaml).render(HIP, false, Some("US"));
        assert_eq!(got, http(c.http_addr, u, p), "{}", c.yaml);
        let Upstream::Http {
            username, password, ..
        } = got
        else {
            unreachable!()
        };
        for s in [username, password] {
            assert!(
                !s.contains("sess") && !s.contains("-sid-") && !s.contains(HIP),
                "{s}"
            );
        }
    }
}

#[test]
fn session_is_sanitised_and_stable() {
    for c in cases() {
        let cfg = up(c.yaml);
        let (u, p) = &c.long;
        assert_eq!(
            cfg.render(LONG, true, None),
            http(c.http_addr, u, p),
            "{}",
            c.yaml
        );
        assert_eq!(cfg.render(LONG, true, None), cfg.render(LONG, true, None));
        assert_ne!(cfg.render(LONG, true, None), cfg.render(HIP, true, None));
    }
    // Short ids with stray characters keep their letters and digits.
    let bd = up(cases()[0].yaml);
    let expect = http(
        "brd.superproxy.io:33335",
        "brd-customer-hl_abc123-zone-residential1-session-BadId",
        "xyz",
    );
    assert_eq!(bd.render("Bad Id!", true, None), expect);
    let ipr = up(cases()[3].yaml).render("Bad Id!", true, None);
    assert_eq!(
        ipr,
        http(
            "geo.iproyal.com:12321",
            "iproy",
            "xyz_session-2b000117_lifetime-24h"
        )
    );
    let nn = up(cases()[4].yaml).render("Bad Id!", true, None);
    assert_eq!(
        nn,
        http("gw.netnut.net:5959", "nnuser-res-any-sid-27224895", "xyz")
    );
}

#[test]
fn protocol_override_uses_socks_port() {
    for c in cases() {
        let cfg = up(&format!("{}, protocol: socks5", c.yaml));
        let (u, p) = c.sticky_de;
        assert_eq!(
            cfg.render(HIP, true, Some("DE")),
            socks(c.socks_addr, u, p),
            "{}",
            c.yaml
        );
        let cfg = up(&format!("{}, protocol: http", c.yaml));
        assert_eq!(cfg.render(HIP, true, Some("DE")), http(c.http_addr, u, p));
    }
}

#[test]
fn endpoint_override_replaces_addr() {
    for c in cases() {
        let (u, p) = c.sticky_any;
        let cfg = up(&format!("{}, endpoint: \"eu.gw.example:9000\"", c.yaml));
        assert_eq!(
            cfg.render(HIP, true, None),
            http("eu.gw.example:9000", u, p)
        );
        let both = format!(
            "{}, protocol: socks5, endpoint: \"eu.gw.example:9001\"",
            c.yaml
        );
        assert_eq!(
            up(&both).render(HIP, true, None),
            socks("eu.gw.example:9001", u, p)
        );
    }
}

#[test]
fn prefixed_usernames_are_not_doubled() {
    let ox = up("kind: oxylabs, username: customer-myuser, password: xyz");
    assert_eq!(
        ox.render(HIP, false, None),
        http("pr.oxylabs.io:7777", "customer-myuser", "xyz")
    );
    let dc = up("kind: decodo, username: user-sp123, password: xyz");
    assert_eq!(
        dc.render(HIP, false, None),
        http("gate.decodo.com:7000", "user-sp123", "xyz")
    );
}

#[test]
fn missing_and_empty_fields_are_rejected() {
    for c in cases() {
        for field in c.required {
            let without: Vec<&str> = c
                .yaml
                .split(", ")
                .filter(|kv| !kv.starts_with(&format!("{field}:")))
                .collect();
            let res = parse(&without.join(", "));
            assert!(
                matches!(&res, Err(ConfigError::Yaml(m)) if m.contains(field)),
                "missing {field} in {}: {res:?}",
                c.yaml
            );
            let empty: Vec<String> = c
                .yaml
                .split(", ")
                .map(|kv| {
                    if kv.starts_with(&format!("{field}:")) {
                        format!("{field}: \"\"")
                    } else {
                        kv.to_string()
                    }
                })
                .collect();
            let res = parse(&empty.join(", "));
            assert!(
                matches!(&res, Err(ConfigError::Invalid(m)) if m.contains(&format!("provider p: {field} must not be empty"))),
                "empty {field} in {}: {res:?}",
                c.yaml
            );
        }
    }
}

#[test]
fn bad_protocol_endpoint_and_unknown_fields_are_rejected() {
    for c in cases() {
        let bad = format!("{}, protocol: quic", c.yaml);
        assert!(matches!(parse(&bad), Err(ConfigError::Yaml(_))), "{bad}");
        let extra = format!("{}, colour: red", c.yaml);
        assert!(
            matches!(parse(&extra), Err(ConfigError::Yaml(_))),
            "{extra}"
        );
        let ep = format!("{}, endpoint: nohostport", c.yaml);
        assert!(
            matches!(parse(&ep), Err(ConfigError::Invalid(m)) if m.contains("host:port")),
            "{ep}"
        );
    }
    assert!(matches!(
        parse("kind: luminati, username: u"),
        Err(ConfigError::Yaml(_))
    ));
}

#[test]
fn debug_hides_password() {
    for c in cases() {
        let yaml = c.yaml.replace("password: xyz", "password: s3cr3t-pw");
        let cfg = parse(&format!("{yaml}, endpoint: \"h.example:1\"")).unwrap();
        let d = format!("{cfg:?}");
        assert!(!d.contains("s3cr3t-pw"), "{d}");
        assert!(d.contains("<redacted>") && d.contains("h.example:1"), "{d}");
    }
}

#[test]
fn generic_kinds_unchanged() {
    let s = up(
        "kind: socks5, addr: \"10.0.0.1:1080\", username: \"u-{session}-{country}\", password: pw",
    );
    assert_eq!(
        s.render("hip1", true, Some("DE")),
        socks("10.0.0.1:1080", "u-hip1-de", "pw")
    );
    assert_eq!(
        s.render("hip2", false, None),
        socks("10.0.0.1:1080", "u-hip2-any", "pw")
    );
    let h = up("kind: http, addr: \"10.0.0.3:8080\", username: \"{session}\", password: pw");
    assert_eq!(
        h.render("hip1", false, None),
        http("10.0.0.3:8080", "hip1", "pw")
    );
}
