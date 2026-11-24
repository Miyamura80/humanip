import { useState } from "react";

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
