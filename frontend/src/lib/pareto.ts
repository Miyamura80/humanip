// Multi-objective view of provider offers. The router's selector uses the
// same rules: never pick a dominated offer, and default to reliability.

import {
	effectivePrice,
	LATENCY_MS,
	type ProviderOffer,
	successRate,
} from "./mock";
import { withTake } from "./policy";

export interface Axis {
	key: "cost" | "success" | "fraud" | "sticky" | "latency";
	label: string;
	/** Which direction is better. */
	better: "low" | "high";
	get: (o: ProviderOffer, domain: string) => number;
	fmt: (n: number) => string;
}

export const AXES: Axis[] = [
	{
		key: "success",
		label: "works",
		better: "high",
		get: (o, d) => successRate(d, o.provider),
		fmt: (n) => `${Math.round(n * 100)}%`,
	},
	{
		key: "cost",
		label: "$/working GB",
		better: "low",
		get: (o, d) => withTake(effectivePrice(o, d)),
		fmt: (n) => `$${n.toFixed(2)}`,
	},
	{
		key: "fraud",
		label: "fraud",
		better: "low",
		get: (o) => o.fraudScore,
		fmt: (n) => String(Math.round(n)),
	},
	{
		key: "sticky",
		label: "max pin",
		better: "high",
		// log scale so 10m, 1d and 90d spread out evenly
		get: (o) => Math.log10(o.maxStickyMinutes),
		fmt: (n) => {
			const m = 10 ** n;
			return m >= 1440 ? `${Math.round(m / 1440)}d` : `${Math.round(m)}m`;
		},
	},
	{
		key: "latency",
		label: "latency",
		better: "low",
		get: (o) => LATENCY_MS[o.provider] ?? 999,
		fmt: (n) => `${Math.round(n)}ms`,
	},
];

export const axis = (k: Axis["key"]) => AXES.find((a) => a.key === k) as Axis;

/** a dominates b: no worse on every axis, strictly better on at least one. */
function dominates(
	a: ProviderOffer,
	b: ProviderOffer,
	axes: Axis[],
	domain: string,
): boolean {
	let strictly = false;
	for (const ax of axes) {
		const va = ax.get(a, domain);
		const vb = ax.get(b, domain);
		const better = ax.better === "high" ? va > vb : va < vb;
		const worse = ax.better === "high" ? va < vb : va > vb;
		if (worse) return false;
		if (better) strictly = true;
	}
	return strictly;
}

/** Offers that no other offer dominates on the given axes. */
export function paretoSet(
	offers: ProviderOffer[],
	axes: Axis[],
	domain: string,
): Set<ProviderOffer> {
	return new Set(
		offers.filter(
			(o) => !offers.some((p) => p !== o && dominates(p, o, axes, domain)),
		),
	);
}

/** 2D frontier on cost vs reliability, ordered cheapest first. */
export function costReliabilityFrontier(
	offers: ProviderOffer[],
	domain: string,
): ProviderOffer[] {
	const cost = axis("cost");
	const set = paretoSet(offers, [cost, axis("success")], domain);
	return [...set].sort((a, b) => cost.get(a, domain) - cost.get(b, domain));
}

/** Reliability floor used for the default pick. */
export const RELIABILITY_FLOOR = 0.95;

/**
 * Sensible default: the cheapest frontier offer that clears the reliability
 * floor; if none does, the most reliable one. Cost only breaks ties.
 */
export function defaultPick(
	offers: ProviderOffer[],
	domain: string,
	floor = RELIABILITY_FLOOR,
): ProviderOffer | undefined {
	const front = costReliabilityFrontier(offers, domain);
	return (
		front.find((o) => successRate(domain, o.provider) >= floor) ??
		front[front.length - 1]
	);
}
