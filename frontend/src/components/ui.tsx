import { useState } from "react";
import { EXIT_LABEL, type ExitType } from "../lib/policy";

export function ExitBadge({ type }: { type: ExitType }) {
	return (
		<span className={`badge exit-${type}`}>
			<span className="dot" />
			{EXIT_LABEL[type]}
		</span>
	);
}

/** 0-100 score bar. `invert` means lower is better (fraud scores). */
export function Meter({ value, invert }: { value: number; invert?: boolean }) {
	const good = invert ? 100 - value : value;
	const color =
		good >= 80 ? "var(--accent)" : good >= 60 ? "var(--warn)" : "var(--bad)";
	return (
		<span className="meter">
			<span className="meter-track">
				<span
					className="meter-fill"
					style={{
						width: `${Math.max(value, 3)}%`,
						background: color,
						display: "block",
					}}
				/>
			</span>
			<span className="mono" style={{ color }}>
				{value}
			</span>
		</span>
	);
}

export function Sparkline({ points }: { points: number[] }) {
	const w = 84;
	const h = 22;
	const step = w / (points.length - 1);
	const d = points
		.map((p, i) => `${i === 0 ? "M" : "L"}${i * step},${h - (p / 100) * h + 1}`)
		.join(" ");
	const last = points[points.length - 1];
	const color =
		last >= 85 ? "var(--accent)" : last >= 60 ? "var(--warn)" : "var(--bad)";
	return (
		<svg width={w} height={h + 2} aria-hidden="true">
			<path d={d} fill="none" stroke={color} strokeWidth="1.5" />
		</svg>
	);
}

export function CopyField({ value }: { value: string }) {
	const [copied, setCopied] = useState(false);
	return (
		<div className="copy">
			<code>{value}</code>
			<button
				type="button"
				className="btn"
				onClick={() => {
					navigator.clipboard?.writeText(value).catch(() => {});
					setCopied(true);
					setTimeout(() => setCopied(false), 1200);
				}}
			>
				{copied ? "Copied" : "Copy"}
			</button>
		</div>
	);
}

export function Segmented<T extends string>({
	value,
	options,
	onChange,
}: {
	value: T;
	options: { value: T; label: string }[];
	onChange: (v: T) => void;
}) {
	return (
		<div className="seg">
			{options.map((o) => (
				<button
					type="button"
					key={o.value}
					className={o.value === value ? "on" : ""}
					onClick={() => onChange(o.value)}
				>
					{o.label}
				</button>
			))}
		</div>
	);
}

export function PageHead({
	title,
	sub,
	actions,
}: {
	title: string;
	sub: string;
	actions?: React.ReactNode;
}) {
	return (
		<div className="page-head">
			<div>
				<h1>{title}</h1>
				<p>{sub}</p>
			</div>
			{actions && <div className="row">{actions}</div>}
		</div>
	);
}
