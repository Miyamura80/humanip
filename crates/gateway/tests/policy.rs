//! Policy layer: auth, exit selection and sticky pins, end to end without network.

use humanip_core::policy::PinLoss;
use humanip_gateway::config::{parse_yaml, GatewayConfig};
use humanip_gateway::select::{
    authenticate, AuthError, AuthedUser, Choice, DialFailure, SelectError, Selector,
};
use humanip_gateway::{SuccessSource, TargetAddr, Upstream};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const YAML: &str = r#"
api_keys:
  - { name: alice, key: k-alice }
  - { name: bob, key: k-bob }
providers:
  - name: cheap
    exit_type: residential
    sourcing: verified
    price_per_gb: 2.0
    fraud_score: 10
    max_sticky_minutes: 200000
    latency_ms: 300
    countries: [US]
    upstream: { kind: socks5, addr: "10.0.0.1:1080", username: "cheap-{session}-{country}", password: p }
  - name: pricey
    exit_type: residential
    sourcing: verified
    price_per_gb: 4.0
    fraud_score: 10
    max_sticky_minutes: 200000
    latency_ms: 300
    countries: [US, DE]
    upstream: { kind: socks5, addr: "10.0.0.2:1080", username: "pricey-{session}-{country}", password: p }
  - name: mobile
    exit_type: mobile
    sourcing: verified
    price_per_gb: 9.0
    fraud_score: 10
    max_sticky_minutes: 200000
    latency_ms: 300
    upstream: { kind: http, addr: "10.0.0.3:8080", username: "mob-{session}", password: p }
routes:
  - name: bridge
    sticky: { ttl: forever, on_pin_loss: fail_closed }
  - name: asn
    sticky: { ttl: { hours: 2 }, on_pin_loss: same_asn }
  - name: anyloss
    sticky: { on_pin_loss: any }
  - name: samecc
    sticky: { on_pin_loss: same_country }
  - name: scrape
    mode: rotating
"#;

/// Success rates per provider, changeable mid-test.
#[derive(Default)]
struct Fake(Mutex<HashMap<String, f64>>);

impl Fake {
    fn set(&self, provider: &str, rate: f64) {
        self.0.lock().unwrap().insert(provider.into(), rate);
    }
}

impl SuccessSource for Fake {
    fn success_rate(&self, _domain: &str, provider: &str) -> Option<f64> {
        self.0.lock().unwrap().get(provider).copied()
    }
}

fn setup(rates: &[(&str, f64)]) -> (Selector, Arc<Fake>) {
    let cfg: Arc<GatewayConfig> = Arc::new(parse_yaml(YAML).expect("fixture parses"));
    let fake = Arc::new(Fake::default());
    for (p, r) in rates {
        fake.set(p, *r);
    }
    (Selector::new(cfg, fake.clone()), fake)
}

fn who(sel: &Selector, username: &str) -> AuthedUser {
    authenticate(sel.config(), username, "k-alice").expect("auth")
}

fn target() -> TargetAddr {
    TargetAddr::Domain("www.Example.com".into(), 443)
}

fn username_of(c: &Choice) -> String {
    match &c.upstream {
        Upstream::Socks5 { username, .. } | Upstream::Http { username, .. } => username.clone(),
        Upstream::Direct => String::new(),
    }
}

/// All providers healthy, cheapest wins.
const HEALTHY: &[(&str, f64)] = &[("cheap", 0.97), ("pricey", 0.97), ("mobile", 0.99)];

#[test]
fn authenticate_accepts_valid_key_and_rejects_others() {
    let (sel, _) = setup(HEALTHY);
    let a = authenticate(sel.config(), "user-session_x", "k-bob").unwrap();
    assert_eq!(a.key_name, "bob");
    assert_eq!(a.user.session.as_deref(), Some("x"));
    assert!(matches!(
        authenticate(sel.config(), "user-session_x", "k-nope"),
        Err(AuthError::BadKey)
    ));
    assert!(matches!(
        authenticate(sel.config(), "user-session_x", ""),
        Err(AuthError::BadKey)
    ));
    assert!(matches!(
        authenticate(sel.config(), "alice", "k-alice"),
        Err(AuthError::BadUsername(_))
    ));
}

