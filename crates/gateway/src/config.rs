//! Gateway configuration: listeners, API keys, providers and routes.
//!
//! Parsed from YAML with [`parse_yaml`], which also validates it.

use crate::providers::{self, AdapterRequest, Protocol};
use crate::Upstream;
use humanip_core::policy::{ExitType, RoutePolicy, SourcingTier};
use serde::Deserialize;
use std::collections::HashSet;
use std::fmt;

fn default_socks_listen() -> String {
    "127.0.0.1:1080".into()
}
fn default_http_listen() -> String {
    "127.0.0.1:8080".into()
}
fn default_connect_timeout_ms() -> u64 {
    10_000
}

/// The whole gateway config file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayConfig {
    #[serde(default = "default_socks_listen")]
    pub socks_listen: String,
    #[serde(default = "default_http_listen")]
    pub http_listen: String,
    /// Allow `kind: direct` upstreams (datacenter exit). Dev and tests only.
    #[serde(default)]
    pub allow_direct: bool,
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
    pub api_keys: Vec<ApiKey>,
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub routes: Vec<RoutePolicy>,
}

impl GatewayConfig {
    /// Provider by name.
    pub fn provider(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.name == name)
    }

    /// Route by name.
    pub fn route(&self, name: &str) -> Option<&RoutePolicy> {
        self.routes.iter().find(|r| r.name == name)
    }
}

/// A client credential. Clients send `key` as the proxy password.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiKey {
    pub name: String,
    pub key: String,
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiKey")
            .field("name", &self.name)
            .field("key", &"<redacted>")
            .finish()
    }
}

/// One upstream proxy provider and the facts selection needs about it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub name: String,
    pub exit_type: ExitType,
    pub sourcing: SourcingTier,
    /// USD per GB, before the humanip fee.
    pub price_per_gb: f64,
    /// Median pool fraud score, 0-100.
    pub fraud_score: u8,
    pub max_sticky_minutes: u32,
    pub latency_ms: u32,
    /// ISO alpha-2 codes this provider can exit in. Empty means any.
    #[serde(default)]
    pub countries: Vec<String>,
    pub upstream: UpstreamConfig,
}

impl ProviderConfig {
    /// True when the provider can exit in `country` (case-insensitive).
    pub fn serves_country(&self, country: &str) -> bool {
        self.countries.is_empty()
            || self
                .countries
                .iter()
                .any(|c| c.eq_ignore_ascii_case(country))
    }
}

/// How to reach a provider. `socks5` and `http` take a username template with
/// `{session}` and `{country}`; the named kinds take only vendor credentials
/// and build the username in [`providers`].
#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum UpstreamConfig {
    Direct,
    Socks5 {
        addr: String,
        username: String,
        password: String,
    },
    Http {
        addr: String,
        username: String,
        password: String,
    },
    #[serde(rename = "brightdata")]
    BrightData(providers::BrightData),
    #[serde(rename = "oxylabs")]
    Oxylabs(providers::Oxylabs),
    #[serde(rename = "decodo")]
    Decodo(providers::Decodo),
    #[serde(rename = "iproyal")]
    IpRoyal(providers::IpRoyal),
    #[serde(rename = "netnut")]
    NetNut(providers::NetNut),
    #[serde(rename = "rayobyte")]
    Rayobyte(providers::Rayobyte),
    #[serde(rename = "proxy_cheap")]
    ProxyCheap(providers::ProxyCheap),
}

impl fmt::Debug for UpstreamConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpstreamConfig::Direct => f.write_str("Direct"),
            UpstreamConfig::Socks5 { addr, username, .. } => f
                .debug_struct("Socks5")
                .field("addr", addr)
                .field("username", username)
                .field("password", &"<redacted>")
                .finish(),
            UpstreamConfig::Http { addr, username, .. } => f
                .debug_struct("Http")
                .field("addr", addr)
                .field("username", username)
                .field("password", &"<redacted>")
                .finish(),
            UpstreamConfig::BrightData(c) => c.fmt(f),
            UpstreamConfig::Oxylabs(c) => c.fmt(f),
            UpstreamConfig::Decodo(c) => c.fmt(f),
            UpstreamConfig::IpRoyal(c) => c.fmt(f),
            UpstreamConfig::NetNut(c) => c.fmt(f),
            UpstreamConfig::Rayobyte(c) => c.fmt(f),
            UpstreamConfig::ProxyCheap(c) => c.fmt(f),
        }
    }
}

