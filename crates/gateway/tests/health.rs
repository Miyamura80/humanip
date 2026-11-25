use std::time::{Duration, Instant};

use humanip_gateway::health::{classify_http_status, HealthRow, HealthTracker, Outcome};
use humanip_gateway::SuccessSource;

const HOUR: Duration = Duration::from_secs(3600);

fn tracker(min: u64) -> HealthTracker {
    HealthTracker::new(HOUR, min)
}

fn record_n(t: &HealthTracker, p: &str, d: &str, o: Outcome, n: usize, now: Instant) {
    for _ in 0..n {
        t.record(p, d, o, now);
    }
}

#[test]
fn none_below_min_samples() {
    let t = tracker(5);
    let now = Instant::now();
    record_n(&t, "a", "x.com", Outcome::Success, 4, now);
    assert_eq!(t.rate("a", "x.com", now), None);
    t.record("a", "x.com", Outcome::Success, now);
    assert_eq!(t.rate("a", "x.com", now), Some(1.0));
}

#[test]
fn exact_rate_without_elapsed_time() {
    let t = tracker(1);
    let now = Instant::now();
    record_n(&t, "a", "x.com", Outcome::Success, 3, now);
    t.record("a", "x.com", Outcome::Timeout, now);
    assert_eq!(t.rate("a", "x.com", now), Some(0.75));
}

#[test]
fn recent_failures_dominate_old_successes() {
    let t = tracker(20);
    let base = Instant::now();
    record_n(&t, "a", "x.com", Outcome::Success, 100, base);
    let later = base + 2 * HOUR;
    record_n(&t, "a", "x.com", Outcome::Reset, 20, later);
    // 100 successes decay to 25 against 20 fresh failures: 25 / 45.
    let r = t.rate("a", "x.com", later).unwrap();
    assert!((r - 25.0 / 45.0).abs() < 1e-9, "rate {r}");
    // A further burst of failures pushes it well under half.
    record_n(&t, "a", "x.com", Outcome::Blocked, 30, later);
    let r = t.rate("a", "x.com", later).unwrap();
    assert!(r < 0.4, "rate {r}");
}

#[test]
fn half_life_math() {
    let t = tracker(1);
    let base = Instant::now();
    record_n(&t, "a", "x.com", Outcome::Success, 10, base);
    let one = base + HOUR;
    t.record("a", "x.com", Outcome::ConnectFailed, one);
    // good 10 * 0.5 = 5, total 5 + 1 = 6.
    let r = t.rate("a", "x.com", one).unwrap();
    assert!((r - 5.0 / 6.0).abs() < 1e-9, "rate {r}");
    // Reading later decays good and total alike, so the rate holds.
    let r2 = t.rate("a", "x.com", one + 3 * HOUR).unwrap();
    assert!((r2 - r).abs() < 1e-9);
    // Reads do not mutate state.
    assert!((t.rate("a", "x.com", one).unwrap() - r).abs() < 1e-12);
}

#[test]
fn reads_before_last_record_do_not_amplify() {
    let t = tracker(1);
    let base = Instant::now();
    let later = base + HOUR;
    t.record("a", "x.com", Outcome::Success, later);
    t.record("a", "x.com", Outcome::Captcha, base);
    assert_eq!(t.rate("a", "x.com", base), Some(0.5));
}

#[test]
fn provider_score_weights_by_samples() {
    let t = tracker(1);
    let now = Instant::now();
    record_n(&t, "a", "x.com", Outcome::Success, 90, now);
    record_n(&t, "a", "x.com", Outcome::Blocked, 10, now);
    record_n(&t, "a", "y.com", Outcome::Blocked, 10, now);
    // (90) / (100 + 10) = 81.8 -> 82; a plain mean of domains would be 45.
    assert_eq!(t.provider_score("a", now), Some(82));
    assert_eq!(t.provider_score("b", now), None);
}

#[test]
fn snapshot_is_sorted() {
    let t = tracker(100);
    let now = Instant::now();
    t.record("b", "a.com", Outcome::Success, now);
    t.record("a", "z.com", Outcome::Timeout, now);
    t.record("a", "b.com", Outcome::Success, now);
    let rows = t.snapshot(now);
    let keys: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r.provider.as_str(), r.domain.as_str()))
        .collect();
    assert_eq!(keys, vec![("a", "b.com"), ("a", "z.com"), ("b", "a.com")]);
    assert_eq!(
        rows[1],
        HealthRow {
            provider: "a".into(),
            domain: "z.com".into(),
            success_rate: 0.0,
            samples: 1,
        }
    );
}

#[test]
fn domains_are_case_insensitive() {
    let t = tracker(2);
    let now = Instant::now();
    t.record("a", "Example.COM", Outcome::Success, now);
    t.record("a", "example.com", Outcome::Reset, now);
    assert_eq!(t.rate("a", "EXAMPLE.com", now), Some(0.5));
    assert_eq!(t.snapshot(now).len(), 1);
}

#[test]
fn classify_http_status_table() {
    let cases = [
        (200, Some(Outcome::Success)),
        (204, Some(Outcome::Success)),
        (301, Some(Outcome::Success)),
        (399, Some(Outcome::Success)),
        (403, Some(Outcome::Blocked)),
        (429, Some(Outcome::Blocked)),
        (451, Some(Outcome::Blocked)),
        (100, None),
        (400, None),
        (404, None),
        (500, None),
        (503, None),
    ];
    for (status, want) in cases {
        assert_eq!(classify_http_status(status), want, "status {status}");
    }
    assert!(Outcome::Success.is_success());
    assert!(!Outcome::Captcha.is_success());
}

#[test]
fn success_source_none_for_unknown() {
    let t = HealthTracker::default();
    assert_eq!(t.half_life(), HOUR);
    assert_eq!(t.min_samples(), 20);
    let src: &dyn SuccessSource = &t;
    assert_eq!(src.success_rate("x.com", "a"), None);
    let now = Instant::now();
    record_n(&t, "a", "x.com", Outcome::Success, 20, now);
    // Argument order is (domain, provider).
    assert_eq!(src.success_rate("X.com", "a"), Some(1.0));
    assert_eq!(src.success_rate("a", "x.com"), None);
}

#[test]
fn concurrent_records_are_counted() {
    let t = tracker(1);
    let now = Instant::now();
    std::thread::scope(|s| {
        for i in 0..8 {
            let t = &t;
            s.spawn(move || {
                let o = if i % 2 == 0 {
                    Outcome::Success
                } else {
                    Outcome::Timeout
                };
                record_n(t, "a", "x.com", o, 250, now);
            });
        }
    });
    let rows = t.snapshot(now);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].samples, 2000);
    assert!((rows[0].success_rate - 0.5).abs() < 1e-9);
}