#[test]
fn unknown_route_is_rejected() {
    let (sel, _) = setup(HEALTHY);
    let err = sel.choose(&who(&sel, "user-route_nope"), &target(), Instant::now());
    assert!(matches!(err, Err(SelectError::UnknownRoute(r)) if r == "nope"));
}

#[test]
fn reliability_first_pick_prefers_pricier_exit_above_floor() {
    let (sel, fake) = setup(&[("cheap", 0.93), ("pricey", 0.97), ("mobile", 0.99)]);
    let u = who(&sel, "user");
    let c = sel.choose(&u, &target(), Instant::now()).unwrap();
    assert_eq!(c.provider, "pricey");
    assert_eq!(c.domain, "example.com");
    fake.set("cheap", 0.96);
    assert_eq!(
        sel.choose(&u, &target(), Instant::now()).unwrap().provider,
        "cheap"
    );
}

#[test]
fn no_data_falls_back_to_neutral_success() {
    let (sel, _) = setup(&[]);
    let c = sel.choose(&who(&sel, "user"), &target(), Instant::now());
    assert_eq!(c.unwrap().provider, "cheap");
}

#[test]
fn sticky_session_reuses_provider_and_upstream_session() {
    let (sel, fake) = setup(HEALTHY);
    let u = who(&sel, "user-session_s1");
    let t = Instant::now();
    let first = sel.choose(&u, &target(), t).unwrap();
    assert!(!first.reused_pin);
    assert_eq!(first.provider, "cheap");
    // Even if health changes, a live pin is kept.
    fake.set("cheap", 0.6);
    let second = sel
        .choose(&u, &target(), t + Duration::from_secs(60))
        .unwrap();
    assert!(second.reused_pin);
    assert_eq!(second.provider, first.provider);
    assert_eq!(second.upstream, first.upstream);
    assert_eq!(sel.sessions().len(), 1);
    let key = second.pin.unwrap();
    assert_eq!((key.key_name.as_str(), key.route), ("alice", None));
}

#[test]
fn named_sticky_route_pins_without_client_session() {
    let (sel, _) = setup(HEALTHY);
    let u = who(&sel, "user-route_bridge");
    let a = sel.choose(&u, &target(), Instant::now()).unwrap();
    let key = a.pin.clone().unwrap();
    assert_eq!(key.session, "_route");
    assert_eq!(key.route.as_deref(), Some("bridge"));
    assert!(
        sel.choose(&u, &target(), Instant::now())
            .unwrap()
            .reused_pin
    );
}

#[test]
fn idle_expiry_reselects() {
    let (sel, _) = setup(HEALTHY);
    let u = who(&sel, "user-session_s1");
    let t = Instant::now();
    sel.choose(&u, &target(), t).unwrap();
    let inside = sel.choose(&u, &target(), t + Duration::from_secs(29 * 60));
    assert!(inside.unwrap().reused_pin);
    let after = t + Duration::from_secs(29 * 60) + Duration::from_secs(31 * 60);
    assert!(!sel.choose(&u, &target(), after).unwrap().reused_pin);
}

#[test]
fn rotating_gives_fresh_upstream_sessions() {
    let (sel, _) = setup(HEALTHY);
    for name in ["user", "user-route_scrape"] {
        let u = who(&sel, name);
        let a = sel.choose(&u, &target(), Instant::now()).unwrap();
        let b = sel.choose(&u, &target(), Instant::now()).unwrap();
        assert!(a.pin.is_none());
        assert_ne!(a.upstream, b.upstream);
        assert_eq!(a.on_pin_loss, PinLoss::Any);
        assert!(username_of(&a).ends_with("-any"));
    }
    assert!(sel.sessions().is_empty());
}

#[test]
fn country_filter_and_render() {
    let (sel, _) = setup(HEALTHY);
    let c = sel
        .choose(&who(&sel, "user-country_de"), &target(), Instant::now())
        .unwrap();
    assert_eq!(c.provider, "pricey");
    assert!(username_of(&c).ends_with("-de"));
    let us = sel
        .choose(&who(&sel, "user-country_us"), &target(), Instant::now())
        .unwrap();
    assert_eq!(us.provider, "cheap");
}

