//! Route policy: the schema a user writes (YAML or API), and how the risk
//! dial turns into concrete limits.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Exit IP class. Declared most premium first; never datacenter.
#[derive(
    Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum ExitType {
    Mobile,
    Isp,
    Residential,
}

impl ExitType {
    pub const ALL: [ExitType; 3] = [ExitType::Mobile, ExitType::Isp, ExitType::Residential];
}

/// How a provider sources its IPs, best first.
#[derive(
    Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum SourcingTier {
    Verified,
    Standard,
    Unverified,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RouteMode {
    /// Same exit IP for the life of a session (or forever).
    #[default]
    Sticky,
    /// Fresh exit per request or on block.
    Rotating,
}

/// What to do when a pinned exit IP stops working.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PinLoss {
    /// Refuse traffic and alert. Safest for messaging bridges.
    #[default]
    FailClosed,
    SameAsn,
    SameCity,
    SameCountry,
    Any,
}

/// How long a sticky route holds its exit.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum StickyTtl {
    /// Until the client goes idle for 30 minutes.
    #[default]
    Session,
    Hours(u32),
    Days(u32),
    /// Hard pin for the life of the route.
    Forever,
}

/// Minutes in 90 days: the longest pin any provider is asked to guarantee.
pub const FOREVER_MINUTES: u32 = 90 * 24 * 60;

impl StickyTtl {
    /// Minimum pin length a provider must support for this TTL.
    pub fn minutes(self) -> u32 {
        match self {
            StickyTtl::Session => 30,
            StickyTtl::Hours(h) => h.saturating_mul(60),
            StickyTtl::Days(d) => d.saturating_mul(24 * 60),
            StickyTtl::Forever => FOREVER_MINUTES,
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Sticky {
    #[serde(default)]
    pub ttl: StickyTtl,
    #[serde(default)]
    pub on_pin_loss: PinLoss,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Geo {
    /// ISO 3166-1 alpha-2, e.g. "DE".
    pub country: Option<String>,
    pub city: Option<String>,
    pub asn: Option<u32>,
    /// Follow the region of the account behind the client (e.g. a phone number).
    #[serde(default)]
    pub match_account: bool,
}

/// Per-dimension overrides of what the risk dial would choose.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct RiskOverrides {
    pub max_fraud_score: Option<u8>,
    pub allowed_exit_types: Option<Vec<ExitType>>,
    pub sourcing_tiers: Option<Vec<SourcingTier>>,
    pub min_sticky_minutes: Option<u32>,
}

/// Default risk: reliability first.
pub const DEFAULT_RISK: u8 = 20;
/// Default share of requests that must succeed on the target.
pub const DEFAULT_RELIABILITY_FLOOR: f64 = 0.95;

fn default_risk() -> u8 {
    DEFAULT_RISK
}
fn default_floor() -> f64 {
    DEFAULT_RELIABILITY_FLOOR
}
fn default_exits() -> Vec<ExitType> {
    vec![ExitType::Residential]
}

/// A saved route. Clients reference it by name in the proxy username or
/// connect to its dedicated endpoint.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RoutePolicy {
    /// Slug: lowercase letters, digits and dashes.
    pub name: String,
    #[serde(default)]
    pub mode: RouteMode,
    /// Required when `mode: sticky`.
    pub sticky: Option<Sticky>,
    /// Preferred exit classes. The risk dial may allow cheaper ones too.
    #[serde(default = "default_exits")]
    pub exit_types: Vec<ExitType>,
    #[serde(default)]
    pub geo: Geo,
    /// 0 = cleanest and most expensive, 100 = cheapest and noisiest.
    #[serde(default = "default_risk")]
    pub risk: u8,
    /// Default pick is the cheapest exit at or above this success rate.
    #[serde(default = "default_floor")]
    pub reliability_floor: f64,
    #[serde(default)]
    pub overrides: RiskOverrides,
    /// Optional hint for the cheapest-exit lookup.
    pub target_url: Option<String>,
    pub spend_cap_usd: Option<f64>,
    /// Called on pin loss, cap reached and health drops.
    pub alert_webhook: Option<String>,
}

/// The file format for `routes.yaml`.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RoutesFile {
    pub routes: Vec<RoutePolicy>,
}

#[derive(Debug, Error, PartialEq)]
pub enum PolicyError {
    #[error("route name {0:?} must be 1-48 chars of a-z, 0-9 and '-'")]
    BadName(String),
    #[error("route {0}: risk must be 0-100")]
    RiskOutOfRange(String),
    #[error("route {0}: reliability_floor must be between 0.5 and 1.0")]
    FloorOutOfRange(String),
    #[error("route {0}: mode is sticky but no `sticky` block was given")]
    StickyMissing(String),
    #[error("route {0}: `sticky` is set but mode is rotating")]
    StickyOnRotating(String),
    #[error("route {0}: exit_types must not be empty")]
    NoExitTypes(String),
    #[error("route {0}: country must be a 2-letter ISO code")]
    BadCountry(String),
    #[error("route {0}: spend_cap_usd must be positive")]
    BadSpendCap(String),
    #[error("duplicate route name {0}")]
    Duplicate(String),
    #[error("invalid YAML: {0}")]
    Yaml(String),
}

impl RoutePolicy {
    pub fn validate(&self) -> Result<(), PolicyError> {
        let n = &self.name;
        let ok_name = !n.is_empty()
            && n.len() <= 48
            && n.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !ok_name {
            return Err(PolicyError::BadName(n.clone()));
        }
        if self.risk > 100 {
            return Err(PolicyError::RiskOutOfRange(n.clone()));
        }
        if !(0.5..=1.0).contains(&self.reliability_floor) {
            return Err(PolicyError::FloorOutOfRange(n.clone()));
        }
        match (self.mode, &self.sticky) {
            (RouteMode::Sticky, None) => return Err(PolicyError::StickyMissing(n.clone())),
            (RouteMode::Rotating, Some(_)) => return Err(PolicyError::StickyOnRotating(n.clone())),
            _ => {}
        }
        if self.exit_types.is_empty() {
            return Err(PolicyError::NoExitTypes(n.clone()));
        }
        if let Some(cc) = &self.geo.country {
            if cc.len() != 2 || !cc.bytes().all(|b| b.is_ascii_alphabetic()) {
                return Err(PolicyError::BadCountry(n.clone()));
            }
        }
        if matches!(self.spend_cap_usd, Some(c) if c <= 0.0 || !c.is_finite()) {
            return Err(PolicyError::BadSpendCap(n.clone()));
        }
        Ok(())
    }

