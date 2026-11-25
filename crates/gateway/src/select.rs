//! Authentication and exit selection.
//!
//! ```text
//! username --parse_user--> ProxyUser --+ KeyRecord (account store) --> AuthedUser
//! AuthedUser + target --choose--> effective policy --> pin lookup
//!                                   |-- live pin: reuse provider and upstream session
//!                                   '-- no pin / lost pin: candidates -> eligible -> default_pick
//! ```

use crate::config::{GatewayConfig, ProviderConfig};
use crate::session::{Pin, PinKey, SessionTable};
use crate::{SuccessSource, TargetAddr, Upstream, NEUTRAL_SUCCESS};
use humanip_core::accounts::KeyRecord;
use humanip_core::domain_of;
use humanip_core::pareto::{check_eligible, default_pick, Offer};
use humanip_core::policy::{
    ExitType, Geo, PinLoss, RiskOverrides, RouteMode, RoutePolicy, Sticky, StickyTtl,
    DEFAULT_RELIABILITY_FLOOR, DEFAULT_RISK,
};
use humanip_core::username::{self, ProxyUser};
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A client that presented a valid API key.
#[derive(Clone, Debug)]
pub struct AuthedUser {
    /// The account store's record for the presented key.
    pub key: KeyRecord,
    /// Always `key.key_id`. Sticky pins and rotating session ids use it.
    pub key_name: String,
    pub user: ProxyUser,
}

