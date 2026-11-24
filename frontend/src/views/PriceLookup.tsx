import { useMemo, useState } from "react";
import { EXIT_ICON, Ring } from "../components/visual";
import {
	effectivePrice,
	eligibleOffers,
	OFFERS,
	type ProviderOffer,
	successRate,
} from "../lib/mock";
import { resolveRisk, riskLabel, usd, withTake } from "../lib/policy";

const domainOf = (url: string) => {
	try {
		return new URL(
			url.includes("://") ? url : `https://${url}`,
		).hostname.replace(/^www\./, "");
	} catch {
		return url;
	}
};

const MEDAL = ["🥇", "🥈", "🥉"];

export function PriceLookup() {
	const [url, setUrl] = useState("https://www.linkedin.com/in/");
	const [risk, setRisk] = useState(35);
	const domain = domainOf(url);

	const resolved = useMemo(
		() => resolveRisk(risk, ["mobile", "isp", "residential"]),
		[risk],
	);
	const eligible = useMemo(
		() => new Set(eligibleOffers(resolved, 0)),
		[resolved],
	);
	const ranked = useMemo(
		() =>
			[...OFFERS].sort((a, b) => {
				const ea = eligible.has(a) ? 0 : 1;
				const eb = eligible.has(b) ? 0 : 1;
				return ea - eb || effectivePrice(a, domain) - effectivePrice(b, domain);
			}),
		[eligible, domain],
	);
	const top = ranked.filter((o) => eligible.has(o)).slice(0, 3);
	const best = top[0];
	// Podium order: 2nd, 1st, 3rd.
	const podium = [top[1], top[0], top[2]].map((o, i) => ({
		o,
		place: [1, 0, 2][i],
	}));

	return (
		<>
			<div className="page-head">
				<h1>💲 Price lookup</h1>
			</div>

			<div className="card card-pad row" style={{ gap: 16, marginBottom: 16 }}>
				<span style={{ fontSize: 20 }}>🎯</span>
				<input
					className="input mono"
					style={{ flex: 1 }}
					value={url}
					aria-label="Target URL"
					onChange={(e) => setUrl(e.target.value)}
				/>
				<span title="risk">🛡️</span>
				<input
					type="range"
					className="risk-slider"
					aria-label="Risk"
					style={{ width: 200, margin: 0 }}
					min={0}
					max={100}
					value={risk}
					onChange={(e) => setRisk(Number(e.target.value))}
				/>
				<span>💸</span>
				<span
					className="badge"
					style={{ minWidth: 96, justifyContent: "center" }}
				>
					{risk} · {riskLabel(risk)}
				</span>
			</div>

			<div
				className="split"
				style={{ gridTemplateColumns: "5fr 6fr", marginBottom: 16 }}
			>
				<div className="podium">
					{podium.map(({ o, place }) =>
						o ? (
							<div
								key={o.provider}
								className={`step${place === 0 ? " first" : ""}`}
							>
								<div className="medal">{MEDAL[place]}</div>
								<div className="big-icon" style={{ margin: "6px 0" }}>
									{EXIT_ICON[o.exitType]}
								</div>
								<b>{o.provider}</b>
								<div
									className={place === 0 ? "big-cost" : "money"}
									style={{ marginTop: 6 }}
								>
									{usd(withTake(effectivePrice(o, domain)))}
								</div>
								<div
									className="faint"
									style={{ fontSize: 11, marginBottom: 10 }}
								>
									per working GB
								</div>
								<div
									className="row"
									style={{ justifyContent: "center", gap: 10 }}
								>
									<Ring
										value={Math.round(successRate(domain, o.provider) * 100)}
										label="works"
										size={44}
									/>
									<Ring value={o.fraudScore} label="fraud" invert size={44} />
								</div>
							</div>
						) : (
							<div key={place} />
						),
					)}
				</div>
				<div className="card card-pad">
					<div className="legend" style={{ marginBottom: 6 }}>
						<span>↑ works more</span>
						<span>→ costs more</span>
						<span style={{ marginLeft: "auto" }}>
							📱 🏢 🏠 · faded = over budget
						</span>
					</div>
					<Scatter
						offers={ranked}
						eligible={eligible}
						domain={domain}
						best={best}
					/>
				</div>
			</div>

			<details className="dig card card-pad">
				<summary>
					📋 all {OFFERS.length} providers · {OFFERS.length - eligible.size}{" "}
					over your risk budget
				</summary>
				<table className="tbl dig-body">
					<thead>
						<tr>
							<th />
							<th>Provider</th>
							<th className="num">List</th>
							<th className="num">Works</th>
							<th className="num">Per working GB</th>
							<th>🛡️</th>
							<th>Fits?</th>
						</tr>
					</thead>
					<tbody>
						{ranked.map((o) => {
							const ok = eligible.has(o);
							return (
								<tr
									key={o.provider}
									className={o === best ? "hl" : ok ? "" : "dim"}
								>
									<td>{EXIT_ICON[o.exitType]}</td>
									<td style={{ fontWeight: 600 }}>{o.provider}</td>
									<td className="num mono">{usd(withTake(o.pricePerGb))}</td>
									<td className="num mono">
										{Math.round(successRate(domain, o.provider) * 100)}%
									</td>
									<td className="num mono">
										{usd(withTake(effectivePrice(o, domain)))}
									</td>
									<td className="mono">{o.fraudScore}</td>
									<td>
										{ok ? (
											"✅"
										) : (
											<span className="faint">✖ {whyNot(o, resolved)}</span>
										)}
									</td>
								</tr>
							);
						})}
					</tbody>
				</table>
				<p className="faint">
					Per working GB = list price ÷ success rate. Includes the 5% fee.
				</p>
			</details>
		</>
	);
}

