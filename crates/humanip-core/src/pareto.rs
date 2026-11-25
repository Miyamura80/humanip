//! Offers, eligibility, the Pareto frontier and the default pick.
//!
//! The selector never picks a dominated offer. By default it takes the
//! cheapest frontier offer that clears the reliability floor; when none does,
//! it takes the most reliable one and says so (`meets_floor: false`).

use crate::policy::{ExitType, ResolvedRisk, SourcingTier};
use crate::TAKE_RATE;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One provider's offer for one target domain.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct Offer {
    pub provider: String,
    pub exit_type: ExitType,
    pub sourcing: SourcingTier,
    /// Provider list price, USD per GB, before the humanip fee.
    pub price_per_gb: f64,
    /// Median pool fraud score, 0-100 (IPQS / Scamalytics blend). Lower is better.
    pub fraud_score: u8,
    /// Longest sticky session the provider guarantees.
    pub max_sticky_minutes: u32,
    /// p50 connect latency, ms.
    pub latency_ms: u32,
    /// Share of requests that succeed on the target domain, 0-1.
    pub success_rate: f64,
}

impl Offer {
    /// List price including the humanip fee.
    pub fn billed_per_gb(&self) -> f64 {
        self.price_per_gb * (1.0 + TAKE_RATE)
    }

    /// What one *successful* GB costs once retries are counted.
    pub fn effective_cost(&self) -> f64 {
        if self.success_rate <= 0.0 {
            f64::INFINITY
        } else {
            self.billed_per_gb() / self.success_rate
        }
    }
}

/// An axis the frontier can be computed over.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Objective {
    Cost,
    Success,
    Fraud,
    Sticky,
    Latency,
}

impl Objective {
    pub const ALL: [Objective; 5] = [
        Objective::Success,
        Objective::Cost,
        Objective::Fraud,
        Objective::Sticky,
        Objective::Latency,
    ];

    fn higher_is_better(self) -> bool {
        matches!(self, Objective::Success | Objective::Sticky)
    }

    pub fn value(self, o: &Offer) -> f64 {
        match self {
            Objective::Cost => o.effective_cost(),
            Objective::Success => o.success_rate,
            Objective::Fraud => f64::from(o.fraud_score),
            Objective::Sticky => f64::from(o.max_sticky_minutes),
            Objective::Latency => f64::from(o.latency_ms),
        }
    }
}

/// `a` dominates `b`: no worse on every objective, strictly better on one.
pub fn dominates(a: &Offer, b: &Offer, objectives: &[Objective]) -> bool {
    let mut strictly = false;
    for &obj in objectives {
        let (va, vb) = (obj.value(a), obj.value(b));
        let (better, worse) = if obj.higher_is_better() {
            (va > vb, va < vb)
        } else {
            (va < vb, va > vb)
        };
        if worse {
            return false;
        }
        strictly |= better;
    }
    strictly
}

/// Indices of offers no other offer dominates on `objectives`.
pub fn pareto_set(offers: &[Offer], objectives: &[Objective]) -> Vec<usize> {
    (0..offers.len())
        .filter(|&i| {
            !offers
                .iter()
                .enumerate()
                .any(|(j, p)| j != i && dominates(p, &offers[i], objectives))
        })
        .collect()
}

/// The cost/reliability frontier, cheapest first.
pub fn cost_reliability_frontier(offers: &[Offer]) -> Vec<usize> {
    let mut idx = pareto_set(offers, &[Objective::Cost, Objective::Success]);
    idx.sort_by(|&a, &b| {
        offers[a]
            .effective_cost()
            .total_cmp(&offers[b].effective_cost())
    });
    idx
}

/// Why an offer was filtered out before selection.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum Ineligible {
    ExitType { exit_type: ExitType },
    Sourcing { sourcing: SourcingTier },
    Fraud { fraud_score: u8, max: u8 },
    StickyTooShort { max_sticky_minutes: u32, need: u32 },
}

/// Check an offer against resolved risk limits and a pin-length requirement.
pub fn check_eligible(
    o: &Offer,
    risk: &ResolvedRisk,
    need_sticky_minutes: u32,
) -> Result<(), Ineligible> {
    if !risk.allowed_exit_types.contains(&o.exit_type) {
        return Err(Ineligible::ExitType {
            exit_type: o.exit_type,
        });
    }
    if !risk.sourcing_tiers.contains(&o.sourcing) {
        return Err(Ineligible::Sourcing {
            sourcing: o.sourcing,
        });
    }
    if o.fraud_score > risk.max_fraud_score {
        return Err(Ineligible::Fraud {
            fraud_score: o.fraud_score,
            max: risk.max_fraud_score,
        });
    }
    let need = need_sticky_minutes.max(risk.min_sticky_minutes);
    if o.max_sticky_minutes < need {
        return Err(Ineligible::StickyTooShort {
            max_sticky_minutes: o.max_sticky_minutes,
            need,
        });
    }
    Ok(())
}

