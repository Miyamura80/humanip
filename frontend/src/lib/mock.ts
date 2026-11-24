// Mock data for the console screens. Replaced by `/api/v1` calls once the
// router's route and quote commands exist.

import type {
	ExitType,
	PinLoss,
	ResolvedRisk,
	RouteMode,
	SourcingTier,
} from "./policy";

export interface ProviderOffer {
	provider: string;
	exitType: ExitType;
	sourcing: SourcingTier;
	pricePerGb: number;
	/** Median fraud score of the pool (IPQS / Scamalytics blend). */
	fraudScore: number;
	maxStickyMinutes: number;
}

export const OFFERS: ProviderOffer[] = [
	{
		provider: "Bright Data",
		exitType: "residential",
		sourcing: "verified",
		pricePerGb: 4.2,
		fraudScore: 22,
		maxStickyMinutes: 30,
	},
	{
		provider: "Oxylabs",
		exitType: "residential",
		sourcing: "verified",
		pricePerGb: 4.0,
		fraudScore: 25,
		maxStickyMinutes: 30,
	},
	{
		provider: "Decodo",
		exitType: "residential",
		sourcing: "standard",
		pricePerGb: 2.6,
		fraudScore: 34,
		maxStickyMinutes: 30,
	},
	{
		provider: "IPRoyal",
		exitType: "residential",
		sourcing: "standard",
		pricePerGb: 1.75,
		fraudScore: 41,
		maxStickyMinutes: 7 * 24 * 60,
	},
	{
		provider: "PoolX",
		exitType: "residential",
		sourcing: "unverified",
		pricePerGb: 0.7,
		fraudScore: 63,
		maxStickyMinutes: 10,
	},
	{
		provider: "NetNut",
		exitType: "isp",
		sourcing: "verified",
		pricePerGb: 3.4,
		fraudScore: 12,
		maxStickyMinutes: 90 * 24 * 60,
	},
	{
		provider: "Rayobyte",
		exitType: "isp",
		sourcing: "verified",
		pricePerGb: 2.1,
		fraudScore: 18,
		maxStickyMinutes: 90 * 24 * 60,
	},
	{
		provider: "Proxy-Cheap",
		exitType: "mobile",
		sourcing: "standard",
		pricePerGb: 6.5,
		fraudScore: 9,
		maxStickyMinutes: 24 * 60,
	},
	{
		provider: "iProxy farm",
		exitType: "mobile",
		sourcing: "verified",
		pricePerGb: 8.9,
		fraudScore: 5,
		maxStickyMinutes: 90 * 24 * 60,
	},
];

/** Historical success rate per (domain, provider) from passive scoring. */
const SUCCESS: Record<string, Record<string, number>> = {
	"linkedin.com": {
		"Bright Data": 0.91,
		Oxylabs: 0.89,
		Decodo: 0.82,
		IPRoyal: 0.74,
		PoolX: 0.31,
		NetNut: 0.94,
		Rayobyte: 0.88,
		"Proxy-Cheap": 0.97,
		"iProxy farm": 0.98,
	},
};

export function successRate(domain: string, provider: string): number {
	const table = SUCCESS[domain];
	if (table?.[provider] !== undefined) return table[provider];
	// Deterministic fallback so unknown domains still render stable numbers.
	let h = 0;
	for (const c of domain + provider) h = (h * 31 + c.charCodeAt(0)) % 997;
	return 0.6 + (h % 38) / 100;
}

export function eligibleOffers(
	risk: ResolvedRisk,
	stickyMinutes: number,
): ProviderOffer[] {
	const needSticky = Math.max(stickyMinutes, risk.minStickyMinutes);
	return OFFERS.filter(
		(o) =>
			risk.allowedExitTypes.includes(o.exitType) &&
			risk.sourcingTiers.includes(o.sourcing) &&
			o.fraudScore <= risk.maxFraudScore &&
			o.maxStickyMinutes >= needSticky,
	);
}

/** Expected cost per *successful* GB: price adjusted for retries. */
export const effectivePrice = (o: ProviderOffer, domain: string) =>
	o.pricePerGb / successRate(domain, o.provider);