impl AuthedUser {
    pub fn new(key: KeyRecord, user: ProxyUser) -> Self {
        Self {
            key_name: key.key_id.clone(),
            key,
            user,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("bad proxy username: {0}")]
    BadUsername(String),
    #[error("invalid API key")]
    BadKey,
    #[error("account store: {0}")]
    Store(String),
}

/// Per-connection settings from the proxy username. A bare `user` (or
/// `user-`) means none. The password (API key) is resolved by the account
/// store, see [`crate::accounts_bridge::authenticate`].
pub fn parse_user(username: &str) -> Result<ProxyUser, AuthError> {
    match username {
        "user" | "user-" => Ok(ProxyUser::default()),
        u => username::parse(u).map_err(|e| AuthError::BadUsername(e.to_string())),
    }
}

/// The domain a connection is selected, billed and abuse-checked under: the
/// username `target` if set, else `domain_of(host)`, else the IP string.
pub fn target_domain(user: &ProxyUser, target: &TargetAddr) -> String {
    match (&user.target, target) {
        (Some(t), _) => t.clone(),
        (None, TargetAddr::Ip(a)) => a.ip().to_string(),
        (None, TargetAddr::Domain(h, _)) => domain_of(h),
    }
}

/// The exit picked for one connection.
#[derive(Clone, Debug)]
pub struct Choice {
    pub provider: String,
    pub upstream: Upstream,
    /// Set when the connection is sticky.
    pub pin: Option<PinKey>,
    /// True when an existing live pin was reused.
    pub reused_pin: bool,
    /// The route's pin-loss rule. `Any` for unpinned choices.
    pub on_pin_loss: PinLoss,
    pub domain: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SelectError {
    #[error("unknown route {0:?}")]
    UnknownRoute(String),
    #[error("no eligible exit for {domain}")]
    NoEligibleExit { domain: String },
    #[error("pinned exit on {provider} was lost and the route fails closed")]
    PinLost { provider: String },
}

/// What the caller should do after a failed dial.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialFailure {
    /// Refuse the connection.
    FailClosed,
    /// Call [`Selector::choose`] again.
    Retry,
}

/// Implicit session for sticky named routes when the client sends none.
const ROUTE_SESSION: &str = "_route";

/// Idle TTL for `StickyTtl::Session`.
const SESSION_IDLE: Duration = Duration::from_secs(30 * 60);

/// Picks exits for authenticated clients and keeps their sticky pins.
pub struct Selector {
    cfg: Arc<GatewayConfig>,
    sessions: SessionTable,
    success: Arc<dyn SuccessSource>,
    counter: AtomicU64,
    seed: u64,
}

/// FNV-1a over parts separated by a 0 byte. Stable across runs and builds.
fn fnv(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for &b in part.iter().chain(std::iter::once(&0u8)) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

fn idle_ttl(ttl: StickyTtl) -> Option<Duration> {
    let hour = Duration::from_secs(3600);
    match ttl {
        StickyTtl::Session => Some(SESSION_IDLE),
        StickyTtl::Hours(h) => Some(hour * h),
        StickyTtl::Days(d) => Some(hour * 24 * d),
        StickyTtl::Forever => None,
    }
}

/// Policy for clients that name no route.
fn adhoc_policy() -> RoutePolicy {
    RoutePolicy {
        name: "adhoc".into(),
        mode: RouteMode::Rotating,
        sticky: None,
        exit_types: ExitType::ALL.to_vec(),
        geo: Geo::default(),
        risk: DEFAULT_RISK,
        reliability_floor: DEFAULT_RELIABILITY_FLOOR,
        overrides: RiskOverrides::default(),
        target_url: None,
        spend_cap_usd: None,
        alert_webhook: None,
    }
}

impl Selector {
    pub fn new(cfg: Arc<GatewayConfig>, success: Arc<dyn SuccessSource>) -> Self {
        Self {
            cfg,
            sessions: SessionTable::new(),
            success,
            counter: AtomicU64::new(0),
            seed: RandomState::new().hash_one(0u8),
        }
    }

    pub fn config(&self) -> &GatewayConfig {
        &self.cfg
    }

    pub fn sessions(&self) -> &SessionTable {
        &self.sessions
    }

    /// Route policy with username overrides applied. A client session on a
    /// rotating policy makes it sticky with the `session` TTL.
    fn effective_policy(&self, u: &ProxyUser) -> Result<RoutePolicy, SelectError> {
        let mut p = match &u.route {
            Some(name) => self
                .cfg
                .route(name)
                .cloned()
                .ok_or_else(|| SelectError::UnknownRoute(name.clone()))?,
            None => adhoc_policy(),
        };
        if let Some(r) = u.risk {
            p.risk = r;
        }
        if let Some(e) = u.exit {
            // An explicit exit class is a hard requirement: no risk downgrade.
            p.exit_types = vec![e];
            p.overrides.allowed_exit_types = Some(vec![e]);
        }
        if let Some(c) = &u.country {
            p.geo.country = Some(c.to_ascii_uppercase());
        }
        if u.session.is_some() && p.mode == RouteMode::Rotating {
            // A client-chosen session (e.g. a browser agent run) fails over to
            // any eligible exit. Fail-closed is for named routes that ask for it.
            p.mode = RouteMode::Sticky;
            p.sticky = Some(Sticky {
                ttl: StickyTtl::Session,
                on_pin_loss: PinLoss::Any,
            });
        }
        Ok(p)
    }

    /// Cheapest eligible provider at or above the floor (else most reliable).
    /// `near` restricts to providers sharing a country with that list.
    fn pick(
        &self,
        policy: &RoutePolicy,
        domain: &str,
        near: Option<&[String]>,
    ) -> Option<&ProviderConfig> {
        let risk = policy.resolved_risk();
        let need = policy.required_sticky_minutes();
        let country = policy.geo.country.as_deref();
        let candidates: Vec<&ProviderConfig> = self
            .cfg
            .providers
            .iter()
            .filter(|p| country.is_none_or(|c| p.serves_country(c)))
            .filter(|p| {
                near.is_none_or(|old| {
                    p.countries
                        .iter()
                        .any(|c| old.iter().any(|o| o.eq_ignore_ascii_case(c)))
                })
            })
            .collect();
        let (offers, provs): (Vec<Offer>, Vec<&ProviderConfig>) = candidates
            .into_iter()
            .map(|p| (self.offer(p, domain), p))
            .filter(|(o, _)| check_eligible(o, &risk, need).is_ok())
            .unzip();
        let pick = default_pick(&offers, policy.reliability_floor)?;
        if !pick.meets_floor {
            tracing::debug!(domain, provider = %provs[pick.index].name, "no exit meets the reliability floor");
        }
        Some(provs[pick.index])
    }

    fn offer(&self, p: &ProviderConfig, domain: &str) -> Offer {
        Offer {
            provider: p.name.clone(),
            exit_type: p.exit_type,
            sourcing: p.sourcing,
            price_per_gb: p.price_per_gb,
            fraud_score: p.fraud_score,
            max_sticky_minutes: p.max_sticky_minutes,
            latency_ms: p.latency_ms,
            success_rate: self
                .success
                .success_rate(domain, &p.name)
                .unwrap_or(NEUTRAL_SUCCESS),
        }
    }

    fn pinned_session_id(k: &PinKey, generation: u32) -> String {
        let route = k.route.as_deref().unwrap_or("");
        let h = fnv(&[
            k.key_name.as_bytes(),
            route.as_bytes(),
            k.session.as_bytes(),
            &generation.to_le_bytes(),
        ]);
        format!("hip{h:016x}")
    }

    fn rotating_session_id(&self, key_name: &str) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let h = fnv(&[
            &self.seed.to_le_bytes(),
            &n.to_le_bytes(),
            key_name.as_bytes(),
        ]);
        format!("hip{h:016x}")
    }

    /// Pick an exit for one connection.
    pub fn choose(
        &self,
        who: &AuthedUser,
        target: &TargetAddr,
        now: Instant,
    ) -> Result<Choice, SelectError> {
        let policy = self.effective_policy(&who.user)?;
        let domain = target_domain(&who.user, target);
        let country = policy.geo.country.clone();
        let no_exit = || SelectError::NoEligibleExit {
            domain: domain.clone(),
        };

        let session = match (&who.user.session, policy.mode) {
            (Some(s), _) => Some(s.clone()),
            (None, RouteMode::Sticky) => Some(ROUTE_SESSION.to_string()),
            (None, RouteMode::Rotating) => None,
        };
        let Some(session) = session else {
            let p = self.pick(&policy, &domain, None).ok_or_else(no_exit)?;
            let sid = self.rotating_session_id(&who.key_name);
            return Ok(Choice {
                provider: p.name.clone(),
                upstream: p.upstream.render(&sid, false, country.as_deref()),
                pin: None,
                reused_pin: false,
                on_pin_loss: PinLoss::Any,
                domain,
            });
        };

        let sticky = policy.sticky.clone().unwrap_or_default();
        let key = PinKey {
            key_name: who.key_name.clone(),
            route: who.user.route.clone(),
            session,
        };
        let choice = |p: &ProviderConfig, sid: &str, reused: bool| Choice {
            provider: p.name.clone(),
            upstream: p.upstream.render(sid, true, country.as_deref()),
            pin: Some(key.clone()),
            reused_pin: reused,
            on_pin_loss: sticky.on_pin_loss,
            domain: domain.clone(),
        };

        let mut generation = 0;
        let mut provider = None;
        let mut near = None;
        if let Some(old) = self.sessions.get(&key, now) {
            let old_provider = self.cfg.provider(&old.provider);
            if let (false, Some(p)) = (old.lost, old_provider) {
                self.sessions.touch(&key, now);
                return Ok(choice(p, &old.upstream_session, true));
            }
            generation = old.generation.wrapping_add(1);
            let lost = || SelectError::PinLost {
                provider: old.provider.clone(),
            };
            match sticky.on_pin_loss {
                PinLoss::FailClosed => return Err(lost()),
                // No per-IP ASN or city data yet, so the same provider is the
                // closest approximation: keep it and ask for a new session.
                PinLoss::SameAsn | PinLoss::SameCity => {
                    provider = Some(old_provider.ok_or_else(lost)?);
                }
                PinLoss::SameCountry => {
                    let old_p = old_provider.ok_or_else(lost)?;
                    if !old_p.countries.is_empty() {
                        near = Some(old_p.countries.clone());
                    }
                }
                PinLoss::Any => {}
            }
            tracing::info!(provider = %old.provider, rule = ?sticky.on_pin_loss, "replacing lost pin");
        }

        let p = match provider {
            Some(p) => p,
            None => self
                .pick(&policy, &domain, near.as_deref())
                .ok_or_else(no_exit)?,
        };
        let sid = Self::pinned_session_id(&key, generation);
        self.sessions.insert(
            key.clone(),
            Pin {
                provider: p.name.clone(),
                upstream_session: sid.clone(),
                idle_ttl: idle_ttl(sticky.ttl),
                last_seen: now,
                lost: false,
                generation,
            },
        );
        Ok(choice(p, &sid, false))
    }

    /// Called when dialing the chosen upstream failed. Pinned: mark the pin lost; FailClosed if on_pin_loss is FailClosed, else Retry. Unpinned: Retry.
    pub fn report_dial_failure(&self, choice: &Choice) -> DialFailure {
        let Some(key) = &choice.pin else {
            return DialFailure::Retry;
        };
        self.sessions.mark_lost(key);
        if choice.on_pin_loss == PinLoss::FailClosed {
            DialFailure::FailClosed
        } else {
            DialFailure::Retry
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_domain_prefers_username_target_then_host_then_ip() {
        let t = TargetAddr::Domain("www.Example.com".into(), 443);
        let ip = TargetAddr::Ip("10.1.2.3:80".parse().unwrap());
        let mut u = ProxyUser::default();
        assert_eq!(target_domain(&u, &t), "example.com");
        assert_eq!(target_domain(&u, &ip), "10.1.2.3");
        u.target = Some("chase.com".into());
        assert_eq!(target_domain(&u, &t), "chase.com");
        assert_eq!(target_domain(&u, &ip), "chase.com");
    }

    #[test]
    fn parse_user_accepts_bare_user() {
        assert!(parse_user("user").unwrap().session.is_none());
        assert!(parse_user("user-").is_ok());
        assert!(matches!(
            parse_user("alice"),
            Err(AuthError::BadUsername(_))
        ));
    }

    #[test]
    fn idle_ttl_mapping() {
        assert_eq!(idle_ttl(StickyTtl::Session), Some(SESSION_IDLE));
        assert_eq!(
            idle_ttl(StickyTtl::Hours(2)),
            Some(Duration::from_secs(7200))
        );
        assert_eq!(
            idle_ttl(StickyTtl::Days(1)),
            Some(Duration::from_secs(86_400))
        );
        assert_eq!(idle_ttl(StickyTtl::Forever), None);
    }

    #[test]
    fn pinned_ids_are_deterministic_and_short() {
        let k = PinKey {
            key_name: "a".into(),
            route: None,
            session: "s".into(),
        };
        let id = Selector::pinned_session_id(&k, 0);
        assert_eq!(id, Selector::pinned_session_id(&k, 0));
        assert_ne!(id, Selector::pinned_session_id(&k, 1));
        assert_eq!(id.len(), 19);
        assert!(id.starts_with("hip"));
    }
}
