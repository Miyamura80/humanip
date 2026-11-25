//! Glue between the gateway config and the [`AccountStore`] contract.
//!
//! ```text
//! api_keys (YAML, dev) --memory_from_config--> MemoryAccounts
//! accounts_db (prod)   --ledger crate (integration)--> SQLite store
//! username + password  --authenticate--> AuthedUser
//! ```

use crate::config::GatewayConfig;
use crate::metrics::ConnResult;
use crate::select::{parse_user, AuthError, AuthedUser};
use crate::socks5::Reply;
use humanip_core::abuse::{Blocked, DomainRule};
use humanip_core::accounts::{
    AccountStore, Admission, Denial, KeyRecord, KycTier, MemoryAccounts, MemoryKey, MicroUsd,
    StoreError, MICROS_PER_USD,
};
use humanip_core::TAKE_RATE;

/// Starting balance for dev keys without `balance_usd`.
pub const DEFAULT_DEV_BALANCE_USD: f64 = 100.0;

/// Whole USD to micro-USD, rounded to the nearest micro.
pub fn usd_to_micro(usd: f64) -> MicroUsd {
    (usd * MICROS_PER_USD as f64).round() as MicroUsd
}

/// In-memory store seeded from `api_keys`: one key and one account per entry
/// (`key_<name>`, `acct_<name>`), with the entry's tier, cap and balance.
pub fn memory_from_config(cfg: &GatewayConfig) -> MemoryAccounts {
    let store = MemoryAccounts::new(TAKE_RATE);
    for k in &cfg.api_keys {
        let record = KeyRecord {
            key_id: format!("key_{}", k.name),
            account_id: format!("acct_{}", k.name),
            name: k.name.clone(),
            kyc_tier: k.kyc_tier,
            monthly_cap_micro: k.monthly_cap_usd.map(usd_to_micro),
            revoked: false,
        };
        let balance = usd_to_micro(k.balance_usd.unwrap_or(DEFAULT_DEV_BALANCE_USD));
        store.add_key(
            MemoryKey {
                record,
                secret: k.key.clone(),
            },
            balance,
        );
    }
    store
}

/// Parse the username, then resolve the password (the API key secret) with
/// the store. Blocking: async callers run it under `spawn_blocking`.
pub fn authenticate(
    store: &dyn AccountStore,
    username: &str,
    password: &str,
) -> Result<AuthedUser, AuthError> {
    let user = parse_user(username)?;
    let key = store
        .authenticate(password)
        .map_err(|e| AuthError::Store(e.to_string()))?
        .ok_or(AuthError::BadKey)?;
    Ok(AuthedUser::new(key, user))
}

/// Why an authenticated connection is refused before dialing.
#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    /// `AccountStore::admit` said no.
    Denied(Denial),
    /// An abuse rule needs a higher KYC tier for this domain.
    Blocked(Blocked),
    /// The store could not answer; fail closed.
    Store(String),
}

impl Refusal {
    pub fn socks_reply(&self) -> Reply {
        match self {
            Refusal::Denied(_) | Refusal::Blocked(_) => Reply::NotAllowed,
            Refusal::Store(_) => Reply::GeneralFailure,
        }
    }

    pub fn http_status(&self) -> (u16, String) {
        match self {
            Refusal::Denied(Denial::Revoked) => (403, "Key Revoked".into()),
            Refusal::Denied(Denial::InsufficientBalance { .. })
            | Refusal::Denied(Denial::SpendCapReached { .. }) => (402, "Payment Required".into()),
            Refusal::Blocked(b) => (403, format!("Blocked: {}", b.category)),
            Refusal::Store(_) => (503, "Accounts Unavailable".into()),
        }
    }

    pub fn conn_result(&self) -> ConnResult {
        match self {
            Refusal::Blocked(_) => ConnResult::Blocked,
            Refusal::Denied(_) | Refusal::Store(_) => ConnResult::Denied,
        }
    }
}

/// The abuse rule check the gateway runs: `humanip_core::abuse::check` in
/// production, a fake in tests.
pub type AbuseCheck = fn(&[DomainRule], &str, KycTier) -> Result<(), Blocked>;

/// Map a store admission answer to a refusal. A store error fails closed.
pub fn admission(answer: Result<Admission, StoreError>) -> Result<(), Refusal> {
    match answer {
        Ok(Admission::Allowed) => Ok(()),
        Ok(Admission::Denied(d)) => Err(Refusal::Denied(d)),
        Err(e) => Err(Refusal::Store(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_yaml;

    #[test]
    fn seeds_one_key_per_entry() {
        let cfg = parse_yaml(
            r#"
allow_direct: true
api_keys:
  - { name: alice, key: ka, kyc_tier: verified, monthly_cap_usd: 2.5, balance_usd: 7 }
  - { name: bob, key: kb }
providers:
  - name: local
    exit_type: residential
    sourcing: verified
    price_per_gb: 1.0
    fraud_score: 5
    max_sticky_minutes: 10
    latency_ms: 10
    upstream: { kind: direct }
"#,
        )
        .unwrap();
        let m = memory_from_config(&cfg);
        let a = authenticate(&m, "user", "ka").unwrap();
        assert_eq!(a.key.key_id, "key_alice");
        assert_eq!(a.key_name, "key_alice");
        assert_eq!(a.key.account_id, "acct_alice");
        assert_eq!(a.key.kyc_tier, KycTier::Verified);
        assert_eq!(a.key.monthly_cap_micro, Some(2_500_000));
        assert_eq!(m.balance("acct_alice").unwrap(), 7_000_000);
        let b = authenticate(&m, "user", "kb").unwrap();
        assert_eq!(b.key.kyc_tier, KycTier::None);
        assert_eq!(b.key.monthly_cap_micro, None);
        assert_eq!(m.balance("acct_bob").unwrap(), 100_000_000);
        assert_eq!(m.admit("key_bob", 0).unwrap(), Admission::Allowed);
        assert!(matches!(
            authenticate(&m, "user", "nope"),
            Err(AuthError::BadKey)
        ));
    }
}
