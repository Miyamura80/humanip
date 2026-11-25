// Orbital primitives shared by every screen: exit marks, client logos,
// status avatars, rings, the flow line, pips and the risk gauge.

import type { RouteRow } from "../lib/mock";
import { EXIT_LABEL, type ExitType, type RouteMode } from "../lib/policy";
import ThinkingOrb, { type OrbState } from "../vendor/ThinkingOrb";
import { Logo, Mark, type MarkName } from "./icons";

const EXIT_MARK: Record<ExitType, MarkName> = {
	mobile: "exit-mobile",
	isp: "exit-isp",
	residential: "exit-residential",
};

export type UseCase = RouteRow["useCase"];

export const USE_LABEL: Record<UseCase, string> = {
	browser: "Browser agent",
	mautrix: "Bridge",
	scrape: "Scraper",
};

/** Client logo by use case: agent, Matrix bridge (Beeper), scraper (Apify). */
export function ClientLogo({
	use,
	size = 20,
}: {
	use: UseCase;
	size?: number;
}) {
	const name =
		use === "mautrix" ? "beeper" : use === "scrape" ? "apify" : "claude-code";
	return <Logo name={name} size={size} label={USE_LABEL[use]} />;
}

/** Exit mark tinted with its exit colour. */
export function ExitMark({
	type,
	size = 18,
}: {
	type: ExitType;
	size?: number;
}) {
	return (
		<Mark
			name={EXIT_MARK[type]}
			size={size}
			label={EXIT_LABEL[type]}
			style={{ color: `var(--${type})` }}
		/>
	);
}

const ORB_FOR: Record<UseCase, OrbState> = {
	mautrix: "breathing",
	browser: "searching",
	scrape: "weaving",
};

/** Live status avatar: an animated orb, or a broken robot when the pin is lost. */
export function StatusAvatar({
	route,
	size = 52,
}: {
	route: RouteRow;
	size?: number;
}) {
	if (route.status === "pin_lost") {
		return (
			<span className="avatar avatar-bad" style={{ width: size, height: size }}>
				<Mark name="broken-robot" size={size * 0.62} label="Pin lost" />
			</span>
		);
	}
	const color = route.status === "degraded" ? "#ffd479" : "#7fdfff";
	return (
		<span className="avatar" style={{ width: size, height: size }}>
			<ThinkingOrb
				state={ORB_FOR[route.useCase]}
				size={size}
				color={color}
				aria-label={route.status}
			/>
		</span>
	);
}

const tone = (good: number) =>
	good >= 80 ? "var(--accent)" : good >= 60 ? "var(--warn)" : "var(--bad)";

/** Thin ring gauge with a light numeral. `invert`: lower is better. */
export function Ring({
	value,
	label,
	invert,
	size = 54,
}: {
	value: number | null;
	label: string;
	invert?: boolean;
	size?: number;
}) {
	const r = size / 2 - 3;
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
					stroke="var(--line)"
					strokeWidth="2"
					fill="none"
				/>
				<circle
					cx={size / 2}
					cy={size / 2}
					r={r}
					stroke={color}
					strokeWidth="2"
					fill="none"
					strokeLinecap="round"
					strokeDasharray={`${(Math.max(v, 2) / 100) * c} ${c}`}
					transform={`rotate(-90 ${size / 2} ${size / 2})`}
					style={{ filter: `drop-shadow(0 0 4px ${color})` }}
				/>
				<text
					x="50%"
					y="53%"
					textAnchor="middle"
					dominantBaseline="middle"
					fontSize="15"
					fontWeight="300"
					fill="var(--text)"
				>
					{value ?? "-"}
				</text>
			</svg>
			<span className="ring-label">{label}</span>
		</div>
	);
}

/** client -> humanip -> exit -> target, as a glowing line of nodes. */
export function Flow({
	use,
	exit,
	target,
	caption,
	mode,
	broken,
}: {
	use: UseCase;
	exit: ExitType;
	target: string;
	caption?: string;
	mode: RouteMode;
	broken?: boolean;
}) {
	return (
		<div className={`flow${broken ? " broken" : ""}`}>
			<span className="node">
				<ClientLogo use={use} size={18} />
			</span>
			<span className="wire" />
			<span className="node hub" title="humanip">
				<Mark name="tunnel" size={16} />
			</span>
			<span className={`wire ${mode}`} />
			<span className="node">
				<ExitMark type={exit} />
			</span>
			<span className={`wire${broken ? " cut" : ""}`} />
			<span className="node target">{target}</span>
			{caption && <span className="flow-caption mono">{caption}</span>}
		</div>
	);
}

const PIP_KEYS = ["p0", "p1", "p2", "p3", "p4", "p5"];

/** Small level bar, 0-4 segments. */
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
					style={
						i < n
							? {
									background: color ?? "var(--accent)",
									boxShadow: `0 0 6px ${color ?? "var(--accent)"}`,
								}
							: undefined
					}
				/>
			))}
		</span>
	);
}

/** Half-circle risk gauge. */
export function Gauge({ value }: { value: number }) {
	const a = Math.PI * (1 - value / 100);
	const cx = 110;
	const cy = 100;
	const r = 80;
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
				stroke="var(--line)"
				strokeWidth="2"
				fill="none"
			/>
			<path
				d={`M ${cx - r} ${cy} A ${r} ${r} 0 0 1 ${cx + r * Math.cos(a)} ${cy - r * Math.sin(a)}`}
				stroke="url(#g-risk)"
				strokeWidth="3"
				fill="none"
				strokeLinecap="round"
			/>
			<circle
				cx={cx + r * Math.cos(a)}
				cy={cy - r * Math.sin(a)}
				r="5"
				fill="var(--text)"
			/>
			<text
				x={cx}
				y={cy - 12}
				textAnchor="middle"
				fontSize="34"
				fontWeight="200"
				fill="var(--text)"
			>
				{value}
			</text>
			<text
				x={cx - r}
				y={cy + 14}
				fontSize="9"
				fill="var(--text-3)"
				textAnchor="middle"
			>
				SAFE
			</text>
			<text
				x={cx + r}
				y={cy + 14}
				fontSize="9"
				fill="var(--text-3)"
				textAnchor="middle"
			>
				CHEAP
			</text>
		</svg>
	);
}