    /// Pin length this route needs from a provider (0 for rotating).
    pub fn required_sticky_minutes(&self) -> u32 {
        match (self.mode, &self.sticky) {
            (RouteMode::Sticky, Some(s)) => s.ttl.minutes(),
            _ => 0,
        }
    }

    /// Risk limits for this route, with the sticky requirement folded in.
    pub fn resolved_risk(&self) -> ResolvedRisk {
        let mut ov = self.overrides.clone();
        if ov.min_sticky_minutes.is_none() {
            ov.min_sticky_minutes = Some(self.required_sticky_minutes());
        }
        resolve_risk(self.risk, &self.exit_types, &ov)
    }
}

/// Parse and validate a `routes.yaml` document.
pub fn parse_routes_yaml(yaml: &str) -> Result<RoutesFile, PolicyError> {
    let file: RoutesFile =
        serde_saphyr::from_str(yaml).map_err(|e| PolicyError::Yaml(e.to_string()))?;
    let mut seen = std::collections::HashSet::new();
    for r in &file.routes {
        r.validate()?;
        if !seen.insert(r.name.as_str()) {
            return Err(PolicyError::Duplicate(r.name.clone()));
        }
    }
    Ok(file)
}

/// Concrete limits a risk dial value resolves to.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct ResolvedRisk {
    pub max_fraud_score: u8,
    pub allowed_exit_types: Vec<ExitType>,
    pub sourcing_tiers: Vec<SourcingTier>,
    pub min_sticky_minutes: u32,
}

