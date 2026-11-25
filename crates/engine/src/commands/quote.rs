//! `quote` – rank exits for a target URL: eligibility under the risk budget,
//! the cost/reliability frontier, and the reliability-first default pick.

use crate::commands::{Command, CommandError};
use crate::context::Ctx;
use crate::register_command;
use async_trait::async_trait;
use humanip_core::pareto::{
    check_eligible, cost_reliability_frontier, default_pick, Ineligible, Offer,
};
use humanip_core::policy::{
    resolve_risk, ExitType, RiskOverrides, DEFAULT_RELIABILITY_FLOOR, DEFAULT_RISK,
};
use humanip_core::{domain_of, sample};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Default)]
pub struct Quote;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QuoteInput {
    /// Target URL or host, e.g. "https://web.whatsapp.com".
    pub url: String,
    /// 0-100. Defaults to 20 (reliability first).
    pub risk: Option<u8>,
    /// Success rate the default pick must clear. Defaults to 0.95.
    pub reliability_floor: Option<f64>,
    /// Preferred exit classes. Defaults to all three.
    pub exit_types: Option<Vec<ExitType>>,
    /// Pin length the caller needs, in minutes. 0 for rotating.
    #[serde(default)]
    pub sticky_minutes: u32,
    /// Offers to rank. Defaults to the built-in sample catalog.
    pub offers: Option<Vec<Offer>>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct QuotedOffer {
    pub provider: String,
    pub exit_type: ExitType,
    pub success_rate: f64,
    pub fraud_score: u8,
    /// Price incl. 5% fee, per GB.
    pub billed_per_gb: f64,
    /// Price incl. fee, per successful GB.
    pub effective_cost: f64,
    pub on_frontier: bool,
    pub ineligible: Option<Ineligible>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct QuoteOutput {
    pub domain: String,
    pub risk: u8,
    pub reliability_floor: f64,
    /// Provider name of the default pick, if any offer is eligible.
    pub pick: Option<String>,
    /// False when no eligible offer clears the floor.
    pub meets_floor: bool,
    /// Eligible frontier, cheapest first.
    pub frontier: Vec<String>,
    /// All offers, cheapest effective cost first.
    pub offers: Vec<QuotedOffer>,
}

#[async_trait]
impl Command for Quote {
    type Input = QuoteInput;
    type Output = QuoteOutput;

    fn name(&self) -> &'static str {
        "quote"
    }

    fn description(&self) -> &'static str {
        "Rank proxy exits for a URL: cheapest exit that clears the reliability floor within the risk budget."
    }

    async fn run(&self, input: QuoteInput, _cx: &Ctx<'_>) -> Result<QuoteOutput, CommandError> {
        let domain = domain_of(&input.url);
        if domain.is_empty() {
            return Err(CommandError::InvalidInput("url has no host".into()));
        }
        let risk = input.risk.unwrap_or(DEFAULT_RISK);
        if risk > 100 {
            return Err(CommandError::InvalidInput("risk must be 0-100".into()));
        }
        let floor = input.reliability_floor.unwrap_or(DEFAULT_RELIABILITY_FLOOR);
        if !(0.5..=1.0).contains(&floor) {
            return Err(CommandError::InvalidInput(
                "reliability_floor must be 0.5-1.0".into(),
            ));
        }
        let preferred = input.exit_types.unwrap_or_else(|| ExitType::ALL.to_vec());
        if preferred.is_empty() {
            return Err(CommandError::InvalidInput(
                "exit_types must not be empty".into(),
            ));
        }
        let mut offers = input.offers.unwrap_or_else(|| sample::offers_for(&domain));
        offers.sort_by(|a, b| a.effective_cost().total_cmp(&b.effective_cost()));

        // The sticky need is explicit here, so the dial's own guarantee is waived.
        let limits = resolve_risk(
            risk,
            &preferred,
            &RiskOverrides {
                min_sticky_minutes: Some(0),
                ..Default::default()
            },
        );
        let verdicts: Vec<Option<Ineligible>> = offers
            .iter()
            .map(|o| check_eligible(o, &limits, input.sticky_minutes).err())
            .collect();
        let pool: Vec<Offer> = offers
            .iter()
            .zip(&verdicts)
            .filter(|(_, v)| v.is_none())
            .map(|(o, _)| o.clone())
            .collect();
        let frontier: Vec<String> = cost_reliability_frontier(&pool)
            .into_iter()
            .map(|i| pool[i].provider.clone())
            .collect();
        let pick = default_pick(&pool, floor);

        Ok(QuoteOutput {
            domain,
            risk,
            reliability_floor: floor,
            pick: pick.as_ref().map(|p| pool[p.index].provider.clone()),
            meets_floor: pick.as_ref().is_some_and(|p| p.meets_floor),
            offers: offers
                .iter()
                .zip(verdicts)
                .map(|(o, ineligible)| QuotedOffer {
                    provider: o.provider.clone(),
                    exit_type: o.exit_type,
                    success_rate: o.success_rate,
                    fraud_score: o.fraud_score,
                    billed_per_gb: o.billed_per_gb(),
                    effective_cost: o.effective_cost(),
                    on_frontier: frontier.contains(&o.provider),
                    ineligible,
                })
                .collect(),
            frontier,
        })
    }
}

register_command!(Quote);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::AppContext;

    async fn quote(input: QuoteInput) -> Result<QuoteOutput, CommandError> {
        let app = AppContext::default();
        Quote.run(input, &Ctx::new(&app)).await
    }

    fn input(url: &str) -> QuoteInput {
        QuoteInput {
            url: url.into(),
            risk: None,
            reliability_floor: None,
            exit_types: Some(vec![ExitType::Mobile, ExitType::Isp]),
            sticky_minutes: 90 * 24 * 60,
            offers: None,
        }
    }

    #[tokio::test]
    async fn whatsapp_bridge_defaults_to_netnut() {
        let out = quote(input("https://web.whatsapp.com/")).await.unwrap();
        assert_eq!(out.domain, "web.whatsapp.com");
        assert_eq!(out.pick.as_deref(), Some("NetNut"));
        assert!(out.meets_floor);
        assert_eq!(out.frontier, vec!["Rayobyte", "NetNut", "iProxy farm"]);
        let poolx = out.offers.iter().find(|o| o.provider == "PoolX").unwrap();
        assert!(poolx.ineligible.is_some());
    }

    #[tokio::test]
    async fn lower_floor_takes_the_cheaper_exit() {
        let mut i = input("web.whatsapp.com");
        i.reliability_floor = Some(0.9);
        assert_eq!(quote(i).await.unwrap().pick.as_deref(), Some("Rayobyte"));
    }

    #[tokio::test]
    async fn rejects_bad_input() {
        let mut i = input("https://x.com");
        i.risk = Some(150);
        assert!(matches!(quote(i).await, Err(CommandError::InvalidInput(_))));
        assert!(matches!(
            quote(input("https:///")).await,
            Err(CommandError::InvalidInput(_))
        ));
    }
}
