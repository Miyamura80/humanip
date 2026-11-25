//! Accounts contract shared by the gateway (auth, admission, metering), the
//! ledger (SQLite store) and billing (top-ups). Pure types and a trait; the
//! in-memory store here backs dev configs and tests.
//!
//! Money is integer micro-USD (1 USD = 1_000_000) so sums never drift.
//! Bandwidth is billed per decimal GB (1 GB = 1_000_000_000 bytes).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use thiserror::Error;

pub type MicroUsd = i64;
pub const MICROS_PER_USD: MicroUsd = 1_000_000;
pub const BYTES_PER_GB: f64 = 1_000_000_000.0;

/// Verification level of an account. Ordered: `None < Basic < Verified`.
#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
)]
#[serde(rename_all = "snake_case")]
pub enum KycTier {
    #[default]
    None,
    Basic,
    Verified,
}

/// What the gateway knows about an authenticated key. Never holds the secret.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct KeyRecord {
    /// Public id, safe to log (e.g. "key_3f9a..."). Not the secret.
    pub key_id: String,
    pub account_id: String,
    pub name: String,
    pub kyc_tier: KycTier,
    /// Per-key spend cap for the current calendar month (UTC), if any.
    pub monthly_cap_micro: Option<MicroUsd>,
    pub revoked: bool,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum Denial {
    Revoked,
    InsufficientBalance {
        balance_micro: MicroUsd,
    },
    SpendCapReached {
        cap_micro: MicroUsd,
        spent_micro: MicroUsd,
    },
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub enum Admission {
    Allowed,
    Denied(Denial),
}

/// One finished connection's traffic, reported by the gateway.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct UsageEvent {
    pub key_id: String,
    pub provider: String,
    pub domain: String,
    pub bytes_up: u64,
    pub bytes_down: u64,
    /// Provider list price at the time, USD per GB, before the fee.
    pub provider_price_per_gb: f64,
    pub at_unix: i64,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Charge {
    pub provider_cost_micro: MicroUsd,
    pub fee_micro: MicroUsd,
    pub total_micro: MicroUsd,
}

/// Aggregated usage for reporting.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct UsageRow {
    pub key_id: String,
    pub provider: String,
    pub domain: String,
    pub bytes: u64,
    pub total_micro: MicroUsd,
}

#[derive(Debug, Error, PartialEq)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("storage: {0}")]
    Storage(String),
}

/// Price `bytes` at `price_per_gb` plus `take_rate`, rounded half up to the micro-dollar.
pub fn price_usage(bytes: u64, price_per_gb: f64, take_rate: f64) -> Charge {
    let cost = (bytes as f64 / BYTES_PER_GB) * price_per_gb * MICROS_PER_USD as f64;
    let provider_cost_micro = cost.round() as MicroUsd;
    let fee_micro = (cost * take_rate).round() as MicroUsd;
    Charge {
        provider_cost_micro,
        fee_micro,
        total_micro: provider_cost_micro + fee_micro,
    }
}

/// Storage for keys, balances and usage. Implementations may block; async
/// callers should use `spawn_blocking`.
pub trait AccountStore: Send + Sync {
    /// Resolve a presented secret to its key. `Ok(None)` for unknown secrets.
    /// Must compare in constant time.
    fn authenticate(&self, presented_secret: &str) -> Result<Option<KeyRecord>, StoreError>;
    /// May this key open a new connection now? Checks revocation, a positive
    /// balance and the monthly cap.
    fn admit(&self, key_id: &str, now_unix: i64) -> Result<Admission, StoreError>;
    /// Debit the account for a finished connection; returns what was charged.
    fn record_usage(&self, ev: &UsageEvent) -> Result<Charge, StoreError>;
    /// Add prepaid credit. Idempotent on `external_ref` (e.g. a Stripe
    /// Checkout Session id): returns `false` when it was already applied.
    fn credit(
        &self,
        account_id: &str,
        amount_micro: MicroUsd,
        external_ref: &str,
    ) -> Result<bool, StoreError>;
    fn balance(&self, account_id: &str) -> Result<MicroUsd, StoreError>;
    /// Usage grouped by (key, provider, domain) since `since_unix`. `None` = all accounts.
    fn usage_since(
        &self,
        account_id: Option<&str>,
        since_unix: i64,
    ) -> Result<Vec<UsageRow>, StoreError>;
}

