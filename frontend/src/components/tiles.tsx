import type { ReactNode } from "react";

/** A numbered builder step on a glass panel. */
export function Step({
	n,
	title,
	children,
}: {
	n: number;
	title: string;
	children: ReactNode;
}) {
	return (
		<section className="glass step">
			<div className="step-head">
				<span className="step-n">{String(n).padStart(2, "0")}</span>
				<h2>{title}</h2>
			</div>
			{children}
		</section>
	);
}

/** A large pickable choice with an illustration slot. */
export function Tile({
	on,
	icon,
	cap,
	sub,
	onClick,
}: {
	on: boolean;
	icon: ReactNode;
	cap: string;
	sub?: ReactNode;
	onClick: () => void;
}) {
	return (
		<button
			type="button"
			className={`tile${on ? " on" : ""}`}
			onClick={onClick}
			aria-pressed={on}
		>
			<span className="tile-art">{icon}</span>
			<span className="tile-cap">{cap}</span>
			{sub && <span className="tile-sub">{sub}</span>}
		</button>
	);
}

const DOT_KEYS = ["d0", "d1", "d2", "d3", "d4"];

/** Five dots: one colour for sticky, many for rotating. */
export function Dots({ colors }: { colors: string[] }) {
	return (
		<span className="ip-dots" aria-hidden="true">
			{DOT_KEYS.map((k, i) => (
				<i
					key={k}
					style={{
						background: colors[i % colors.length],
						boxShadow: `0 0 6px ${colors[i % colors.length]}`,
					}}
				/>
			))}
		</span>
	);
}

export function Field({
	id,
	label,
	v,
	ph,
}: {
	id: string;
	label: string;
	v: string;
	ph?: string;
}) {
	return (
		<div className="field">
			<label htmlFor={id}>{label}</label>
			<input id={id} className="input mono" defaultValue={v} placeholder={ph} />
		</div>
	);
}
