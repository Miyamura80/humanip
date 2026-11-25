// One shared slice of product data, rendered differently by each system.

import type { MarkName } from "../../components/icons";
import {
	OFFERS,
	type ProviderOffer,
	ROUTES,
	type RouteRow,
	successRate,
} from "../../lib/mock";
import {
	axis,
	costReliabilityFrontier,
	defaultPick,
	RELIABILITY_FLOOR,
} from "../../lib/pareto";
import type { ExitType } from "../../lib/policy";

const DOMAIN = "web.whatsapp.com";
export const FLOOR = RELIABILITY_FLOOR;

export const routes: RouteRow[] = ROUTES.filter((r) =>
	[
		"rt_wa_bridge",
		"rt_tg_bridge",
		"rt_agent_browse",
		"rt_price_scrape",
	].includes(r.id),
);

export const EXIT_MARK: Record<ExitType, MarkName> = {
	mobile: "exit-mobile",
	isp: "exit-isp",
	residential: "exit-residential",
};

export const EXIT_SHORT: Record<ExitType, string> = {
	mobile: "MOB",
	isp: "ISP",
	residential: "RES",
};

export const HEALTH_MARK: Record<RouteRow["status"], MarkName> = {
	healthy: "robot",
	degraded: "glitch-robot",
	pin_lost: "broken-robot",
};

const cost = axis("cost");
export const costOf = (o: ProviderOffer) => cost.get(o, DOMAIN);
export const worksOf = (o: ProviderOffer) => successRate(DOMAIN, o.provider);
export const usd = (n: number) => `$${n.toFixed(2)}`;
export const pct = (n: number) => `${Math.round(n * 100)}%`;

export const offers = OFFERS;
export const frontier = costReliabilityFrontier(OFFERS, DOMAIN);
export const pick = defaultPick(OFFERS, DOMAIN) as ProviderOffer;

/** The frontier step just below the pick: what you would save by dropping reliability. */
export const stepBelow = frontier[frontier.indexOf(pick) - 1];
