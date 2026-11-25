// Cyan Orbital: spatial and ambient. The frontier as an orbit around the
// gateway (closer = more reliable, further round the dial = more expensive),
// routes as living orb avatars, big light numerals, chromatic metal.

import { Mark } from "../../components/icons";
import LiquidMetal from "../../vendor/LiquidMetal";
import ThinkingOrb, { type OrbState } from "../../vendor/ThinkingOrb";
import {
	costOf,
	frontier,
	offers,
	pct,
	pick,
	routes,
	usd,
	worksOf,
} from "./data";

const S = 520;
const C = S / 2;
const maxCost = Math.max(...offers.map(costOf));
// Zoom on the 90-100% band; anything worse rides the outer edge.
const radius = (w: number) =>
	70 + (Math.min(1 - Math.min(w, 1), 0.1) / 0.1) * 170;
const angle = (c: number) => (-90 + (c / maxCost) * 300) * (Math.PI / 180);
const pos = (o: (typeof offers)[number]) => {
	const r = radius(worksOf(o));
	const a = angle(costOf(o));
	return { x: C + r * Math.cos(a), y: C + r * Math.sin(a) };
};

const ORB: Record<string, OrbState> = {
	healthy: "breathing",
	degraded: "weaving",
};

export function Orbital() {
	const pp = pos(pick);
	const path = frontier
		.map((o, i) => {
			const p = pos(o);
			return `${i === 0 ? "M" : "L"}${p.x.toFixed(1)} ${p.y.toFixed(1)}`;
		})
		.join(" ");

	return (
		<div className="ob">
			<div className="ob-hero">
				<div className="ob-orbit">
					<svg
						viewBox={`0 0 ${S} ${S}`}
						className="ob-svg"
						role="img"
						aria-label="Orbital frontier"
					>
						<defs>
							<radialGradient id="ob-glow">
								<stop offset="0" stopColor="#7fdfff" stopOpacity="0.28" />
								<stop offset="1" stopColor="#7fdfff" stopOpacity="0" />
							</radialGradient>
						</defs>
						<circle cx={C} cy={C} r={C} fill="url(#ob-glow)" />
						{[0.99, 0.97, 0.95, 0.93, 0.9].map((w) => (
							<g key={w}>
								<circle
									cx={C}
									cy={C}
									r={radius(w)}
									className={w === 0.95 ? "ob-ring floor" : "ob-ring"}
								/>
								<text x={C + 4} y={C - radius(w) - 4} className="ob-ringlbl">
									{pct(w)}
								</text>
							</g>
						))}
						{[0, 2, 4, 6, 8].map((c) => {
							const a = angle(c);
							return (
								<text
									key={c}
									x={C + 262 * Math.cos(a)}
									y={C + 262 * Math.sin(a)}
									className="ob-cost"
									textAnchor="middle"
								>
									${c}
								</text>
							);
						})}
						<path d={path} className="ob-front" />
						{offers.map((o) => {
							const p = pos(o);
							const on = frontier.includes(o);
							return (
								<g key={o.provider} className={on ? "ob-pt on" : "ob-pt"}>
									<circle cx={p.x} cy={p.y} r={on ? 5 : 3} />
									{on && o !== pick && (
										<text x={p.x + 10} y={p.y + 4}>
											{o.provider}
										</text>
									)}
								</g>
							);
						})}
					</svg>
					<div className="ob-hub">
						<Mark name="tunnel" size={30} />
					</div>
					<div
						className="ob-pickorb"
						style={{
							left: `${(pp.x / S) * 100}%`,
							top: `${(pp.y / S) * 100}%`,
						}}
					>
						<ThinkingOrb state="connecting" size={46} color="#c3fffd" />
						<span>{pick.provider}</span>
					</div>
				</div>

				<div className="ob-side">
					<div className="ob-eyebrow">Default pick · web.whatsapp.com</div>
					<LiquidMetal
						preset="chromatic"
						theme="dark"
						radius={22}
						ringWidth={2}
						strength={1}
					>
						<div className="ob-pick">
							<div className="ob-price">
								{usd(costOf(pick))}
								<small>per working GB</small>
							</div>
							<div className="ob-stats">
								<div>
									<b>{pct(worksOf(pick))}</b>
									<span>works</span>
								</div>
								<div>
									<b>{pick.fraudScore}</b>
									<span>fraud</span>
								</div>
								<div>
									<b>90d</b>
									<span>max pin</span>
								</div>
							</div>
							<button type="button" className="ob-btn">
								Create route
							</button>
						</div>
					</LiquidMetal>
					<p className="ob-caption">
						Closer to the centre works more often. Further round the dial costs
						more. The bright arc is the frontier.
					</p>
				</div>
			</div>

			<div className="ob-routes">
				{routes.map((r) => (
					<div key={r.id} className={`ob-route ${r.status}`}>
						<div className="ob-avatar">
							{r.status === "pin_lost" ? (
								<Mark name="broken-robot" size={34} label="pin lost" />
							) : (
								<ThinkingOrb
									state={
										r.useCase === "browser"
											? "searching"
											: (ORB[r.status] ?? "breathing")
									}
									size={52}
									color={r.status === "degraded" ? "#ffd479" : "#7fdfff"}
								/>
							)}
						</div>
						<div>
							<div className="ob-rname">{r.name}</div>
							<div className="ob-rstate">
								{r.status === "pin_lost"
									? "pin lost · waiting on you"
									: r.pinnedFor
										? `pinned ${r.pinnedFor}`
										: `${r.sessions} live`}
							</div>
						</div>
					</div>
				))}
			</div>
		</div>
	);
}
