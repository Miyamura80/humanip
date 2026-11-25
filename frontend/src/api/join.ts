// Map backend JSON onto the shapes the screens already draw. Anything the
// backend does not send is filled from the demo catalog and flagged.

import { OFFERS, type ProviderOffer, type RouteRow } from "../lib/mock";
import type {
	GatewayProvider,
	GatewayRoute,
	GatewayStatus,
	QuoteOutput,
	UsageRow,
} from "./types";

/** Hostname of a URL or bare host, without `www.`. */
export function hostOf(url: string): string {
	try {
		return new URL(
			url.includes("://") ? url : `https://${url}`,
		).hostname.replace(/^www\./, "");
	} catch {
		return url;
	}
}

// ─── quote -> ProviderOffer ───────────────────────────────────────────────

interface QuotedView {
	offers: ProviderOffer[];
	eligible: Set<ProviderOffer>;
	pick?: ProviderOffer;
}

/**
 * `quote` has no sourcing, max pin or latency, so those come from the demo
 * catalog by provider name and are listed in `estimated`.
 */
export function offersFromQuote(q: QuoteOutput): QuotedView {
	const eligible = new Set<ProviderOffer>();
	let pick: ProviderOffer | undefined;
	const offers = q.offers.map((w) => {
		const demo = OFFERS.find((o) => o.provider === w.provider);
		const o: ProviderOffer = {
			provider: w.provider,
			exitType: w.exit_type,
			sourcing: demo?.sourcing ?? "standard",
			// billed_per_gb includes the 5% fee; the UI adds it back itself.
			pricePerGb: w.billed_per_gb / 1.05,
			fraudScore: w.fraud_score,
			maxStickyMinutes: demo?.maxStickyMinutes ?? 30,
			successRate: w.success_rate,
			estimated: ["sourcing", "max pin", "latency"],
		};
		if (!w.ineligible) eligible.add(o);
		if (w.provider === q.pick) pick = o;
		return o;
	});
	return { offers, eligible, pick };
}

// ─── status + usage -> RouteRow ───────────────────────────────────────────

const isForever = (r: GatewayRoute) => {
	const ttl = r.sticky?.ttl;
	return ttl === "forever" || (typeof ttl === "object" && "days" in ttl);
};

function useCaseOf(r: GatewayRoute): RouteRow["useCase"] {
	if (r.mode === "rotating") return "scrape";
	return isForever(r) || r.exit_types.includes("mobile")
		? "mautrix"
		: "browser";
}

function ttlLabel(r: GatewayRoute): string {
	const ttl = r.sticky?.ttl;
	if (!ttl) return "rotating";
	if (typeof ttl === "string")
		return ttl === "forever" ? "hard pin" : "per session";
	return "hours" in ttl ? `${ttl.hours}h pin` : `${ttl.days}d pin`;
}

export interface LiveRoute extends RouteRow {
	/** False when no health samples exist for the route's target. */
	healthKnown: boolean;
	/** Short site name of the target, e.g. "whatsapp". */
	site?: string;
}

/** Pick the provider a route most likely exits through. */
function primaryProvider(
	r: GatewayRoute,
	s: GatewayStatus,
	target?: string,
): GatewayProvider | undefined {
	const country = r.geo.country;
	const fits = s.providers.filter(
		(p) =>
			r.exit_types.includes(p.exit_type) &&
			(!country || p.countries.length === 0 || p.countries.includes(country)),
	);
	const works = (p: GatewayProvider) =>
		s.health.find((h) => h.provider === p.name && h.domain === target)
			?.success_rate ?? -1;
	return [...fits].sort(
		(a, b) => works(b) - works(a) || a.price_per_gb - b.price_per_gb,
	)[0];
}

const micro = (rows: UsageRow[]) =>
	rows.reduce((n, u) => n + u.total_micro, 0) / 1e6;
const gb = (rows: UsageRow[]) => rows.reduce((n, u) => n + u.bytes, 0) / 1e9;

export function routesFromStatus(
	s: GatewayStatus,
	use: UsageRow[] | undefined,
): LiveRoute[] {
	return s.routes.map((r) => {
		const target = r.target_url ? hostOf(r.target_url) : undefined;
		const p = primaryProvider(r, s, target);
		const h = s.health.find(
			(x) => x.provider === p?.name && x.domain === target,
		);
		const health = h ? Math.round(h.success_rate * 100) : 0;
		const mine = (use ?? []).filter(
			(u) => u.provider === p?.name && (!target || u.domain === target),
		);
		const geo = [r.geo.country ?? "any", r.geo.city]
			.filter(Boolean)
			.join(" · ");
		return {
			id: `rt_${r.name}`,
			name: r.name,
			useCase: useCaseOf(r),
			mode: r.mode,
			pinLoss: r.sticky?.on_pin_loss,
			exitType: p?.exit_type ?? r.exit_types[0] ?? "residential",
			provider: p?.name ?? "no provider fits",
			exitIp: ttlLabel(r),
			geo,
			asn: r.geo.asn ? `AS${r.geo.asn}` : "any ASN",
			health,
			fraudScore: p?.fraud_score ?? 0,
			sessions: 0,
			gb30d: gb(mine),
			spend30d: micro(mine),
			risk: r.risk,
			status: h && health < 80 ? "degraded" : "healthy",
			spark: h ? Array(12).fill(health) : [],
			healthKnown: !!h,
			site: target?.split(".").slice(-2)[0],
		};
	});
}

export interface Totals {
	spend?: number;
	gb?: number;
	sessions: number;
	pins: number;
	medianFraud: number;
}

const median = (xs: number[]) => {
	if (!xs.length) return 0;
	const s = [...xs].sort((a, b) => a - b);
	return s[Math.floor(s.length / 2)];
};

export function totalsFrom(
	s: GatewayStatus,
	routes: LiveRoute[],
	use: UsageRow[] | undefined,
): Totals {
	return {
		spend: use ? micro(use) : undefined,
		gb: use ? gb(use) : undefined,
		sessions: s.sessions,
		pins: s.routes.filter(isForever).length,
		medianFraud: median(routes.map((r) => r.fraudScore)),
	};
}

/** Demo provider strip: the mock catalog, with one adapter left unverified. */
export const DEMO_PROVIDERS: GatewayProvider[] = OFFERS.map((o) => ({
	name: o.provider,
	exit_type: o.exitType,
	sourcing: o.sourcing,
	price_per_gb: o.pricePerGb,
	fraud_score: o.fraudScore,
	countries: [],
	unverified_adapter: o.provider === "Bright Data" ? "brightdata" : null,
}));