/// Map the 0-100 dial onto the four risk dimensions. Low risk means clean
/// IPs, premium exits, vetted sourcing and long sticky guarantees; high risk
/// trades each for cost. Overrides win per field.
pub fn resolve_risk(risk: u8, preferred: &[ExitType], ov: &RiskOverrides) -> ResolvedRisk {
    let r = u32::from(risk.min(100));

    let max_fraud_score = (15 + (r * 6 + 5) / 10) as u8;

    // Downgrade chain: higher risk allows cheaper classes after the worst
    // preferred one. Mobile -> ISP -> residential; never datacenter.
    let steps = if r < 30 {
        0
    } else if r < 60 {
        1
    } else {
        2
    };
    let worst = preferred.iter().map(|e| *e as usize).max().unwrap_or(0);
    let mut allowed: Vec<ExitType> = ExitType::ALL
        .into_iter()
        .filter(|e| {
            preferred.contains(e) || ((*e as usize) > worst && (*e as usize) <= worst + steps)
        })
        .collect();
    allowed.sort();

    let sourcing_tiers = if r < 40 {
        vec![SourcingTier::Verified]
    } else if r < 75 {
        vec![SourcingTier::Verified, SourcingTier::Standard]
    } else {
        vec![
            SourcingTier::Verified,
            SourcingTier::Standard,
            SourcingTier::Unverified,
        ]
    };

    let min_sticky_minutes = if r < 30 {
        24 * 60
    } else if r < 70 {
        30
    } else {
        0
    };

    ResolvedRisk {
        max_fraud_score: ov.max_fraud_score.unwrap_or(max_fraud_score),
        allowed_exit_types: ov.allowed_exit_types.clone().unwrap_or(allowed),
        sourcing_tiers: ov.sourcing_tiers.clone().unwrap_or(sourcing_tiers),
        min_sticky_minutes: ov.min_sticky_minutes.unwrap_or(min_sticky_minutes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(name: &str) -> RoutePolicy {
        RoutePolicy {
            name: name.into(),
            mode: RouteMode::Sticky,
            sticky: Some(Sticky::default()),
            exit_types: vec![ExitType::Residential],
            geo: Geo::default(),
            risk: DEFAULT_RISK,
            reliability_floor: DEFAULT_RELIABILITY_FLOOR,
            overrides: RiskOverrides::default(),
            target_url: None,
            spend_cap_usd: None,
            alert_webhook: None,
        }
    }

    #[test]
    fn low_risk_is_strict() {
        let r = resolve_risk(0, &[ExitType::Mobile], &RiskOverrides::default());
        assert_eq!(r.max_fraud_score, 15);
        assert_eq!(r.allowed_exit_types, vec![ExitType::Mobile]);
        assert_eq!(r.sourcing_tiers, vec![SourcingTier::Verified]);
        assert_eq!(r.min_sticky_minutes, 24 * 60);
    }

    #[test]
    fn high_risk_downgrades_but_never_past_residential() {
        let r = resolve_risk(100, &[ExitType::Mobile], &RiskOverrides::default());
        assert_eq!(r.max_fraud_score, 75);
        assert_eq!(r.allowed_exit_types, ExitType::ALL.to_vec());
        assert_eq!(r.sourcing_tiers.len(), 3);
        assert_eq!(r.min_sticky_minutes, 0);
    }

    #[test]
    fn mid_risk_allows_one_step_down() {
        let r = resolve_risk(45, &[ExitType::Mobile], &RiskOverrides::default());
        assert_eq!(r.allowed_exit_types, vec![ExitType::Mobile, ExitType::Isp]);
    }

    #[test]
    fn overrides_win() {
        let ov = RiskOverrides {
            max_fraud_score: Some(5),
            min_sticky_minutes: Some(0),
            ..Default::default()
        };
        let r = resolve_risk(0, &[ExitType::Isp], &ov);
        assert_eq!(r.max_fraud_score, 5);
        assert_eq!(r.min_sticky_minutes, 0);
    }

    #[test]
    fn route_sticky_requirement_replaces_dial_guarantee() {
        let mut r = route("agent");
        r.sticky = Some(Sticky {
            ttl: StickyTtl::Session,
            on_pin_loss: PinLoss::SameCity,
        });
        assert_eq!(r.resolved_risk().min_sticky_minutes, 30);
        r.sticky = Some(Sticky {
            ttl: StickyTtl::Forever,
            ..Default::default()
        });
        assert_eq!(r.resolved_risk().min_sticky_minutes, FOREVER_MINUTES);
    }

    #[test]
    fn validation_catches_bad_routes() {
        assert_eq!(
            route("Bad Name").validate(),
            Err(PolicyError::BadName("Bad Name".into()))
        );
        let mut r = route("x");
        r.sticky = None;
        assert_eq!(r.validate(), Err(PolicyError::StickyMissing("x".into())));
        let mut r = route("x");
        r.mode = RouteMode::Rotating;
        assert_eq!(r.validate(), Err(PolicyError::StickyOnRotating("x".into())));
        let mut r = route("x");
        r.reliability_floor = 0.2;
        assert_eq!(r.validate(), Err(PolicyError::FloorOutOfRange("x".into())));
        let mut r = route("x");
        r.geo.country = Some("DEU".into());
        assert_eq!(r.validate(), Err(PolicyError::BadCountry("x".into())));
        assert_eq!(route("ok-route-1").validate(), Ok(()));
    }

    #[test]
    fn example_routes_file_parses() {
        let file = parse_routes_yaml(include_str!("../../../examples/routes.yaml"))
            .expect("example parses");
        let wa = file
            .routes
            .iter()
            .find(|r| r.name == "mautrix-whatsapp")
            .unwrap();
        assert_eq!(wa.sticky.as_ref().unwrap().ttl, StickyTtl::Forever);
        assert_eq!(wa.sticky.as_ref().unwrap().on_pin_loss, PinLoss::FailClosed);
        assert_eq!(wa.risk, 5);
        let scrape = file
            .routes
            .iter()
            .find(|r| r.name == "price-scrape")
            .unwrap();
        assert_eq!(scrape.mode, RouteMode::Rotating);
        assert_eq!(scrape.reliability_floor, DEFAULT_RELIABILITY_FLOOR);
    }

    #[test]
    fn yaml_rejects_unknown_fields_and_duplicates() {
        let bad = "routes:\n  - name: a\n    sticky: {}\n    colour: red\n";
        assert!(matches!(parse_routes_yaml(bad), Err(PolicyError::Yaml(_))));
        let dup = "routes:\n  - name: a\n    sticky: {}\n  - name: a\n    sticky: {}\n";
        assert_eq!(
            parse_routes_yaml(dup),
            Err(PolicyError::Duplicate("a".into()))
        );
    }
}
