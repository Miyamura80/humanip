// Picture-book primitives: one icon vocabulary, used everywhere.
//   📱 mobile · 🏢 ISP · 🏠 residential
//   📌 sticky · 🔄 rotating
//   🤖 browser agent · 🌉 bridge · 🕷️ scraper
//   🛡️ fraud · ⬇️ exit downgrade · 🏷️ sourcing · ⏱️ sticky guarantee

import type { ExitType, RouteMode } from "../lib/policy";

export const EXIT_ICON: Record<ExitType, string> = {
	mobile: "📱",
	isp: "🏢",
	residential: "🏠",
};
export const MODE_ICON: Record<RouteMode, string> = {
	sticky: "📌",
	rotating: "🔄",
};
export const USE_ICON = { browser: "🤖", mautrix: "🌉", scrape: "🕷️" } as const;

const tone = (good: number) =>
	good >= 80 ? "var(--accent)" : good >= 60 ? "var(--warn)" : "var(--bad)";

/** Donut ring with the value in the middle. `invert`: lower is better. */
export function Ring({
	value,
	label,
	invert,
	size = 52,
}: {
	value: number | null;
	label: string;
	invert?: boolean;
	size?: number;
}) {
	const r = size / 2 - 5;
	const c = 2 * Math.PI * r;
	const v = value ?? 0;
	const color = value === null ? "var(--text-3)" : tone(invert ? 100 - v : v);
	return (
		<div className="ring" title={`${label}: ${value ?? "n/a"}`}>
			<svg width={size} height={size} aria-hidden="true">
				<circle
					cx={size / 2}
					cy={size / 2}
					r={r}
					stroke="var(--surface-3)"
					strokeWidth="5"
					fill="none"
				/>
				<circle
					cx={size / 2}
					cy={size / 2}
					r={r}
					stroke={color}
					strokeWidth="5"
					fill="none"
					strokeLinecap="round"
					strokeDasharray={`${(Math.max(v, 2) / 100) * c} ${c}`}
					transform={`rotate(-90 ${size / 2} ${size / 2})`}
				/>
				<text
					x="50%"
					y="54%"
					textAnchor="middle"
					dominantBaseline="middle"
					fontSize="13"
					fontWeight="600"
					fill={color}
				>
					{value ?? "–"}
				</text>
			</svg>
			<span className="ring-label">{label}</span>
		</div>
	);
}

/** client ─▶ humanip ─▶ exit ─▶ site, the core picture of the product. */
export function Flow({
	from,
	exit,
	ip,
	broken,
	mode,
}: {
	from: string;
	exit: ExitType;
	ip: string;
	broken?: boolean;
	mode: RouteMode;
}) {
	return (
		<div className={`flow${broken ? " broken" : ""}`}>
			<span className="node" title="client">
				{from}
			</span>
			<span className="wire" />
			<span className="node hub" title="humanip">
				hip
			</span>
			<span className="wire">
				<span className="wire-tag">{MODE_ICON[mode]}</span>
			</span>
			<span className="node" title={`exit: ${exit}`}>
				{EXIT_ICON[exit]}
			</span>
			<span className={`wire${broken ? " cut" : ""}`}>
				{broken && <span className="wire-tag">✖</span>}
			</span>
			<span className="node" title="target">
				🌐
			</span>
			<span className="flow-ip mono">{ip}</span>
		</div>
	);
}

const PIP_KEYS = ["p0", "p1", "p2", "p3", "p4", "p5"];

/** Small horizontal level bar, 0-4 pips. */
export function Pips({
	n,
	of = 4,
	color,
}: {
	n: number;
	of?: number;
	color?: string;
}) {
	return (
		<span className="pips" role="img" aria-label={`${n} of ${of}`}>
			{PIP_KEYS.slice(0, of).map((k, i) => (
				<i
					key={k}
					style={i < n ? { background: color ?? "var(--accent)" } : undefined}
				/>
			))}
		</span>
	);
}

/** Half-circle risk gauge with a needle. */
export function Gauge({ value }: { value: number }) {
	const a = Math.PI * (1 - value / 100);
	const cx = 110;
	const cy = 100;
	const r = 80;
	const nx = cx + Math.cos(a) * (r - 14);
	const ny = cy - Math.sin(a) * (r - 14);
	return (
		<svg
			viewBox="0 0 220 116"
			className="gauge"
			role="img"
			aria-label={`Risk ${value}`}
		>
			<defs>
				<linearGradient id="g-risk" x1="0" x2="1">
					<stop offset="0" stopColor="var(--accent)" />
					<stop offset="0.6" stopColor="var(--warn)" />
					<stop offset="1" stopColor="var(--bad)" />
				</linearGradient>
			</defs>
			<path
				d={`M ${cx - r} ${cy} A ${r} ${r} 0 0 1 ${cx + r} ${cy}`}
				stroke="url(#g-risk)"
				strokeWidth="14"
				fill="none"
				strokeLinecap="round"
			/>
			<line
				x1={cx}
				y1={cy}
				x2={nx}
				y2={ny}
				stroke="var(--text)"
				strokeWidth="3"
				strokeLinecap="round"
			/>
			<circle cx={cx} cy={cy} r="6" fill="var(--text)" />
			<text
				x={cx - r}
				y={cy + 14}
				fontSize="10"
				fill="var(--text-3)"
				textAnchor="middle"
			>
				🛡️ safe
			</text>
			<text
				x={cx + r}
				y={cy + 14}
				fontSize="10"
				fill="var(--text-3)"
				textAnchor="middle"
			>
				💸 cheap
			</text>
		</svg>
	);
}
