import { useMemo, useState } from "react";
import { ExitBadge, Meter, PageHead } from "../components/ui";
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
	const best = ranked.find((o) => eligible.has(o));

	return (
		<>
			<PageHead
				title="Price lookup"
				sub="Cheapest exit that works on a site, by success rate on that domain, within your risk budget."
			/>

			<div className="card card-pad" style={{ marginBottom: 16 }}>
				<div className="row" style={{ gap: 16, alignItems: "flex-end" }}>
					<div className="field" style={{ flex: 1, margin: 0 }}>
						<label htmlFor="lurl">Target URL</label>
						<input
							id="lurl"
							className="input mono"
							value={url}
							onChange={(e) => setUrl(e.target.value)}
						/>
					</div>
					<div className="field" style={{ width: 280, margin: 0 }}>
						<span className="field-label">
							Risk {risk} · {riskLabel(risk)}
						</span>
						<input
							type="range"
							className="risk-slider"
							aria-label="Risk"
							style={{ margin: "8px 0" }}
							min={0}
							max={100}
							value={risk}
							onChange={(e) => setRisk(Number(e.target.value))}
						/>
					</div>
					<button type="button" className="btn btn-primary">
						Create route with best
					</button>
				</div>
			</div>

			<div
				className="split"
				style={{ marginBottom: 16, gridTemplateColumns: "1fr 2fr" }}
			>
				<div className="card card-pad">
					<div className="card-title">Recommendation for {domain}</div>
					{best ? (
						<>
							<div className="row" style={{ gap: 8, marginBottom: 6 }}>
								<span style={{ fontSize: 16, fontWeight: 600 }}>
									{best.provider}
								</span>
								<ExitBadge type={best.exitType} />
							</div>
							<div className="big-cost">
								{usd(withTake(effectivePrice(best, domain)))}
								<span
									className="muted"
									style={{ fontSize: 13, fontWeight: 400 }}
								>
									{" "}
									/ successful GB
								</span>
							</div>
							<div className="kv">
								<span className="muted">List price + 5%</span>
								<span className="mono">
									{usd(withTake(best.pricePerGb))}/GB
								</span>
							</div>
							<div className="kv">
								<span className="muted">Success on {domain} · 7d</span>
								<span className="mono">
									{Math.round(successRate(domain, best.provider) * 100)}%
								</span>
							</div>
							<div className="kv">
								<span className="muted">Samples</span>
								<span className="mono">48,211 requests</span>
							</div>
						</>
					) : (
						<div className="muted">
							No provider fits this risk budget. Raise risk or widen exit types.
						</div>
					)}
				</div>
				<div className="card card-pad">
					<div className="card-title">Cost vs success · {domain}</div>
					<Scatter
						offers={ranked}
						eligible={eligible}
						domain={domain}
						best={best}
					/>
				</div>
			</div>

			<div className="card">
				<table className="tbl">
					<thead>
						<tr>
							<th>#</th>
							<th>Provider</th>
							<th>Exit</th>
							<th>Sourcing</th>
							<th className="num">List $/GB</th>
							<th className="num">Success</th>
							<th className="num">Effective $/GB</th>
							<th>Pool fraud</th>
							<th>Fits risk {risk}?</th>
						</tr>
					</thead>
					<tbody>
						{ranked.map((o, i) => {
							const ok = eligible.has(o);
							return (
								<tr
									key={o.provider}
									className={o === best ? "hl" : ok ? "" : "dim"}
								>
									<td className="mono faint">{i + 1}</td>
									<td style={{ fontWeight: 600 }}>{o.provider}</td>
									<td>
										<ExitBadge type={o.exitType} />
									</td>
									<td className="muted">{o.sourcing}</td>
									<td className="num mono">{usd(withTake(o.pricePerGb))}</td>
									<td className="num mono">
										{Math.round(successRate(domain, o.provider) * 100)}%
									</td>
									<td className="num mono" style={{ fontWeight: 600 }}>
										{usd(withTake(effectivePrice(o, domain)))}
									</td>
									<td>
										<Meter value={o.fraudScore} invert />
									</td>
									<td>
										{ok ? (
											<span className="badge badge-good">Yes</span>
										) : (
											<span className="badge">{whyNot(o, resolved)}</span>
										)}
									</td>
								</tr>
							);
						})}
					</tbody>
				</table>
			</div>
			<p className="faint" style={{ marginTop: 10 }}>
				Effective price = list price ÷ success rate, i.e. what you pay once
				retries are counted. All prices include the 5% humanip fee.
			</p>
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
					<circle
						cx={cx}
						cy={cy}
						r={o === best ? 7 : 5}
						fill={`var(--${o.exitType})`}
						stroke={o === best ? "var(--text)" : "none"}
						strokeWidth="2"
					/>
					<text x={cx + 10} y={ly + 4} fontSize="10.5" fill="var(--text-2)">
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
