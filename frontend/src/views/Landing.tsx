// Public landing page: one Orbital hero that sells humanip at a glance,
// three value cards, the pricing line and the sign-up / sign-in CTAs.
// The orbit runs on the bundled demo offers; no backend call is made.

import { useMemo } from "react";
import { Logo, Mark, type MarkName } from "../components/icons";
import { OrbitalFrontier } from "../components/orbit";
import { ExitMark } from "../components/visual";
import { eligibleOffers, OFFERS } from "../lib/mock";
import { defaultPick, RELIABILITY_FLOOR } from "../lib/pareto";
import { resolveRisk } from "../lib/policy";

const DEMO_DOMAIN = "linkedin.com";

const VALUE: { mark: MarkName; title: string; body: string }[] = [
	{
		mark: "robot",
		title: "Sticky sessions for agents",
		body: "One exit per run. Browser agents keep the same human IP from login to checkout.",
	},
	{
		mark: "shield-user",
		title: "Months-long pins for bridges",
		body: "Matrix and messaging bridges hold one mobile IP for months, not minutes.",
	},
	{
		mark: "shield-check",
		title: "Reliability-first routing",
		body: "Each exit is scored per domain. The cheapest one that actually works wins.",
	},
];

export function Landing() {
	const eligible = useMemo(
		() =>
			new Set(
				eligibleOffers(
					resolveRisk(20, ["mobile", "isp", "residential"], {
						minStickyMinutes: 0,
					}),
					0,
				),
			),
		[],
	);
	const pick = useMemo(
		() =>
			defaultPick(
				OFFERS.filter((o) => eligible.has(o)),
				DEMO_DOMAIN,
			),
		[eligible],
	);

	return (
		<div className="landing">
			<section className="landing-hero">
				<div className="landing-copy">
					<div className="eyebrow">Hosted proxy broker</div>
					<h1 className="landing-title">
						Real human exits
						<br />
						for agents and bridges.
					</h1>
					<p className="landing-lede">
						Point any SOCKS5 or HTTP client at humanip. Get residential, ISP or
						mobile IPs. Pay per GB.
					</p>
					<div className="row landing-exits">
						<ExitMark type="residential" size={20} />
						<ExitMark type="isp" size={20} />
						<ExitMark type="mobile" size={20} />
						<span className="faint">Residential · ISP · Mobile</span>
					</div>
					<div className="row landing-cta">
						<a href="#/signup" className="btn btn-primary">
							Create account
						</a>
						<a href="#/login" className="btn">
							Sign in
						</a>
					</div>
				</div>
				<div className="landing-orbit">
					<OrbitalFrontier
						offers={OFFERS}
						domain={DEMO_DOMAIN}
						eligible={eligible}
						pick={pick}
						auto={pick}
						floor={RELIABILITY_FLOOR}
						size={480}
					/>
					<span className="faint landing-orbit-cap">
						Exits for {DEMO_DOMAIN}: closer is more reliable, further round
						costs more. Demo data.
					</span>
				</div>
			</section>

			<section className="landing-values">
				{VALUE.map((v) => (
					<article key={v.title} className="glass value-card">
						<span className="value-mark">
							<Mark name={v.mark} size={22} />
						</span>
						<h2>{v.title}</h2>
						<p className="muted">{v.body}</p>
					</article>
				))}
			</section>

			<section className="glass price-line">
				<div>
					<div className="num-l">
						Provider cost <span className="accent">+ 5%</span>
					</div>
					<p className="muted" style={{ margin: "10px 0 0" }}>
						Prepaid credit, no subscription. Top up by card or USDC.
					</p>
				</div>
				<div className="row price-clients">
					<Logo name="claude-code" size={22} label="Claude Code" />
					<Logo name="codex" size={22} label="Codex" />
					<Logo name="cursor" size={22} label="Cursor" />
					<Logo name="beeper" size={22} label="Beeper" />
					<Logo name="apify" size={22} label="Apify" />
				</div>
				<a href="#/signup" className="btn btn-primary">
					Get started
				</a>
			</section>
		</div>
	);
}
