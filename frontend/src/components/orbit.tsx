// The orbital frontier: offers placed around the gateway. Distance from the
// centre is reliability (closer works more often), angle is cost per working
// GB (further round the dial costs more). The glowing arc is the Pareto
// frontier; the pick carries a live orb.

import type { ProviderOffer } from "../lib/mock";
import { axis, costReliabilityFrontier } from "../lib/pareto";
import ThinkingOrb from "../vendor/ThinkingOrb";
import { Mark } from "./icons";

const cost = axis("cost");
const works = axis("success");

export function OrbitalFrontier({
	offers,
	domain,
	eligible,
	pick,
	auto,
	floor,
	onPick,
	size = 520,
}: {
	offers: ProviderOffer[];
	domain: string;
	eligible: Set<ProviderOffer>;
	pick?: ProviderOffer;
	auto?: ProviderOffer;
	floor: number;
	onPick?: (o: ProviderOffer) => void;
	size?: number;
}) {
	const S = 520; // drawing units; the SVG scales to `size`
	const C = S / 2;
	const pool = offers.filter((o) => eligible.has(o));
	const front = costReliabilityFrontier(pool, domain);

	// Reliability band: from a little under the worst frontier exit (or the
	// floor) up to 100%. Anything worse rides the outer edge.
	const lowest = Math.min(floor, ...front.map((o) => works.get(o, domain)));
	const lo = Math.max(0.5, Math.floor((lowest - 0.03) * 100) / 100);
	const radius = (w: number) =>
		64 + (Math.min(1 - Math.min(w, 1), 1 - lo) / (1 - lo)) * 176;
	const maxCost = Math.max(...pool.map((o) => cost.get(o, domain)), 1) * 1.08;
	const angle = (c: number) =>
		(-90 + (Math.min(c, maxCost) / maxCost) * 300) * (Math.PI / 180);
	const pos = (o: ProviderOffer) => {
		const r = radius(works.get(o, domain));
		const a = angle(cost.get(o, domain));
		return { x: C + r * Math.cos(a), y: C + r * Math.sin(a) };
	};
	const rings = [0, 1, 2, 3].map((k) => 1 - ((1 - lo) * (k + 1)) / 4);
	const path = front
		.map(
			(o, i) =>
				`${i === 0 ? "M" : "L"}${pos(o).x.toFixed(1)} ${pos(o).y.toFixed(1)}`,
		)
		.join(" ");
	const pp = pick ? pos(pick) : undefined;
	const costTicks = [0.25, 0.5, 0.75, 1].map((t) => t * maxCost);

	return (
		<div className="orbit" style={{ width: size, height: size }}>
			<svg
				viewBox={`0 0 ${S} ${S}`}
				className="orbit-svg"
				role="img"
				aria-label={`Orbital frontier for ${domain}`}
			>
				<defs>
					<radialGradient id="orbit-glow">
						<stop offset="0" stopColor="#7fdfff" stopOpacity="0.22" />
						<stop offset="1" stopColor="#7fdfff" stopOpacity="0" />
					</radialGradient>
				</defs>
				<circle cx={C} cy={C} r={C} fill="url(#orbit-glow)" />
				{rings.map((w) => (
					<g key={w}>
						<circle cx={C} cy={C} r={radius(w)} className="orbit-ring" />
						<text x={C + 4} y={C - radius(w) - 4} className="orbit-tick">
							{works.fmt(w)}
						</text>
					</g>
				))}
				<circle cx={C} cy={C} r={radius(floor)} className="orbit-ring floor" />
				<text
					x={C + radius(floor) * 0.71 + 6}
					y={C + radius(floor) * 0.71 + 12}
					className="orbit-tick floor"
				>
					{works.fmt(floor)} floor
				</text>
				{costTicks.map((c) => {
					const a = angle(c);
					return (
						<text
							key={c}
							x={C + 256 * Math.cos(a)}
							y={C + 256 * Math.sin(a)}
							className="orbit-tick"
							textAnchor="middle"
						>
							{cost.fmt(c)}
						</text>
					);
				})}
				<path d={path} className="orbit-front" />
				{offers.map((o) => {
					const p = pos(o);
					const out = !eligible.has(o);
					const on = front.includes(o);
					const flip = p.x > C + 150;
					return (
						// biome-ignore lint/a11y/useSemanticElements: SVG has no <button>; role + tabIndex + Enter give keyboard access
						<g
							key={o.provider}
							className={`orbit-pt${on ? " on" : ""}${out ? " out" : ""}`}
							role="button"
							tabIndex={out ? -1 : 0}
							aria-label={`${o.provider}, ${works.fmt(works.get(o, domain))} works, ${cost.fmt(cost.get(o, domain))}`}
							onClick={() => !out && onPick?.(o)}
							onKeyDown={(e) => e.key === "Enter" && !out && onPick?.(o)}
						>
							<circle cx={p.x} cy={p.y} r={14} className="hit" />
							{o === auto && o !== pick && (
								<circle cx={p.x} cy={p.y} r={11} className="orbit-auto" />
							)}
							<circle
								cx={p.x}
								cy={p.y}
								r={on ? 5 : 3.5}
								style={{ fill: `var(--${o.exitType})` }}
							/>
							{on && o !== pick && (
								<text
									x={flip ? p.x - 12 : p.x + 12}
									y={p.y + 4}
									textAnchor={flip ? "end" : "start"}
								>
									{o.provider}
									{o === auto ? " (default)" : ""}
								</text>
							)}
						</g>
					);
				})}
			</svg>
			<div className="orbit-hub">
				<Mark name="tunnel" size={28} />
			</div>
			{pick && pp && (
				<div
					className="orbit-pick"
					style={{ left: `${(pp.x / S) * 100}%`, top: `${(pp.y / S) * 100}%` }}
				>
					<ThinkingOrb state="connecting" size={44} color="#c3fffd" />
					<span>{pick.provider}</span>
				</div>
			)}
		</div>
	);
}
