// Pareto frontier visuals. Convention on every chart: up = better on the
// y-axis, left = better on the x-axis, so the frontier hugs the top-left and
// everything it dominates sits in the shaded area below-right.

import { type ProviderOffer, successRate } from "../lib/mock";
import {
	AXES,
	type Axis,
	axis,
	costReliabilityFrontier,
	paretoSet,
} from "../lib/pareto";
import { EXIT_ICON } from "./visual";

interface PlotProps {
	x: Axis;
	y: Axis;
	offers: ProviderOffer[];
	domain: string;
	eligible?: Set<ProviderOffer>;
	pick?: ProviderOffer;
	auto?: ProviderOffer;
	floor?: number;
	small?: boolean;
	onPick?: (o: ProviderOffer) => void;
}

const span = (vals: number[]) => {
	const lo = Math.min(...vals);
	const hi = Math.max(...vals);
	const pad = (hi - lo || 1) * 0.08;
	return [lo - pad, hi + pad, lo, hi] as const;
};

export function FrontierPlot({
	x,
	y,
	offers,
	domain,
	eligible,
	pick,
	auto,
	floor,
	small,
	onPick,
}: PlotProps) {
	const W = small ? 260 : 620;
	const H = small ? 170 : 330;
	const p = small
		? { l: 40, r: 12, t: 22, b: 20 }
		: { l: 44, r: 20, t: 14, b: 30 };
	const pool = eligible ? offers.filter((o) => eligible.has(o)) : offers;
	// Scale to what you can actually buy; out-of-budget outliers pin to the edge.
	const basis = pool.length >= 2 ? pool : offers;
	const [x0, x1, xlo, xhi] = span(basis.map((o) => x.get(o, domain)));
	const yVals = basis.map((o) => y.get(o, domain));
	if (floor !== undefined && y.key === "success") yVals.push(floor);
	const [y0, y1, ylo, yhi] = span(yVals);
	const clamp = (t: number) => Math.min(1, Math.max(0, t));
	// Orient: better is left (x) and up (y).
	const sx = (v: number) => {
		const t = clamp((v - x0) / (x1 - x0));
		return p.l + (x.better === "low" ? t : 1 - t) * (W - p.l - p.r);
	};
	const sy = (v: number) => {
		const t = clamp((v - y0) / (y1 - y0));
		return p.t + (y.better === "high" ? 1 - t : t) * (H - p.t - p.b);
	};

	const front = [...paretoSet(pool, [x, y], domain)]
		.map((o) => ({ o, px: sx(x.get(o, domain)), py: sy(y.get(o, domain)) }))
		.sort((a, b) => a.px - b.px);
	const frontSet = new Set(front.map((f) => f.o));

	const bottom = H - p.b;
	const right = W - p.r;
	let stair = "";
	front.forEach((f, i) => {
		stair += i === 0 ? `M ${f.px} ${f.py}` : ` H ${f.px} V ${f.py}`;
	});
	if (front.length) stair += ` H ${right}`;
	const shade = front.length
		? `M ${front[0].px} ${bottom} V ${front[0].py} ${stair.slice(stair.indexOf("H") - 1)} V ${bottom} Z`
		: "";

	const ticksY = [...new Set([0, 0.5, 1].map((t) => ylo + t * (yhi - ylo)))];
	const ticksX = [...new Set([0, 0.5, 1].map((t) => xlo + t * (xhi - xlo)))];
	const fs = small ? 9 : 10.5;

	return (
		<svg
			viewBox={`0 0 ${W} ${H}`}
			className="frontier"
			role="img"
			aria-label={`${y.label} versus ${x.label} Pareto frontier`}
		>
			{small && (
				<text x={p.l} y={12} fontSize="11" fill="var(--text-2)">
					{y.icon} {y.label} vs {x.icon} {x.label}
				</text>
			)}
			{ticksY.map((v) => (
				<g key={`y${v}`}>
					<line
						x1={p.l}
						x2={right}
						y1={sy(v)}
						y2={sy(v)}
						stroke="var(--border)"
					/>
					<text
						x={p.l - 5}
						y={sy(v) + 3}
						textAnchor="end"
						fontSize={fs}
						fill="var(--text-3)"
					>
						{y.fmt(v)}
					</text>
				</g>
			))}
			{ticksX.map((v) => (
				<text
					key={`x${v}`}
					x={sx(v)}
					y={H - 6}
					textAnchor="middle"
					fontSize={fs}
					fill="var(--text-3)"
				>
					{x.fmt(v)}
				</text>
			))}
			<path d={shade} fill="var(--surface-3)" opacity="0.55" />
			<path
				d={stair}
				fill="none"
				stroke="var(--accent)"
				strokeWidth={small ? 1.5 : 2.5}
				strokeLinejoin="round"
			/>
			{floor !== undefined && y.key === "success" && (
				<g>
					<line
						x1={p.l}
						x2={right}
						y1={sy(floor)}
						y2={sy(floor)}
						stroke="var(--warn)"
						strokeDasharray="5 4"
					/>
					<text
						x={right - 4}
						y={sy(floor) + 14}
						textAnchor="end"
						fontSize={fs}
						fill="var(--warn)"
					>
						🎯 {y.fmt(floor)} floor
					</text>
				</g>
			)}

			{placeLabels(
				offers.map((o) => ({
					o,
					cx: sx(x.get(o, domain)),
					cy: sy(y.get(o, domain)),
				})),
			).map(({ o, cx, cy, ly }) => {
				const out = eligible && !eligible.has(o);
				const onFront = frontSet.has(o);
				const flip = cx > W - (small ? 70 : 120);
				return (
					// biome-ignore lint/a11y/noStaticElementInteractions: SVG point; table below is the keyboard path
					<g
						key={o.provider}
						opacity={out ? 0.18 : onFront ? 1 : 0.45}
						onClick={() => onPick?.(o)}
						style={{ cursor: onPick ? "pointer" : undefined }}
					>
						{o === auto && (
							<circle
								cx={cx}
								cy={cy}
								r={small ? 10 : 15}
								fill="var(--accent-dim)"
								stroke="var(--accent)"
								strokeWidth="2"
							/>
						)}
						{o === pick && o !== auto && (
							<circle
								cx={cx}
								cy={cy}
								r={small ? 10 : 15}
								fill="none"
								stroke="var(--text)"
								strokeWidth="2"
							/>
						)}
						<text
							x={cx}
							y={cy + (small ? 4 : 5)}
							fontSize={small ? 11 : 15}
							textAnchor="middle"
						>
							{EXIT_ICON[o.exitType]}
						</text>
						{(!small || onFront) && (
							<text
								x={flip ? cx - 14 : cx + 14}
								y={ly + 4}
								textAnchor={flip ? "end" : "start"}
								fontSize={fs}
								fill={onFront ? "var(--text)" : "var(--text-3)"}
							>
								{o === auto && !small ? "⭐ " : ""}
								{o.provider}
							</text>
						)}
					</g>
				);
			})}
		</svg>
	);
}