/// The selector's answer.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct Pick {
    /// Index into the offers slice that was passed in.
    pub index: usize,
    /// False when nothing clears the floor and the most reliable offer was taken.
    pub meets_floor: bool,
}

/// Cheapest frontier offer at or above `floor`; otherwise the most reliable.
pub fn default_pick(offers: &[Offer], floor: f64) -> Option<Pick> {
    let front = cost_reliability_frontier(offers);
    if let Some(&index) = front.iter().find(|&&i| offers[i].success_rate >= floor) {
        return Some(Pick {
            index,
            meets_floor: true,
        });
    }
    front.last().map(|&index| Pick {
        index,
        meets_floor: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{resolve_risk, RiskOverrides};

    fn offer(name: &str, price: f64, success: f64) -> Offer {
        Offer {
            provider: name.into(),
            exit_type: ExitType::Isp,
            sourcing: SourcingTier::Verified,
            price_per_gb: price,
            fraud_score: 10,
            max_sticky_minutes: 1440,
            latency_ms: 300,
            success_rate: success,
        }
    }

    #[test]
    fn effective_cost_counts_retries_and_fee() {
        let o = offer("a", 2.0, 0.5);
        assert!((o.effective_cost() - 4.2).abs() < 1e-9);
        assert!(offer("dead", 1.0, 0.0).effective_cost().is_infinite());
    }

    #[test]
    fn dominated_offer_is_off_the_frontier() {
        // b is pricier and less reliable than a: dominated.
        let offers = vec![
            offer("a", 2.0, 0.95),
            offer("b", 3.0, 0.90),
            offer("c", 5.0, 0.99),
        ];
        assert_eq!(cost_reliability_frontier(&offers), vec![0, 2]);
    }

    #[test]
    fn equal_offers_do_not_dominate_each_other() {
        let offers = vec![offer("a", 2.0, 0.9), offer("b", 2.0, 0.9)];
        assert_eq!(pareto_set(&offers, &Objective::ALL), vec![0, 1]);
    }

    #[test]
    fn more_objectives_keep_more_offers() {
        let mut fast = offer("fast", 6.0, 0.9);
        fast.latency_ms = 100;
        let offers = vec![offer("cheap", 2.0, 0.95), fast];
        assert_eq!(
            pareto_set(&offers, &[Objective::Cost, Objective::Success]),
            vec![0]
        );
        assert_eq!(pareto_set(&offers, &Objective::ALL), vec![0, 1]);
    }

    #[test]
    fn default_pick_is_cheapest_above_floor() {
        // Rayobyte-like is cheaper but below the floor; NetNut-like is picked.
        let offers = vec![
            offer("ray", 2.1, 0.93),
            offer("net", 3.4, 0.96),
            offer("iproxy", 8.9, 0.99),
        ];
        assert_eq!(
            default_pick(&offers, 0.95),
            Some(Pick {
                index: 1,
                meets_floor: true
            })
        );
        assert_eq!(
            default_pick(&offers, 0.90),
            Some(Pick {
                index: 0,
                meets_floor: true
            })
        );
    }

    #[test]
    fn default_pick_falls_back_to_most_reliable_and_says_so() {
        let offers = vec![offer("a", 2.0, 0.80), offer("b", 4.0, 0.90)];
        assert_eq!(
            default_pick(&offers, 0.95),
            Some(Pick {
                index: 1,
                meets_floor: false
            })
        );
        assert_eq!(default_pick(&[], 0.95), None);
    }

    #[test]
    fn eligibility_reports_the_first_failing_rule() {
        let risk = resolve_risk(20, &[ExitType::Isp], &RiskOverrides::default());
        let mut o = offer("a", 1.0, 0.9);
        assert_eq!(check_eligible(&o, &risk, 0), Ok(()));
        o.fraud_score = 60;
        assert_eq!(
            check_eligible(&o, &risk, 0),
            Err(Ineligible::Fraud {
                fraud_score: 60,
                max: 27
            })
        );
        o.fraud_score = 10;
        o.max_sticky_minutes = 30;
        assert!(matches!(
            check_eligible(&o, &risk, 0),
            Err(Ineligible::StickyTooShort { need: 1440, .. })
        ));
        o.exit_type = ExitType::Residential;
        assert!(matches!(
            check_eligible(&o, &risk, 0),
            Err(Ineligible::ExitType { .. })
        ));
    }
}