#[test]
fn exit_override_and_no_eligible_exit() {
    let (sel, _) = setup(HEALTHY);
    let c = sel
        .choose(&who(&sel, "user-exit_mobile"), &target(), Instant::now())
        .unwrap();
    assert_eq!(c.provider, "mobile");
    assert!(matches!(c.upstream, Upstream::Http { .. }));
    let none = sel.choose(&who(&sel, "user-exit_isp"), &target(), Instant::now());
    assert!(matches!(none, Err(SelectError::NoEligibleExit { domain }) if domain == "example.com"));
}

#[test]
fn target_param_and_ip_target_set_domain() {
    let (sel, _) = setup(HEALTHY);
    let ip = TargetAddr::Ip("[::1]:443".parse().unwrap());
    let c = sel.choose(&who(&sel, "user"), &ip, Instant::now()).unwrap();
    assert_eq!(c.domain, "::1");
    let u = who(&sel, "user-target_LinkedIn.com");
    let c = sel.choose(&u, &target(), Instant::now()).unwrap();
    assert_eq!(c.domain, "linkedin.com");
}

#[test]
fn fail_closed_pin_loss_refuses() {
    let (sel, _) = setup(HEALTHY);
    let u = who(&sel, "user-route_bridge");
    let t = Instant::now();
    let c = sel.choose(&u, &target(), t).unwrap();
    assert_eq!(c.on_pin_loss, PinLoss::FailClosed);
    assert_eq!(sel.report_dial_failure(&c), DialFailure::FailClosed);
    let err = sel.choose(&u, &target(), t + Duration::from_secs(86_400));
    assert!(matches!(err, Err(SelectError::PinLost { provider }) if provider == "cheap"));
}

#[test]
fn same_asn_pin_loss_keeps_provider_with_new_session() {
    let (sel, fake) = setup(HEALTHY);
    let u = who(&sel, "user-route_asn");
    let t = Instant::now();
    let first = sel.choose(&u, &target(), t).unwrap();
    assert_eq!(sel.report_dial_failure(&first), DialFailure::Retry);
    fake.set("cheap", 0.5);
    let second = sel.choose(&u, &target(), t).unwrap();
    assert_eq!(second.provider, first.provider);
    assert_ne!(second.upstream, first.upstream);
    assert!(!second.reused_pin);
    let third = sel.choose(&u, &target(), t).unwrap();
    assert!(third.reused_pin);
    assert_eq!(third.upstream, second.upstream);
    let pin = sel.sessions().get(&third.pin.unwrap(), t).unwrap();
    assert_eq!(pin.generation, 1);
    assert_eq!(pin.idle_ttl, Some(Duration::from_secs(7200)));
}

#[test]
fn any_pin_loss_reselects() {
    let (sel, fake) = setup(HEALTHY);
    let u = who(&sel, "user-route_anyloss");
    let t = Instant::now();
    let first = sel.choose(&u, &target(), t).unwrap();
    assert_eq!(first.provider, "cheap");
    assert_eq!(sel.report_dial_failure(&first), DialFailure::Retry);
    fake.set("cheap", 0.5);
    let second = sel.choose(&u, &target(), t).unwrap();
    assert_eq!(second.provider, "pricey");
    assert!(!second.reused_pin);
}

#[test]
fn same_country_pin_loss_stays_in_country() {
    // pricey is below the floor but is the only other provider sharing US.
    let (sel, fake) = setup(&[("cheap", 0.97), ("pricey", 0.9), ("mobile", 0.999)]);
    let u = who(&sel, "user-route_samecc");
    let t = Instant::now();
    let first = sel.choose(&u, &target(), t).unwrap();
    assert_eq!(first.provider, "cheap");
    sel.report_dial_failure(&first);
    fake.set("cheap", 0.5);
    let second = sel.choose(&u, &target(), t).unwrap();
    assert_eq!(second.provider, "pricey");
}

#[test]
fn report_dial_failure_unpinned_is_retry() {
    let (sel, _) = setup(HEALTHY);
    let c = sel
        .choose(&who(&sel, "user"), &target(), Instant::now())
        .unwrap();
    assert_eq!(sel.report_dial_failure(&c), DialFailure::Retry);
    // Ad-hoc client sessions fail over; only named routes can fail closed.
    let s = sel
        .choose(&who(&sel, "user-session_z"), &target(), Instant::now())
        .unwrap();
    assert_eq!(sel.report_dial_failure(&s), DialFailure::Retry);
    assert!(sel
        .choose(&who(&sel, "user-session_z"), &target(), Instant::now())
        .is_ok());
}