/// Seed entry for [`MemoryAccounts`].
#[derive(Clone, Debug)]
pub struct MemoryKey {
    pub record: KeyRecord,
    pub secret: String,
}

/// Unix seconds at the start of the UTC calendar month containing `unix`.
pub fn month_start_unix(unix: i64) -> i64 {
    let days = unix.div_euclid(86_400);
    // Civil-from-days (Howard Hinnant), then back to the 1st of that month.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5; // day of month, 0-based
    (days - d) * 86_400
}

#[derive(Default)]
struct MemState {
    keys: Vec<MemoryKey>,
    balances: HashMap<String, MicroUsd>,
    refs: std::collections::HashSet<String>,
    usage: Vec<(UsageEvent, Charge, String)>, // (event, charge, account_id)
}

/// In-memory store for dev configs and tests. Secrets compared in constant time.
#[derive(Default)]
pub struct MemoryAccounts {
    state: Mutex<MemState>,
    take_rate: f64,
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= (*a.get(i).unwrap_or(&0) ^ *b.get(i).unwrap_or(&0)) as usize;
    }
    diff == 0
}

impl MemoryAccounts {
    pub fn new(take_rate: f64) -> Self {
        Self {
            state: Mutex::default(),
            take_rate,
        }
    }

    pub fn add_key(&self, key: MemoryKey, starting_balance_micro: MicroUsd) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.balances
            .entry(key.record.account_id.clone())
            .or_insert(starting_balance_micro);
        s.keys.push(key);
    }

    fn spent_this_month(s: &MemState, key_id: &str, now_unix: i64) -> MicroUsd {
        let from = month_start_unix(now_unix);
        s.usage
            .iter()
            .filter(|(e, _, _)| e.key_id == key_id && e.at_unix >= from)
            .map(|(_, c, _)| c.total_micro)
            .sum()
    }
}

impl AccountStore for MemoryAccounts {
    fn authenticate(&self, presented: &str) -> Result<Option<KeyRecord>, StoreError> {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut found = None;
        for k in &s.keys {
            if ct_eq(k.secret.as_bytes(), presented.as_bytes()) {
                found = Some(k.record.clone());
            }
        }
        Ok(found)
    }

    fn admit(&self, key_id: &str, now_unix: i64) -> Result<Admission, StoreError> {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let k = s
            .keys
            .iter()
            .find(|k| k.record.key_id == key_id)
            .ok_or_else(|| StoreError::NotFound(key_id.into()))?;
        if k.record.revoked {
            return Ok(Admission::Denied(Denial::Revoked));
        }
        let balance = *s.balances.get(&k.record.account_id).unwrap_or(&0);
        if balance <= 0 {
            return Ok(Admission::Denied(Denial::InsufficientBalance {
                balance_micro: balance,
            }));
        }
        if let Some(cap) = k.record.monthly_cap_micro {
            let spent = Self::spent_this_month(&s, key_id, now_unix);
            if spent >= cap {
                return Ok(Admission::Denied(Denial::SpendCapReached {
                    cap_micro: cap,
                    spent_micro: spent,
                }));
            }
        }
        Ok(Admission::Allowed)
    }