export interface RouteRow {
	id: string;
	name: string;
	useCase: "browser" | "mautrix" | "scrape";
	mode: RouteMode;
	pinLoss?: PinLoss;
	exitType: ExitType;
	provider: string;
	exitIp: string;
	geo: string;
	asn: string;
	health: number;
	fraudScore: number;
	sessions: number;
	pinnedFor?: string;
	gb30d: number;
	spend30d: number;
	risk: number;
	status: "healthy" | "degraded" | "pin_lost";
	spark: number[];
}

export const ROUTES: RouteRow[] = [
	{
		id: "rt_wa_bridge",
		name: "mautrix-whatsapp",
		useCase: "mautrix",
		mode: "sticky",
		pinLoss: "fail_closed",
		exitType: "mobile",
		provider: "iProxy farm",
		exitIp: "172.58.201.44",
		geo: "US · Brooklyn, NY",
		asn: "AS21928 T-Mobile USA",
		health: 98,
		fraudScore: 4,
		sessions: 1,
		pinnedFor: "41d 6h",
		gb30d: 0.82,
		spend30d: 7.66,
		risk: 5,
		status: "healthy",
		spark: [97, 98, 98, 99, 98, 97, 98, 98, 99, 98, 98, 98],
	},
	{
		id: "rt_signal_bridge",
		name: "mautrix-signal",
		useCase: "mautrix",
		mode: "sticky",
		pinLoss: "same_asn",
		exitType: "isp",
		provider: "NetNut",
		exitIp: "98.142.17.203",
		geo: "US · Austin, TX",
		asn: "AS7018 AT&T",
		health: 95,
		fraudScore: 11,
		sessions: 1,
		pinnedFor: "12d 2h",
		gb30d: 0.31,
		spend30d: 1.11,
		risk: 10,
		status: "healthy",
		spark: [93, 94, 96, 95, 95, 94, 96, 95, 95, 96, 95, 95],
	},
	{
		id: "rt_tg_bridge",
		name: "mautrix-telegram",
		useCase: "mautrix",
		mode: "sticky",
		pinLoss: "fail_closed",
		exitType: "isp",
		provider: "Rayobyte",
		exitIp: "no exit",
		geo: "DE · Berlin",
		asn: "AS3320 Deutsche Telekom",
		health: 0,
		fraudScore: 0,
		sessions: 0,
		pinnedFor: "lost 14m ago",
		gb30d: 0.12,
		spend30d: 0.26,
		risk: 10,
		status: "pin_lost",
		spark: [96, 95, 96, 94, 95, 96, 95, 92, 80, 41, 0, 0],
	},
	{
		id: "rt_agent_browse",
		name: "agent-browser",
		useCase: "browser",
		mode: "sticky",
		pinLoss: "same_city",
		exitType: "residential",
		provider: "Decodo",
		exitIp: "pool · 38 live",
		geo: "US · any",
		asn: "mixed",
		health: 87,
		fraudScore: 29,
		sessions: 38,
		gb30d: 46.2,
		spend30d: 126.12,
		risk: 45,
		status: "healthy",
		spark: [88, 86, 85, 87, 89, 86, 84, 88, 87, 86, 88, 87],
	},
	{
		id: "rt_price_scrape",
		name: "price-scrape",
		useCase: "scrape",
		mode: "rotating",
		exitType: "residential",
		provider: "IPRoyal",
		exitIp: "rotating · 1.2k/h",
		geo: "US, CA, GB",
		asn: "mixed",
		health: 71,
		fraudScore: 44,
		sessions: 212,
		gb30d: 318.4,
		spend30d: 585.06,
		risk: 70,
		status: "degraded",
		spark: [82, 80, 79, 77, 78, 74, 73, 75, 72, 70, 71, 71],
	},
];

export const ACCOUNT = {
	balance: 412.38,
	spend30d: ROUTES.reduce((s, r) => s + r.spend30d, 0),
	apiKey: "hip_live_7Qx2…Fv9c",
};
