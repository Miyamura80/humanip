// Wire types for the two backends the console reads. Field names stay in
// snake_case so they match the JSON byte for byte.

import type { ExitType, PinLoss, RouteMode, SourcingTier } from "../lib/policy";

/** Every client call resolves to this; it never throws. */
export type Result<T> = { ok: true; data: T } | { ok: false; reason: string };

// ─── appctl serve: `quote` ────────────────────────────────────────────────

export interface QuoteInput {
	url: string;
	/** Integer 0-100 (u8 on the server). */
	risk?: number;
	reliability_floor?: number;
	exit_types?: ExitType[];
	sticky_minutes?: number;
}

/** Why an offer is out of budget (`#[serde(tag = "reason")]`). */
type Ineligible =
	| { reason: "exit_type"; exit_type: ExitType }
	| { reason: "sourcing"; sourcing: SourcingTier }
	| { reason: "fraud"; fraud_score: number; max: number }
	| { reason: "sticky_too_short"; max_sticky_minutes: number; need: number };

interface QuotedOffer {
	provider: string;
	exit_type: ExitType;
	success_rate: number;
	fraud_score: number;
	/** Price incl. the 5% fee, per GB. */
	billed_per_gb: number;
	/** Price incl. fee, per successful GB. */
	effective_cost: number;
	on_frontier: boolean;
	ineligible: Ineligible | null;
}

export interface QuoteOutput {
	domain: string;
	risk: number;
	reliability_floor: number;
	pick: string | null;
	meets_floor: boolean;
	frontier: string[];
	offers: QuotedOffer[];
}

// ─── appctl serve: `validate_routes` ──────────────────────────────────────

export interface ValidatedRoute {
	name: string;
	sticky_minutes: number;
	limits: {
		max_fraud_score: number;
		allowed_exit_types: ExitType[];
		sourcing_tiers: SourcingTier[];
		min_sticky_minutes: number;
	};
}

export interface ValidateRoutesOutput {
	routes: ValidatedRoute[];
}

// ─── Gateway admin API ────────────────────────────────────────────────────

export interface GatewayProvider {
	name: string;
	exit_type: ExitType;
	sourcing: SourcingTier;
	price_per_gb: number;
	fraud_score: number;
	countries: string[];
	/** Set when the adapter has not been checked against the real provider. */
	unverified_adapter: string | null;
}

/** `StickyTtl`: externally tagged enum. */
type StickyTtl = "session" | "forever" | { hours: number } | { days: number };

export interface GatewayRoute {
	name: string;
	mode: RouteMode;
	sticky: { ttl: StickyTtl; on_pin_loss: PinLoss } | null;
	exit_types: ExitType[];
	geo: {
		country: string | null;
		city: string | null;
		asn: number | null;
		match_account: boolean;
	};
	risk: number;
	reliability_floor: number;
	target_url?: string | null;
	spend_cap_usd?: number | null;
}

interface HealthRow {
	provider: string;
	domain: string;
	success_rate: number;
	samples: number;
}

export interface GatewayStatus {
	version: string;
	uptime_secs: number;
	sessions: number;
	providers: GatewayProvider[];
	routes: GatewayRoute[];
	health: HealthRow[];
}

export interface UsageRow {
	key_id: string;
	provider: string;
	domain: string;
	bytes: number;
	/** Micro-USD. */
	total_micro: number;
}