impl UpstreamConfig {
    /// Concrete upstream for one connection. `session` is the selector's
    /// `hip...` id, `sticky` false for rotating routes (named kinds then omit
    /// the session parameter), `country` uppercase ISO alpha-2 or None.
    /// `socks5`/`http` fill `{session}` and `{country}` (lowercase, "any").
    pub fn render(&self, session: &str, sticky: bool, country: Option<&str>) -> Upstream {
        let req = AdapterRequest {
            session,
            sticky,
            country,
        };
        match self {
            UpstreamConfig::Direct => Upstream::Direct,
            UpstreamConfig::Socks5 {
                addr,
                username,
                password,
            } => providers::generic_template(Protocol::Socks5, addr, username, password, &req),
            UpstreamConfig::Http {
                addr,
                username,
                password,
            } => providers::generic_template(Protocol::Http, addr, username, password, &req),
            UpstreamConfig::BrightData(c) => providers::brightdata(c, &req),
            UpstreamConfig::Oxylabs(c) => providers::oxylabs(c, &req),
            UpstreamConfig::Decodo(c) => providers::decodo(c, &req),
            UpstreamConfig::IpRoyal(c) => providers::iproyal(c, &req),
            UpstreamConfig::NetNut(c) => providers::netnut(c, &req),
            UpstreamConfig::Rayobyte(c) => providers::rayobyte(c, &req),
            UpstreamConfig::ProxyCheap(c) => providers::proxy_cheap(c, &req),
        }
    }

