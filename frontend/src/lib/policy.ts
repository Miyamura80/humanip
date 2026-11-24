// Route policy model shared by every screen. Mirrors the Rust config schema
// (`crates/config` route policy) so the UI and the router agree on fields.

export type ExitType = "mobile" | "isp" | "residential";
export type SourcingTier = "verified" | "standard" | "unverified";
export type RouteMode = "sticky" | "rotating";
export type PinLoss =
	| "fail_closed"
	| "same_asn"
	| "same_city"
	| "same_country"
	| "any";

interface RiskOverrides {
	maxFraudScore?: number;
	allowedExitTypes?: ExitType[];
	sourcingTiers?: SourcingTier[];
	minStickyMinutes?: number;
}

/** Concrete limits a risk dial value resolves to. */
export interface ResolvedRisk {
	maxFraudScore: number;
	allowedExitTypes: ExitType[];
	sourcingTiers: SourcingTier[];
	/** Minimum sticky-session duration the provider must guarantee. */
	minStickyMinutes: number;
}

export const EXIT_LABEL: Record<ExitType, string> = {
	mobile: "Mobile / eSIM",
	isp: "ISP static",
	residential: "Residential",
};

export const PIN_LOSS_LABEL: Record<PinLoss, string> = {
	fail_closed: "Fail closed + alert",
	same_asn: "Failover: same ASN",
	same_city: "Failover: same city",
	same_country: "Failover: same country",
	any: "Failover: any match",
};

const clamp = (n: number, lo: number, hi: number) =>
	Math.min(hi, Math.max(lo, n));

/**
 * Map the 0-100 risk dial onto the four risk dimensions. Low risk means
 * clean IPs, premium exits, vetted sourcing and long sticky guarantees;
 * high risk trades each of those for lower cost. Overrides win per field.
 */
export function resolveRisk(
	risk: number,
	preferred: ExitType[],
	overrides: RiskOverrides = {},
): ResolvedRisk {
	const r = clamp(Math.round(risk), 0, 100);

	const maxFraudScore = Math.round(15 + r * 0.6);

	// Downgrade chain: higher risk allows cheaper exit classes after the
	// preferred ones. Never datacenter: that is the product's whole point.
	const chain: ExitType[] = ["mobile", "isp", "residential"];
	const downgradeSteps = r < 30 ? 0 : r < 60 ? 1 : 2;
	const allowed = new Set<ExitType>(preferred);
	const worstPreferred = Math.max(...preferred.map((t) => chain.indexOf(t)), 0);
	for (let i = 1; i <= downgradeSteps; i++) {
		const next = chain[worstPreferred + i];
		if (next) allowed.add(next);
	}

	const sourcingTiers: SourcingTier[] =
		r < 40
			? ["verified"]
			: r < 75
				? ["verified", "standard"]
				: ["verified", "standard", "unverified"];

	const minStickyMinutes = r < 30 ? 24 * 60 : r < 70 ? 30 : 0;

	return {
		maxFraudScore: overrides.maxFraudScore ?? maxFraudScore,
		allowedExitTypes:
			overrides.allowedExitTypes ?? chain.filter((t) => allowed.has(t)),
		sourcingTiers: overrides.sourcingTiers ?? sourcingTiers,
		minStickyMinutes: overrides.minStickyMinutes ?? minStickyMinutes,
	};
}

export function riskLabel(risk: number): string {
	if (risk < 20) return "Paranoid";
	if (risk < 45) return "Balanced";
	if (risk < 75) return "Cost-leaning";
	return "Cheapest";
}

export function formatSticky(minutes: number): string {
	if (minutes === 0) return "best effort";
	if (minutes >= 24 * 60) return `${Math.round(minutes / 60 / 24)}d+`;
	if (minutes >= 60) return `${Math.round(minutes / 60)}h+`;
	return `${minutes}m+`;
}

/** humanip bills provider cost plus a 5% take rate. */
const TAKE_RATE = 0.05;
export const withTake = (cost: number) => cost * (1 + TAKE_RATE);

export const usd = (n: number, digits = 2) =>
	`$${n.toLocaleString("en-US", { minimumFractionDigits: digits, maximumFractionDigits: digits })}`;
