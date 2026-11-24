import { useMemo, useState } from "react";
import {
	FrontierPlot,
	FrontierWalk,
	ParallelAxes,
	ReliabilityBadge,
} from "../components/frontier";
import { EXIT_ICON, Ring } from "../components/visual";
import { eligibleOffers, OFFERS, successRate } from "../lib/mock";
import { axis, defaultPick, RELIABILITY_FLOOR } from "../lib/pareto";
import { resolveRisk, riskLabel } from "../lib/policy";

const domainOf = (url: string) => {
	try {
		return new URL(
			url.includes("://") ? url : `https://${url}`,
		).hostname.replace(/^www\./, "");
	} catch {
		return url;
	}
};

// Default risk is low: reliability-first unless changed under Advanced.
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
	const [manual, setManual] = useState<(typeof OFFERS)[number]>();
	const domain = domainOf(url);

	const eligible = useMemo(
		() =>
			new Set(
				eligibleOffers(resolveRisk(risk, ["mobile", "isp", "residential"]), 0),
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

	return (
		<>
			<div className="page-head">
				<h1>💲 Price lookup</h1>
			</div>

			<div className="card card-pad" style={{ marginBottom: 16 }}>
				<div className="row" style={{ gap: 12 }}>
					<span style={{ fontSize: 20 }}>🎯</span>
					<input
						className="input mono"
						style={{ flex: 1 }}
						value={url}
						aria-label="Target URL"
						onChange={(e) => setUrl(e.target.value)}
					/>
					<ReliabilityBadge
						edited={edited}
						risk={risk}
						floor={floor}
						achieved={pick ? successRate(domain, pick.provider) : undefined}
					/>
				</div>
				<details className="dig" style={{ marginTop: 12 }}>
					<summary>⚙️ Advanced: reliability floor · risk dial</summary>
					<div className="dig-body grid-2">
						<div className="field">
							<span className="field-label">
								🎯 reliability floor · {Math.round(floor * 100)}%
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
								🛡️ risk · {risk} {riskLabel(risk)}
							</span>
							<input
								type="range"
								className="risk-slider"
								style={{ margin: "6px 0" }}
								min={0}
								max={100}
								value={risk}
								aria-label="Risk"
								onChange={(e) => setRisk(Number(e.target.value))}
							/>
						</div>
					</div>
				</details>
			</div>

			<div className="hero">
				<div className="card card-pad">
					<div className="legend" style={{ marginBottom: 4 }}>
						<span>✅ works ↑</span>
						<span>💸 $/working GB ←</span>
						<span style={{ marginLeft: "auto" }}>
							━ frontier · ░ beaten · faded = over risk
						</span>
					</div>
					<FrontierPlot
						x={cost}
						y={axis("success")}
						offers={OFFERS}
						domain={domain}
						eligible={eligible}
						pick={pick}
						auto={auto}
						floor={floor}
						onPick={setManual}
					/>
				</div>
				{pick && (
					<div className="card card-pad" style={{ textAlign: "center" }}>
						<span className={`badge ${pick === auto ? "badge-good" : ""}`}>
							{pick === auto ? "⭐ default pick" : "👆 your pick"}
						</span>
						<div
							className="big-icon"
							style={{ fontSize: 40, margin: "14px 0 6px" }}
						>
							{EXIT_ICON[pick.exitType]}
						</div>
						<div style={{ fontSize: 16, fontWeight: 600 }}>{pick.provider}</div>
						<div className="big-cost" style={{ margin: "6px 0 2px" }}>
							{cost.fmt(cost.get(pick, domain))}
						</div>
						<div className="faint" style={{ fontSize: 11, marginBottom: 14 }}>
							per working GB · incl. 5%
						</div>
						<div className="row" style={{ justifyContent: "center", gap: 14 }}>
							<Ring
								value={Math.round(successRate(domain, pick.provider) * 100)}
								label="works"
							/>
							<Ring value={pick.fraudScore} label="fraud" invert />
						</div>
						{pick !== auto && (
							<button
								type="button"
								className="btn"
								style={{ marginTop: 14 }}
								onClick={() => setManual(undefined)}
							>
								↺ back to ⭐
							</button>
						)}
						<button
							type="button"
							className="btn btn-primary"
							style={{ width: "100%", justifyContent: "center", marginTop: 14 }}
						>
							Create route
						</button>
					</div>
				)}
			</div>

			<div className="card card-pad" style={{ marginBottom: 16 }}>
				<b>🚶 Walk the frontier</b>
				<FrontierWalk
					offers={pool}
					domain={domain}
					pick={pick}
					auto={auto}
					onPick={setManual}
				/>
			</div>

			<div className="card card-pad" style={{ marginBottom: 16 }}>
				<div className="legend">
					<b style={{ color: "var(--text)" }}>🧭 All 5 axes</b>
					<span>↑ better on every axis</span>
					<span>━ unbeaten · ┄ beaten on all 5</span>
					<span style={{ marginLeft: "auto" }}>⬜ = selected</span>
				</div>
				<ParallelAxes
					offers={OFFERS}
					domain={domain}
					eligible={eligible}
					pick={pick}
					onPick={setManual}
				/>
			</div>

			<div className="multiples">
				{MULTIPLES.map((m) => (
					<div key={`${m.x}-${m.y}`} className="card card-pad">
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
