//! A small illustrative catalog for demos and tests. Numbers are made up and
//! match the console mock data. Production quotes come from the live
//! price/reliability database, which is a separate service.

use crate::pareto::Offer;
use crate::policy::{ExitType, SourcingTier};

const D: u32 = 24 * 60;

/// (provider, exit, sourcing, $/GB, fraud, max pin minutes, latency ms)
const ROWS: [(&str, ExitType, SourcingTier, f64, u8, u32, u32); 9] = [
    (
        "Bright Data",
        ExitType::Residential,
        SourcingTier::Verified,
        4.2,
        22,
        30,
        380,
    ),
    (
        "Oxylabs",
        ExitType::Residential,
        SourcingTier::Verified,
        4.0,
        25,
        30,
        410,
    ),
    (
        "Decodo",
        ExitType::Residential,
        SourcingTier::Standard,
        2.6,
        34,
        30,
        520,
    ),
    (
        "IPRoyal",
        ExitType::Residential,
        SourcingTier::Standard,
        1.75,
        41,
        7 * D,
        640,
    ),
    (
        "PoolX",
        ExitType::Residential,
        SourcingTier::Unverified,
        0.7,
        63,
        10,
        1100,
    ),
    (
        "NetNut",
        ExitType::Isp,
        SourcingTier::Verified,
        3.4,
        12,
        90 * D,
        210,
    ),
    (
        "Rayobyte",
        ExitType::Isp,
        SourcingTier::Verified,
        2.1,
        18,
        90 * D,
        260,
    ),
    (
        "Proxy-Cheap",
        ExitType::Mobile,
        SourcingTier::Standard,
        6.5,
        9,
        D,
        720,
    ),
    (
        "iProxy farm",
        ExitType::Mobile,
        SourcingTier::Verified,
        8.9,
        5,
        90 * D,
        690,
    ),
];

/// Success rates observed per domain. Unknown domains get a neutral 0.8.
fn success(domain: &str, provider: &str) -> f64 {
    let table: &[(&str, f64)] = match domain {
        "web.whatsapp.com" => &[
            ("Bright Data", 0.84),
            ("Oxylabs", 0.82),
            ("Decodo", 0.77),
            ("IPRoyal", 0.71),
            ("PoolX", 0.22),
            ("NetNut", 0.96),
            ("Rayobyte", 0.93),
            ("Proxy-Cheap", 0.97),
            ("iProxy farm", 0.99),
        ],
        "linkedin.com" => &[
            ("Bright Data", 0.91),
            ("Oxylabs", 0.89),
            ("Decodo", 0.82),
            ("IPRoyal", 0.74),
            ("PoolX", 0.31),
            ("NetNut", 0.94),
            ("Rayobyte", 0.88),
            ("Proxy-Cheap", 0.97),
            ("iProxy farm", 0.98),
        ],
        _ => &[],
    };
    table
        .iter()
        .find(|(p, _)| *p == provider)
        .map_or(0.8, |(_, s)| *s)
}

/// The sample catalog priced for one target domain.
pub fn offers_for(domain: &str) -> Vec<Offer> {
    ROWS.iter()
        .map(
            |&(
                provider,
                exit_type,
                sourcing,
                price_per_gb,
                fraud_score,
                max_sticky_minutes,
                latency_ms,
            )| Offer {
                provider: provider.into(),
                exit_type,
                sourcing,
                price_per_gb,
                fraud_score,
                max_sticky_minutes,
                latency_ms,
                success_rate: success(domain, provider),
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pareto::{check_eligible, default_pick};
    use crate::policy::{parse_routes_yaml, DEFAULT_RELIABILITY_FLOOR};

    /// The story the console tells: for the WhatsApp bridge, Rayobyte is
    /// cheaper but below the floor, so NetNut is the default pick.
    #[test]
    fn whatsapp_bridge_picks_netnut() {
        let file = parse_routes_yaml(include_str!("../../../examples/routes.yaml")).unwrap();
        let mut route = file
            .routes
            .into_iter()
            .find(|r| r.name == "mautrix-whatsapp")
            .unwrap();
        route.exit_types = vec![ExitType::Mobile, ExitType::Isp];
        route.risk = 20;
        let risk = route.resolved_risk();
        let pool: Vec<Offer> = offers_for("web.whatsapp.com")
            .into_iter()
            .filter(|o| check_eligible(o, &risk, route.required_sticky_minutes()).is_ok())
            .collect();
        let names: Vec<&str> = pool.iter().map(|o| o.provider.as_str()).collect();
        assert_eq!(names, vec!["NetNut", "Rayobyte", "iProxy farm"]);
        let pick = default_pick(&pool, DEFAULT_RELIABILITY_FLOOR).unwrap();
        assert_eq!(pool[pick.index].provider, "NetNut");
        assert!(pick.meets_floor);
    }
}
