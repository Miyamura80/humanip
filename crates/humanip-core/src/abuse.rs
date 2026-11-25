//! Abuse controls contract: domain rules that require a minimum KYC tier.
//! The matching logic (`check`) is implemented by the abuse workstream.

use crate::accounts::KycTier;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Requests to domains matching `pattern` need at least `min_kyc`.
/// `pattern` is a domain suffix on a label boundary: "chase.com" matches
/// "chase.com" and "www.chase.com" but not "notchase.com"; ".gov" matches any
/// host ending in ".gov".
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DomainRule {
    pub pattern: String,
    /// Grouping for reporting, e.g. "banking", "government".
    pub category: String,
    /// `verified` blocks everyone but verified accounts; there is no tier
    /// above it, so use `verified` for "always require review".
    pub min_kyc: KycTier,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct Blocked {
    pub category: String,
    pub pattern: String,
    pub required: KycTier,
}

/// Is `domain` allowed for an account at `tier`? Stub: allows everything
/// until the abuse workstream implements matching.
pub fn check(_rules: &[DomainRule], _domain: &str, _tier: KycTier) -> Result<(), Blocked> {
    Ok(())
}
