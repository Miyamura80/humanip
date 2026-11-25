//! Passive health: time-decayed success share per (provider, domain).
//!
//! The gateway mostly relays opaque TLS tunnels (HTTP CONNECT, SOCKS5), so on
//! its own it can only observe transport signals: upstream dial success or
//! failure, handshake timeouts, connection resets, and session duration and
//! bytes. Content signals (HTTP 403/429, CAPTCHA pages) are visible only when
//! the client reports them; a later API feeds those through the same
//! [`HealthTracker::record`] call, using [`classify_http_status`] for codes.
//!
//! Each pair keeps decayed `good` and `total` sums. Older observations lose
//! half their weight every `half_life`, so recent outcomes dominate.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// What happened to one attempt through an exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Tunnel or request worked.
    Success,
    /// Upstream dial or proxy handshake was refused or failed.
    ConnectFailed,
    /// Dial or handshake took too long.
    Timeout,
    /// Connection was reset early.
    Reset,
    /// Target blocked the exit (client reported, e.g. 403 or 429).
    Blocked,
    /// Target served a CAPTCHA (client reported).
    Captcha,
}

impl Outcome {
    pub fn is_success(self) -> bool {
        matches!(self, Outcome::Success)
    }
}

/// Map a client-reported HTTP status to an outcome.
///
/// 403, 429 and 451 are `Blocked`; 2xx and 3xx are `Success`. Anything else
/// (404, 5xx, ...) is `None` because it says nothing about the exit.
pub fn classify_http_status(status: u16) -> Option<Outcome> {
    match status {
        403 | 429 | 451 => Some(Outcome::Blocked),
        200..=399 => Some(Outcome::Success),
        _ => None,
    }
}

/// One row of [`HealthTracker::snapshot`].
#[derive(Clone, Debug, PartialEq)]
pub struct HealthRow {
    pub provider: String,
    pub domain: String,
    /// Decayed success share in 0..=1.
    pub success_rate: f64,
    /// Raw observation count (not decayed).
    pub samples: u64,
}

#[derive(Clone, Copy, Debug)]
struct Stat {
    good: f64,
    total: f64,
    last: Instant,
    count: u64,
}

impl Stat {
    /// Decayed (good, total) as seen at `now`, without mutating.
    fn decayed(&self, now: Instant, half_life: Duration) -> (f64, f64) {
        let f = decay_factor(self.last, now, half_life);
        (self.good * f, self.total * f)
    }

    fn rate(&self, now: Instant, half_life: Duration) -> Option<f64> {
        let (good, total) = self.decayed(now, half_life);
        (total > 0.0).then(|| good / total)
    }
}

/// `0.5^(elapsed / half_life)`; elapsed is zero when `now` precedes `last`.
fn decay_factor(last: Instant, now: Instant, half_life: Duration) -> f64 {
    let elapsed = now.saturating_duration_since(last).as_secs_f64();
    let hl = half_life.as_secs_f64();
    if hl <= 0.0 {
        return if elapsed > 0.0 { 0.0 } else { 1.0 };
    }
    0.5f64.powf(elapsed / hl)
}

/// Thread-safe tracker of decayed success per (provider, domain).
pub struct HealthTracker {
    half_life: Duration,
    min_samples: u64,
    stats: Mutex<HashMap<(String, String), Stat>>,
}

impl HealthTracker {
    pub fn new(half_life: Duration, min_samples: u64) -> Self {
        Self {
            half_life,
            min_samples,
            stats: Mutex::new(HashMap::new()),
        }
    }

    pub fn half_life(&self) -> Duration {
        self.half_life
    }

    pub fn min_samples(&self) -> u64 {
        self.min_samples
    }

    /// Lock the map, recovering from a poisoned lock (the data stays valid).
    fn lock(&self) -> MutexGuard<'_, HashMap<(String, String), Stat>> {
        self.stats.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Record one outcome at `now`. `now` is injected so tests need no sleep.
    pub fn record(&self, provider: &str, domain: &str, outcome: Outcome, now: Instant) {
        let key = (provider.to_string(), domain.to_ascii_lowercase());
        let hit = if outcome.is_success() { 1.0 } else { 0.0 };
        let mut map = self.lock();
        let stat = map.entry(key).or_insert(Stat {
            good: 0.0,
            total: 0.0,
            last: now,
            count: 0,
        });
        let (good, total) = stat.decayed(now, self.half_life);
        stat.good = good + hit;
        stat.total = total + 1.0;
        stat.count += 1;
        if now > stat.last {
            stat.last = now;
        }
    }

    /// Decayed success share, or `None` until `min_samples` raw observations exist.
    pub fn rate(&self, provider: &str, domain: &str, now: Instant) -> Option<f64> {
        let key = (provider.to_string(), domain.to_ascii_lowercase());
        let map = self.lock();
        let stat = map.get(&key)?;
        if stat.count < self.min_samples {
            return None;
        }
        stat.rate(now, self.half_life)
    }

    /// 0-100 health for a provider across its domains, weighted by decayed
    /// sample mass. `None` without data. Ignores `min_samples`.
    pub fn provider_score(&self, provider: &str, now: Instant) -> Option<u8> {
        let map = self.lock();
        let (mut good, mut total) = (0.0, 0.0);
        for ((p, _), stat) in map.iter() {
            if p == provider {
                let (g, t) = stat.decayed(now, self.half_life);
                good += g;
                total += t;
            }
        }
        if total <= 0.0 {
            return None;
        }
        Some((good / total * 100.0).round().clamp(0.0, 100.0) as u8)
    }

    /// Every tracked pair, sorted by provider then domain. Ignores `min_samples`.
    pub fn snapshot(&self, now: Instant) -> Vec<HealthRow> {
        let map = self.lock();
        let mut rows: Vec<HealthRow> = map
            .iter()
            .map(|((provider, domain), stat)| HealthRow {
                provider: provider.clone(),
                domain: domain.clone(),
                success_rate: stat.rate(now, self.half_life).unwrap_or(0.0),
                samples: stat.count,
            })
            .collect();
        rows.sort_by(|a, b| (&a.provider, &a.domain).cmp(&(&b.provider, &b.domain)));
        rows
    }
}

impl Default for HealthTracker {
    /// Half-life of one hour, 20 samples before a rate is reported.
    fn default() -> Self {
        Self::new(Duration::from_secs(3600), 20)
    }
}

impl crate::SuccessSource for HealthTracker {
    fn success_rate(&self, domain: &str, provider: &str) -> Option<f64> {
        self.rate(provider, domain, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decay_factor_guards_backwards_time() {
        let base = Instant::now();
        let later = base + Duration::from_secs(10);
        assert_eq!(decay_factor(later, base, Duration::from_secs(5)), 1.0);
        let f = decay_factor(base, later, Duration::from_secs(5));
        assert!((f - 0.25).abs() < 1e-12);
    }
}