function whyNot(o: ProviderOffer, r: ReturnType<typeof resolveRisk>): string {
	if (o.fraudScore > r.maxFraudScore) return `fraud > ${r.maxFraudScore}`;
	if (!r.sourcingTiers.includes(o.sourcing)) return `${o.sourcing} sourcing`;
	return "exit type";
}

function Scatter({
	offers,
	eligible,
	domain,
	best,
}: {
	offers: ProviderOffer[];
	eligible: Set<ProviderOffer>;
	domain: string;
	best?: ProviderOffer;
}) {
	const W = 560;
	const H = 220;
	const pad = { l: 40, r: 16, t: 10, b: 28 };
	const maxP = 10;
	const x = (p: number) => pad.l + (p / maxP) * (W - pad.l - pad.r);
	const y = (s: number) => pad.t + (1 - (s - 0.2) / 0.8) * (H - pad.t - pad.b);
	return (
		<svg
			viewBox={`0 0 ${W} ${H}`}
			className="scatter"
			role="img"
			aria-label="Cost versus success scatter"
		>
			{[0.2, 0.4, 0.6, 0.8, 1].map((s) => (
				<g key={s}>
					<line
						x1={pad.l}
						x2={W - pad.r}
						y1={y(s)}
						y2={y(s)}
						stroke="var(--border)"
					/>
					<text
						x={pad.l - 6}
						y={y(s) + 4}
						textAnchor="end"
						fontSize="10"
						fill="var(--text-3)"
					>
						{Math.round(s * 100)}%
					</text>
				</g>
			))}
			{[0, 2, 4, 6, 8, 10].map((p) => (
				<text
					key={p}
					x={x(p)}
					y={H - 8}
					textAnchor="middle"
					fontSize="10"
					fill="var(--text-3)"
				>
					${p}
				</text>
			))}
			{placeLabels(
				offers.map((o) => ({
					o,
					cx: x(withTake(o.pricePerGb)),
					cy: y(successRate(domain, o.provider)),
				})),
			).map(({ o, cx, cy, ly }) => (
				<g key={o.provider} opacity={eligible.has(o) ? 1 : 0.3}>
					{o === best && (
						<circle
							cx={cx}
							cy={cy}
							r={13}
							fill="var(--accent-dim)"
							stroke="var(--accent)"
							strokeWidth="2"
						/>
					)}
					<text x={cx} y={cy + 5} fontSize="14" textAnchor="middle">
						{EXIT_ICON[o.exitType]}
					</text>
					<text
						x={cx > W - 110 ? cx - 15 : cx + 15}
						y={ly + 4}
						textAnchor={cx > W - 110 ? "end" : "start"}
						fontSize="10.5"
						fill="var(--text-2)"
					>
						{o.provider}
					</text>
				</g>
			))}
		</svg>
	);
}

/** Greedy vertical nudge so neighbouring point labels never overlap. */
function placeLabels<T extends { cx: number; cy: number }>(pts: T[]) {
	const placed: (T & { ly: number })[] = [];
	for (const p of [...pts].sort((a, b) => a.cy - b.cy)) {
		let ly = p.cy;
		for (const q of placed) {
			if (Math.abs(q.cx - p.cx) < 80 && Math.abs(q.ly - ly) < 13)
				ly = q.ly + 13;
		}
		placed.push({ ...p, ly });
	}
	return placed;
}