/** Greedy vertical nudge so neighbouring point labels never overlap. */
function placeLabels<T extends { cx: number; cy: number }>(pts: T[]) {
	const placed: (T & { ly: number })[] = [];
	for (const pt of [...pts].sort((a, b) => a.cy - b.cy)) {
		let ly = pt.cy;
		for (const q of placed) {
			if (Math.abs(q.cx - pt.cx) < 80 && Math.abs(q.ly - ly) < 12)
				ly = q.ly + 12;
		}
		placed.push({ ...pt, ly });
	}
	return placed;
}

/**
 * Parallel coordinates across every axis, each flipped so up = better.
 * Bold lines are not beaten on all five axes at once by any other offer.
 */
export function ParallelAxes({
	offers,
	domain,
	eligible,
	pick,
	onPick,
}: {
	offers: ProviderOffer[];
	domain: string;
	eligible: Set<ProviderOffer>;
	pick?: ProviderOffer;
	onPick: (o: ProviderOffer) => void;
}) {
	const W = 1000;
	const H = 250;
	const p = { l: 60, r: 120, t: 40, b: 22 };
	const front = paretoSet(offers, AXES, domain);
	const gap = (W - p.l - p.r) / (AXES.length - 1);
	const ranges = AXES.map((a) => span(offers.map((o) => a.get(o, domain))));
	// Right-edge labels, nudged apart so neighbours never overlap.
	const last = AXES[AXES.length - 1];
	const labelY = new Map<ProviderOffer, number>();
	let prev = Number.NEGATIVE_INFINITY;
	for (const o of [...offers].sort(
		(a, b) =>
			yOfRaw(ranges, AXES.length - 1, last.get(a, domain)) -
			yOfRaw(ranges, AXES.length - 1, last.get(b, domain)),
	)) {
		const y0 = Math.max(
			yOfRaw(ranges, AXES.length - 1, last.get(o, domain)),
			prev + 13,
		);
		labelY.set(o, y0);
		prev = y0;
	}
	function yOfRaw(r: typeof ranges, ai: number, v: number) {
		const [lo, hi] = r[ai];
		const t = (v - lo) / (hi - lo);
		return p.t + (AXES[ai].better === "high" ? 1 - t : t) * (H - p.t - p.b);
	}
	const yOf = (ai: number, v: number) => {
		const [lo, hi] = ranges[ai];
		const t = (v - lo) / (hi - lo);
		return p.t + (AXES[ai].better === "high" ? 1 - t : t) * (H - p.t - p.b);
	};
	const order = [...offers].sort(
		(a, b) => Number(a === pick) - Number(b === pick),
	);

	return (
		<svg
			viewBox={`0 0 ${W} ${H}`}
			className="parallel"
			role="img"
			aria-label="Parallel coordinates"
		>
			{AXES.map((a, ai) => {
				const x = p.l + ai * gap;
				const [, , lo, hi] = ranges[ai];
				const top = a.better === "high" ? hi : lo;
				const bot = a.better === "high" ? lo : hi;
				return (
					<g key={a.key}>
						<line
							x1={x}
							x2={x}
							y1={p.t}
							y2={H - p.b}
							stroke="var(--border-strong)"
							strokeWidth="2"
						/>
						<text x={x} y={14} textAnchor="middle" fontSize="15">
							{a.icon}
						</text>
						<text
							x={x}
							y={30}
							textAnchor="middle"
							fontSize="11"
							fill="var(--text-2)"
						>
							{a.label}
						</text>
						<text x={x + 6} y={p.t + 4} fontSize="9.5" fill="var(--accent)">
							{a.fmt(top)}
						</text>
						<text x={x + 6} y={H - p.b} fontSize="9.5" fill="var(--text-3)">
							{a.fmt(bot)}
						</text>
					</g>
				);
			})}
			{order.map((o) => {
				const pts = AXES.map(
					(a, ai) => `${p.l + ai * gap},${yOf(ai, a.get(o, domain))}`,
				);
				const isPick = o === pick;
				const onFront = front.has(o);
				const out = !eligible.has(o);
				const lastY = yOf(
					AXES.length - 1,
					AXES[AXES.length - 1].get(o, domain),
				);
				return (
					// biome-ignore lint/a11y/noStaticElementInteractions: SVG line; table below is the keyboard path
					<g
						key={o.provider}
						onClick={() => onPick(o)}
						style={{ cursor: "pointer" }}
						opacity={out ? 0.15 : 1}
					>
						<polyline
							points={pts.join(" ")}
							fill="none"
							stroke={
								isPick
									? "var(--text)"
									: onFront
										? `var(--${o.exitType})`
										: "var(--text-3)"
							}
							strokeWidth={isPick ? 4 : onFront ? 2.2 : 1}
							strokeDasharray={onFront || isPick ? undefined : "3 3"}
							strokeLinejoin="round"
						/>
						<text
							x={W - p.r + 10}
							y={(labelY.get(o) ?? lastY) + 4}
							fontSize="11"
							fill={isPick ? "var(--text)" : "var(--text-2)"}
						>
							{EXIT_ICON[o.exitType]} {o.provider}
						</text>
					</g>
				);
			})}
		</svg>
	);
}