    fn record_usage(&self, ev: &UsageEvent) -> Result<Charge, StoreError> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let account = s
            .keys
            .iter()
            .find(|k| k.record.key_id == ev.key_id)
            .map(|k| k.record.account_id.clone())
            .ok_or_else(|| StoreError::NotFound(ev.key_id.clone()))?;
        let charge = price_usage(
            ev.bytes_up + ev.bytes_down,
            ev.provider_price_per_gb,
            self.take_rate,
        );
        *s.balances.entry(account.clone()).or_insert(0) -= charge.total_micro;
        s.usage.push((ev.clone(), charge, account));
        Ok(charge)
    }

    fn credit(
        &self,
        account_id: &str,
        amount_micro: MicroUsd,
        external_ref: &str,
    ) -> Result<bool, StoreError> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !s.refs.insert(external_ref.to_string()) {
            return Ok(false);
        }
        *s.balances.entry(account_id.to_string()).or_insert(0) += amount_micro;
        Ok(true)
    }

    fn balance(&self, account_id: &str) -> Result<MicroUsd, StoreError> {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.balances
            .get(account_id)
            .copied()
            .ok_or_else(|| StoreError::NotFound(account_id.into()))
    }

    fn usage_since(
        &self,
        account_id: Option<&str>,
        since_unix: i64,
    ) -> Result<Vec<UsageRow>, StoreError> {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut rows: Vec<UsageRow> = Vec::new();
        for (e, c, acct) in &s.usage {
            if e.at_unix < since_unix || account_id.is_some_and(|a| a != acct) {
                continue;
            }
            match rows
                .iter_mut()
                .find(|r| r.key_id == e.key_id && r.provider == e.provider && r.domain == e.domain)
            {
                Some(r) => {
                    r.bytes += e.bytes_up + e.bytes_down;
                    r.total_micro += c.total_micro;
                }
                None => rows.push(UsageRow {
                    key_id: e.key_id.clone(),
                    provider: e.provider.clone(),
                    domain: e.domain.clone(),
                    bytes: e.bytes_up + e.bytes_down,
                    total_micro: c.total_micro,
                }),
            }
        }
        rows.sort_by(|a, b| {
            (&a.key_id, &a.provider, &a.domain).cmp(&(&b.key_id, &b.provider, &b.domain))
        });
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> MemoryAccounts {
        let m = MemoryAccounts::new(0.05);
        m.add_key(
            MemoryKey {
                record: KeyRecord {
                    key_id: "key_a".into(),
                    account_id: "acct_1".into(),
                    name: "alice".into(),
                    kyc_tier: KycTier::Basic,
                    monthly_cap_micro: Some(2 * MICROS_PER_USD),
                    revoked: false,
                },
                secret: "s3cret".into(),
            },
            10 * MICROS_PER_USD,
        );
        m
    }

    fn ev(bytes: u64, at: i64) -> UsageEvent {
        UsageEvent {
            key_id: "key_a".into(),
            provider: "p".into(),
            domain: "d.com".into(),
            bytes_up: 0,
            bytes_down: bytes,
            provider_price_per_gb: 4.0,
            at_unix: at,
        }
    }

    #[test]
    fn pricing_adds_five_percent() {
        let c = price_usage(500_000_000, 4.0, 0.05);
        assert_eq!(
            c,
            Charge {
                provider_cost_micro: 2_000_000,
                fee_micro: 100_000,
                total_micro: 2_100_000
            }
        );
    }

    #[test]
    fn month_start_is_first_of_month_utc() {
        // 2026-11-25T12:00:00Z -> 2026-11-01T00:00:00Z
        assert_eq!(month_start_unix(1_795_608_000), 1_793_491_200);
    }

    #[test]
    fn auth_admit_debit_cap_and_idempotent_credit() {
        let m = store();
        assert!(m.authenticate("nope").unwrap().is_none());
        let k = m.authenticate("s3cret").unwrap().unwrap();
        assert_eq!(k.key_id, "key_a");
        let now = 1_795_608_000;
        assert_eq!(m.admit("key_a", now).unwrap(), Admission::Allowed);
        // 0.5 GB at $4 + 5% = $2.10, which reaches the $2 cap.
        assert_eq!(
            m.record_usage(&ev(500_000_000, now)).unwrap().total_micro,
            2_100_000
        );
        assert_eq!(m.balance("acct_1").unwrap(), 7_900_000);
        assert!(matches!(
            m.admit("key_a", now).unwrap(),
            Admission::Denied(Denial::SpendCapReached { .. })
        ));
        assert!(m.credit("acct_1", 1_000_000, "cs_1").unwrap());
        assert!(!m.credit("acct_1", 1_000_000, "cs_1").unwrap());
        assert_eq!(m.balance("acct_1").unwrap(), 8_900_000);
        let rows = m.usage_since(Some("acct_1"), 0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].bytes, 500_000_000);
    }
}
