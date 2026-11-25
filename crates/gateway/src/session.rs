//! Sticky pin table: which provider and upstream session a client session uses.
//!
//! The clock is passed in as `now` so callers (and tests) control time.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Identifies one client sticky session.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PinKey {
    pub key_name: String,
    /// Named route, or None for ad-hoc username settings.
    pub route: Option<String>,
    pub session: String,
}

/// Where a sticky session is pinned.
#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    pub provider: String,
    /// Session id sent to the provider in its upstream username.
    pub upstream_session: String,
    /// Idle time after which the pin is dropped. None = never expires.
    pub idle_ttl: Option<Duration>,
    pub last_seen: Instant,
    /// The pinned exit stopped working.
    pub lost: bool,
    /// Bumped each time the pin is replaced after a loss.
    pub generation: u32,
}

impl Pin {
    fn expired(&self, now: Instant) -> bool {
        self.idle_ttl
            .is_some_and(|ttl| now.saturating_duration_since(self.last_seen) > ttl)
    }
}

/// Thread-safe map of sticky pins.
#[derive(Debug, Default)]
pub struct SessionTable {
    pins: Mutex<HashMap<PinKey, Pin>>,
}

impl SessionTable {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<PinKey, Pin>> {
        // A panic while holding the lock cannot leave the map half-written.
        self.pins.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The pin for `k`. Drops it and returns None if it has gone idle too long.
    pub fn get(&self, k: &PinKey, now: Instant) -> Option<Pin> {
        let mut pins = self.lock();
        if pins.get(k)?.expired(now) {
            pins.remove(k);
            return None;
        }
        pins.get(k).cloned()
    }

    /// Insert or replace the pin for `k`.
    pub fn insert(&self, k: PinKey, p: Pin) {
        self.lock().insert(k, p);
    }

    /// Record use of `k` at `now`, resetting its idle timer.
    pub fn touch(&self, k: &PinKey, now: Instant) {
        if let Some(p) = self.lock().get_mut(k) {
            p.last_seen = now;
        }
    }

    /// Mark the pinned exit as lost. Returns false if there is no pin.
    pub fn mark_lost(&self, k: &PinKey) -> bool {
        match self.lock().get_mut(k) {
            Some(p) => {
                p.lost = true;
                true
            }
            None => false,
        }
    }

    /// Remove a pin, for example when an operator clears a fail-closed route.
    pub fn remove(&self, k: &PinKey) -> Option<Pin> {
        self.lock().remove(k)
    }

    /// Drop every idle-expired pin. Returns how many were dropped.
    pub fn purge_expired(&self, now: Instant) -> usize {
        let mut pins = self.lock();
        let before = pins.len();
        pins.retain(|_, p| !p.expired(now));
        before - pins.len()
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> PinKey {
        PinKey {
            key_name: "alice".into(),
            route: None,
            session: s.into(),
        }
    }

    fn pin(now: Instant, ttl: Option<Duration>) -> Pin {
        Pin {
            provider: "resi".into(),
            upstream_session: "hip0".into(),
            idle_ttl: ttl,
            last_seen: now,
            lost: false,
            generation: 0,
        }
    }

    const MIN: Duration = Duration::from_secs(60);

    #[test]
    fn get_returns_inserted_pin() {
        let (t, s) = (Instant::now(), SessionTable::new());
        assert!(s.is_empty());
        assert_eq!(s.get(&key("a"), t), None);
        s.insert(key("a"), pin(t, Some(MIN)));
        assert_eq!(s.get(&key("a"), t).unwrap().provider, "resi");
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn idle_expiry_drops_on_get_and_touch_extends() {
        let (t, s) = (Instant::now(), SessionTable::new());
        s.insert(key("a"), pin(t, Some(MIN)));
        assert!(s.get(&key("a"), t + MIN).is_some());
        s.touch(&key("a"), t + MIN);
        assert!(s.get(&key("a"), t + MIN * 2).is_some());
        assert_eq!(s.get(&key("a"), t + MIN * 2 + Duration::from_secs(1)), None);
        assert!(s.is_empty());
    }

    #[test]
    fn none_ttl_never_expires() {
        let (t, s) = (Instant::now(), SessionTable::new());
        s.insert(key("a"), pin(t, None));
        let later = t + Duration::from_secs(365 * 24 * 3600);
        assert!(s.get(&key("a"), later).is_some());
        assert_eq!(s.purge_expired(later), 0);
    }

    #[test]
    fn mark_lost_and_remove() {
        let (t, s) = (Instant::now(), SessionTable::new());
        assert!(!s.mark_lost(&key("a")));
        s.insert(key("a"), pin(t, None));
        assert!(s.mark_lost(&key("a")));
        assert!(s.get(&key("a"), t).unwrap().lost);
        assert!(s.remove(&key("a")).is_some());
        assert!(s.is_empty());
    }

    #[test]
    fn purge_drops_only_expired() {
        let (t, s) = (Instant::now(), SessionTable::new());
        s.insert(key("short"), pin(t, Some(MIN)));
        s.insert(key("long"), pin(t, Some(MIN * 10)));
        s.insert(key("forever"), pin(t, None));
        assert_eq!(s.purge_expired(t + MIN * 2), 1);
        assert_eq!(s.len(), 2);
        assert_eq!(s.get(&key("short"), t), None);
    }
}