    /// Required named-adapter fields are non-empty and `endpoint` is host:port.
    /// `direct`, `socks5` and `http` are not checked here.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            UpstreamConfig::Direct
            | UpstreamConfig::Socks5 { .. }
            | UpstreamConfig::Http { .. } => Ok(()),
            UpstreamConfig::BrightData(c) => c.validate(),
            UpstreamConfig::Oxylabs(c) => c.validate(),
            UpstreamConfig::Decodo(c) => c.validate(),
            UpstreamConfig::IpRoyal(c) => c.validate(),
            UpstreamConfig::NetNut(c) => c.validate(),
            UpstreamConfig::Rayobyte(c) => c.validate(),
            UpstreamConfig::ProxyCheap(c) => c.validate(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid YAML: {0}")]
    Yaml(String),
    #[error("invalid config: {0}")]
    Invalid(String),
}

/// Parse and validate a gateway config document.
pub fn parse_yaml(yaml: &str) -> Result<GatewayConfig, ConfigError> {
    let cfg: GatewayConfig =
        serde_saphyr::from_str(yaml).map_err(|e| ConfigError::Yaml(e.to_string()))?;
    validate(&cfg)?;
    Ok(cfg)
}

fn invalid(msg: impl Into<String>) -> ConfigError {
    ConfigError::Invalid(msg.into())
}

fn is_country(c: &str) -> bool {
    c.len() == 2 && c.bytes().all(|b| b.is_ascii_alphabetic())
}

fn validate(cfg: &GatewayConfig) -> Result<(), ConfigError> {
    if cfg.connect_timeout_ms == 0 {
        return Err(invalid("connect_timeout_ms must be positive"));
    }
    if cfg.api_keys.is_empty() {
        return Err(invalid("at least one api key is required"));
    }
    let (mut names, mut keys) = (HashSet::new(), HashSet::new());
    for k in &cfg.api_keys {
        if k.name.is_empty() || k.key.is_empty() {
            return Err(invalid("api key name and key must not be empty"));
        }
        if !names.insert(k.name.as_str()) {
            return Err(invalid(format!("duplicate api key name {:?}", k.name)));
        }
        if !keys.insert(k.key.as_str()) {
            return Err(invalid(format!(
                "api key {:?} reuses another key's secret",
                k.name
            )));
        }
    }

    if cfg.providers.is_empty() {
        return Err(invalid("at least one provider is required"));
    }
    let mut providers = HashSet::new();
    for p in &cfg.providers {
        let n = &p.name;
        if n.is_empty() {
            return Err(invalid("provider name must not be empty"));
        }
        if !providers.insert(n.as_str()) {
            return Err(invalid(format!("duplicate provider name {n:?}")));
        }
        if !(p.price_per_gb.is_finite() && p.price_per_gb > 0.0) {
            return Err(invalid(format!("provider {n}: price_per_gb must be > 0")));
        }
        if p.fraud_score > 100 {
            return Err(invalid(format!("provider {n}: fraud_score must be 0-100")));
        }
        if let Some(c) = p.countries.iter().find(|c| !is_country(c)) {
            return Err(invalid(format!(
                "provider {n}: country {c:?} must be 2 ASCII letters"
            )));
        }
        p.upstream
            .validate()
            .map_err(|e| invalid(format!("provider {n}: {e}")))?;
        if matches!(p.upstream, UpstreamConfig::Direct) && !cfg.allow_direct {
            return Err(invalid(format!(
                "provider {n}: direct upstream needs allow_direct: true"
            )));
        }
    }

    let mut routes = HashSet::new();
    for r in &cfg.routes {
        r.validate().map_err(|e| invalid(e.to_string()))?;
        if !routes.insert(r.name.as_str()) {
            return Err(invalid(format!("duplicate route name {}", r.name)));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
api_keys:
  - { name: alice, key: k-alice }
providers:
  - name: resi
    exit_type: residential
    sourcing: verified
    price_per_gb: 3.0
    fraud_score: 10
    max_sticky_minutes: 1440
    latency_ms: 300
    countries: [US, de]
    upstream:
      kind: socks5
      addr: "10.0.0.1:1080"
      username: "u-session-{session}-cc-{country}"
      password: pw
routes:
  - name: agent
    sticky: { ttl: session }
"#;

    fn err(yaml: &str) -> String {
        match parse_yaml(yaml) {
            Err(ConfigError::Invalid(m)) => m,
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn good_yaml_parses_with_defaults() {
        let c = parse_yaml(GOOD).unwrap();
        assert_eq!(c.socks_listen, "127.0.0.1:1080");
        assert_eq!(c.http_listen, "127.0.0.1:8080");
        assert_eq!(c.connect_timeout_ms, 10_000);
        assert!(!c.allow_direct);
        assert_eq!(c.providers[0].exit_type, ExitType::Residential);
        assert!(c.providers[0].serves_country("us"));
        assert!(!c.providers[0].serves_country("FR"));
        assert_eq!(c.routes[0].name, "agent");
        assert!(!format!("{c:?}").contains("k-alice"));
    }

    #[test]
    fn render_substitutes_placeholders() {
        let c = parse_yaml(GOOD).unwrap();
        let up = &c.providers[0].upstream;
        assert_eq!(
            up.render("hip1", true, Some("DE")),
            Upstream::Socks5 {
                addr: "10.0.0.1:1080".into(),
                username: "u-session-hip1-cc-de".into(),
                password: "pw".into(),
            }
        );
        match up.render("s", false, None) {
            Upstream::Socks5 { username, .. } => assert_eq!(username, "u-session-s-cc-any"),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            UpstreamConfig::Direct.render("s", true, None),
            Upstream::Direct
        );
    }

    #[test]
    fn yaml_errors_and_unknown_fields() {
        assert!(matches!(parse_yaml("::"), Err(ConfigError::Yaml(_))));
        let extra = format!("{GOOD}colour: red\n");
        assert!(matches!(parse_yaml(&extra), Err(ConfigError::Yaml(_))));
    }

    #[test]
    fn each_validation_rule_rejects() {
        let no_keys = GOOD.replace("  - { name: alice, key: k-alice }", "  []");
        let no_keys = no_keys.replace("api_keys:\n  []", "api_keys: []");
        assert!(err(&no_keys).contains("api key"));

        let empty_name = GOOD.replace("name: alice,", "name: \"\",");
        assert!(err(&empty_name).contains("empty"));

        let dup_name = GOOD.replace(
            "  - { name: alice, key: k-alice }",
            "  - { name: alice, key: k-alice }\n  - { name: alice, key: k-two }",
        );
        assert!(err(&dup_name).contains("duplicate api key name"));

        let dup_key = GOOD.replace(
            "  - { name: alice, key: k-alice }",
            "  - { name: alice, key: k-alice }\n  - { name: bob, key: k-alice }",
        );
        assert!(err(&dup_key).contains("reuses"));

        let no_providers = r#"
api_keys: [{ name: a, key: k }]
providers: []
"#;
        assert!(err(no_providers).contains("provider"));

        assert!(err(&GOOD.replace("price_per_gb: 3.0", "price_per_gb: 0")).contains("price"));
        assert!(err(&GOOD.replace("fraud_score: 10", "fraud_score: 101")).contains("fraud"));
        assert!(err(&GOOD.replace("[US, de]", "[USA]")).contains("country"));
        assert!(err(&GOOD.replace("[US, de]", "[\"u1\"]")).contains("country"));

        let direct = GOOD.replace(
            "      kind: socks5\n      addr: \"10.0.0.1:1080\"\n      username: \"u-session-{session}-cc-{country}\"\n      password: pw",
            "      kind: direct",
        );
        assert!(err(&direct).contains("allow_direct"));
        let allowed = format!("allow_direct: true\n{direct}");
        assert!(parse_yaml(&allowed).is_ok());

        let bad_route = GOOD.replace("name: agent", "name: Bad Route");
        assert!(err(&bad_route).contains("route name"));

        let dup_route = format!("{GOOD}  - name: agent\n    sticky: {{}}\n");
        assert!(err(&dup_route).contains("duplicate route"));
    }

    #[test]
    fn duplicate_provider_rejected() {
        let p = GOOD
            .split("providers:\n")
            .nth(1)
            .unwrap()
            .split("routes:")
            .next()
            .unwrap();
        let yaml = format!("api_keys: [{{ name: a, key: k }}]\nproviders:\n{p}{p}");
        assert!(err(&yaml).contains("duplicate provider"));
    }
}