/** Walk the cost/reliability frontier: cheapest on the left, most reliable on the right. */
export function FrontierWalk({
	offers,
	domain,
	pick,
	auto,
	onPick,
}: {
	offers: ProviderOffer[];
	domain: string;
	pick?: ProviderOffer;
	auto?: ProviderOffer;
	onPick: (o: ProviderOffer) => void;
}) {
	const front = costReliabilityFrontier(offers, domain);
	const cost = axis("cost");
	return (
		<div className="walk">
			<span className="walk-end">💸 cheapest</span>
			{front.map((o, i) => (
				<span key={o.provider} className="walk-step">
					{i > 0 && <span className="walk-arrow">→</span>}
					<button
						type="button"
						className={`walk-node${o === pick ? " on" : ""}`}
						onClick={() => onPick(o)}
					>
						{o === auto && <span className="walk-star">⭐ default</span>}
						<span className="big-icon">{EXIT_ICON[o.exitType]}</span>
						<b>{o.provider}</b>
						<span className="mono">{cost.fmt(cost.get(o, domain))}</span>
						<span className="mono faint">
							✅ {Math.round(successRate(domain, o.provider) * 100)}%
						</span>
					</button>
				</span>
			))}
			<span className="walk-end">✅ most reliable</span>
		</div>
	);
}

/** States the reliability policy and whether the chosen exit actually meets it. */
export function ReliabilityBadge({
	edited,
	risk,
	floor,
	achieved,
}: {
	edited: boolean;
	risk: number;
	floor: number;
	achieved?: number;
}) {
	const pct = (n: number) => `${Math.round(n * 100)}%`;
	if (achieved !== undefined && achieved < floor) {
		return (
			<span className="badge badge-warn">
				⚠️ nothing ≥{pct(floor)} · best {pct(achieved)}
			</span>
		);
	}
	return edited ? (
		<span className="badge badge-warn">
			⚙️ custom · risk {risk} · ≥{pct(floor)}
		</span>
	) : (
		<span className="badge badge-good">
			⭐ high reliability · ≥{pct(floor)} works
		</span>
	);
}
