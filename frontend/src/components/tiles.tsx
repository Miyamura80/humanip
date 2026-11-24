export function Step({
	n,
	title,
	children,
}: {
	n: number;
	title: string;
	children: React.ReactNode;
}) {
	return (
		<div className="card card-pad">
			<div className="row" style={{ marginBottom: 12, gap: 8 }}>
				<span className="brand-mark" style={{ width: 20, height: 20 }}>
					{n}
				</span>
				<b>{title}</b>
			</div>
			{children}
		</div>
	);
}

export function Tile({
	on,
	icon,
	cap,
	small,
	onClick,
	children,
}: {
	on: boolean;
	icon: string;
	cap: string;
	small?: boolean;
	onClick: () => void;
	children?: React.ReactNode;
}) {
	return (
		<button
			type="button"
			className={`tile${on ? " on" : ""}`}
			onClick={onClick}
			style={small ? { padding: "10px 4px" } : undefined}
		>
			<span className="big-icon" style={small ? { fontSize: 20 } : undefined}>
				{icon}
			</span>
			<span className="tile-cap">{cap}</span>
			{children}
		</button>
	);
}

export function Dots({ colors }: { colors: string[] }) {
	return (
		<span className="ip-dots" aria-hidden="true">
			{colors.map((c, i) => (
				// biome-ignore lint/suspicious/noArrayIndexKey: static illustration
				<i key={i} style={{ background: c }} />
			))}
		</span>
	);
}

export const Label = ({ children }: { children: React.ReactNode }) => (
	<div className="field-label" style={{ margin: "14px 0 8px" }}>
		{children}
	</div>
);

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
