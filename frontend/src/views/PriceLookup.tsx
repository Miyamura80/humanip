import { useMemo, useState } from "react";
import {
	FrontierPlot,
	FrontierWalk,
	ParallelAxes,
	ReliabilityBadge,
} from "../components/frontier";
import { Mark } from "../components/icons";
import { OrbitalFrontier } from "../components/orbit";
import { ExitMark, Ring } from "../components/visual";
import {
	eligibleOffers,
	OFFERS,
	type ProviderOffer,
	successRate,
} from "../lib/mock";
import { axis, defaultPick, RELIABILITY_FLOOR } from "../lib/pareto";
import {
	EXIT_LABEL,
	resolveRisk,
	riskLabel,
	usd,
	withTake,
} from "../lib/policy";
import LiquidMetal from "../vendor/LiquidMetal";

const domainOf = (url: string) => {
	try {
		return new URL(
			url.includes("://") ? url : `https://${url}`,
		).hostname.replace(/^www\./, "");
	} catch {
		return url;
	}
};

// Reliability first unless changed under Advanced.
const DEFAULT_RISK = 20;

const MULTIPLES = [
	{ x: "cost", y: "fraud" },
	{ x: "cost", y: "latency" },
	{ x: "cost", y: "sticky" },
	{ x: "fraud", y: "success" },
] as const;

export function PriceLookup() {
	const [url, setUrl] = useState("https://www.linkedin.com/in/");
	const [risk, setRisk] = useState(DEFAULT_RISK);
	const [floor, setFloor] = useState(RELIABILITY_FLOOR);
	const [manual, setManual] = useState<ProviderOffer>();
	const domain = domainOf(url);

	// A lookup has no pin requirement, so the dial's sticky guarantee is waived.
	const eligible = useMemo(
		() =>
			new Set(
				eligibleOffers(
					resolveRisk(risk, ["mobile", "isp", "residential"], {
						minStickyMinutes: 0,
					}),
					0,
				),
			),
		[risk],
	);
	const pool = useMemo(() => OFFERS.filter((o) => eligible.has(o)), [eligible]);
	const auto = useMemo(
		() => defaultPick(pool, domain, floor),
		[pool, domain, floor],
	);
	const pick = manual && eligible.has(manual) ? manual : auto;
	const edited = risk !== DEFAULT_RISK || floor !== RELIABILITY_FLOOR;
	const cost = axis("cost");
	const works = pick ? successRate(domain, pick.provider) : undefined;

	return (
		<>
			<div className="page-head">
				<div>
					<div className="eyebrow">Price lookup</div>
					<h1>What does a working GB cost here?</h1>
				</div>
			</div>

			<div className="glass search">
				<Mark name="tunnel" size={20} />
				<input
					className="search-input mono"
					value={url}
					aria-label="Target URL"
					onChange={(e) => setUrl(e.target.value)}
				/>
				<ReliabilityBadge
					edited={edited}
					risk={risk}
					floor={floor}
					achieved={works}
				/>
			</div>
			<details className="dig adv-row">
				<summary>Advanced: reliability floor and risk dial</summary>
				<div className="dig-body grid-2">
					<div className="field">
						<span className="field-label">
							Reliability floor · {Math.round(floor * 100)}%
						</span>
						<input
							type="range"
							min={70}
							max={99}
							value={Math.round(floor * 100)}
							aria-label="Reliability floor"
							onChange={(e) => setFloor(Number(e.target.value) / 100)}
						/>
					</div>
					<div className="field">
						<span className="field-label">
							Risk · {risk} {riskLabel(risk)}
						</span>
						<input
							type="range"
							className="risk-slider"
							min={0}
							max={100}
							value={risk}
							aria-label="Risk"
							onChange={(e) => setRisk(Number(e.target.value))}
						/>
					</div>
				</div>
			</details>

			<div className="hero">
				<OrbitalFrontier
					offers={OFFERS}
					domain={domain}
					eligible={eligible}
					pick={pick}
					auto={auto}
					floor={floor}
					onPick={setManual}
				/>
				<div className="hero-side">
					<div className="eyebrow">
						{pick === auto ? "Default pick" : "Your pick"} · {domain}
					</div>
					{pick ? (
						<LiquidMetal
							preset="chromatic"
							theme="dark"
							radius={24}
							ringWidth={2}
						>
							<div className="pick-card">
								<div className="row" style={{ gap: 12 }}>
									<ExitMark type={pick.exitType} size={26} />
									<div>
										<b className="pick-name">{pick.provider}</b>
										<div className="faint">{EXIT_LABEL[pick.exitType]}</div>
									</div>
								</div>
								<div className="num-xl">
									{cost.fmt(cost.get(pick, domain))}
									<small>per working GB</small>
								</div>
								<div className="row" style={{ gap: 18 }}>
									<Ring
										value={Math.round((works ?? 0) * 100)}
										label="works"
										size={60}
									/>
									<Ring
										value={pick.fraudScore}
										label="fraud"
										invert
										size={60}
									/>
								</div>
								<div className="faint">
									List {usd(withTake(pick.pricePerGb))}/GB incl. 5% fee, divided
									by success rate.
								</div>
								<div className="row" style={{ gap: 10 }}>
									<button type="button" className="btn btn-primary wide">
										Create route
									</button>
									{pick !== auto && (
										<button
											type="button"
											className="btn"
											onClick={() => setManual(undefined)}
										>
											Reset
										</button>
									)}
								</div>
							</div>
						</LiquidMetal>
					) : (
						<div className="glass step muted">
							No exit fits this risk budget. Raise risk under Advanced.
						</div>
					)}
					<p className="caption">
						Closer to the centre works more often. Further round the dial costs
						more. The bright arc is the Pareto frontier: nothing off it is both
						cheaper and more reliable.
					</p>
				</div>
			</div>

			<section className="glass step">
				<div className="section-head">
					<h2>Walk the frontier</h2>
					<span className="faint">Each step buys reliability with money.</span>
				</div>
				<FrontierWalk
					offers={pool}
					domain={domain}
					pick={pick}
					auto={auto}
					onPick={setManual}
				/>
			</section>

			<section className="glass step">
				<div className="section-head">
					<h2>All five axes</h2>
					<span className="faint">
						Up is better on every axis. Solid lines are not beaten on all five
						at once.
					</span>
				</div>
				<ParallelAxes
					offers={OFFERS}
					domain={domain}
					eligible={eligible}
					pick={pick}
					onPick={setManual}
				/>
			</section>

			<div className="multiples">
				{MULTIPLES.map((m) => (
					<div key={`${m.x}-${m.y}`} className="glass tight">
						<FrontierPlot
							small
							x={axis(m.x)}
							y={axis(m.y)}
							offers={OFFERS}
							domain={domain}
							eligible={eligible}
							pick={pick}
							auto={auto}
							onPick={setManual}
						/>
					</div>
				))}
			</div>
		</>
	);
}
