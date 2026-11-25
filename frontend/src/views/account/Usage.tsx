// 30-day usage: total spend, then per-provider and per-domain bar lists.

import type { AppUsageRow } from "../../api/app";
import { Mark } from "../../components/icons";
import { usd } from "../../lib/policy";

const bytesLabel = (n: number) => {
	const units = ["B", "KB", "MB", "GB", "TB"];
	let v = n;
	let i = 0;
	while (v >= 1000 && i < units.length - 1) {
		v /= 1000;
		i += 1;
	}
	return `${v.toFixed(v >= 100 || i === 0 ? 0 : 1)} ${units[i]}`;
};

interface Group {
	name: string;
	bytes: number;
	usd: number;
}

function groupBy(rows: AppUsageRow[], key: "provider" | "domain"): Group[] {
	const m = new Map<string, Group>();
	for (const r of rows) {
		const name = r[key] || "unknown";
		const g = m.get(name) ?? { name, bytes: 0, usd: 0 };
		g.bytes += r.bytes;
		g.usd += r.total_usd;
		m.set(name, g);
	}
	return [...m.values()].sort((a, b) => b.usd - a.usd || b.bytes - a.bytes);
}

function BarList({ title, groups }: { title: string; groups: Group[] }) {
	const top = groups.slice(0, 6);
	const max = Math.max(...top.map((g) => g.usd), 0.000001);
	return (
		<div>
			<div className="sub-label" style={{ marginTop: 0 }}>
				{title}
			</div>
			<ul className="bar-list">
				{top.map((g) => (
					<li key={g.name}>
						<div className="row spread">
							<span className="bar-name">{g.name}</span>
							<span className="mono">
								<span className="faint">{bytesLabel(g.bytes)}</span>{" "}
								{usd(g.usd)}
							</span>
						</div>
						<span className="bar">
							<i style={{ width: `${Math.max((g.usd / max) * 100, 2)}%` }} />
						</span>
					</li>
				))}
			</ul>
		</div>
	);
}

export function UsagePanel({
	rows,
	total,
	loading,
	error,
}: {
	rows: AppUsageRow[];
	total?: number;
	loading: boolean;
	error?: string;
}) {
	const bytes = rows.reduce((s, r) => s + r.bytes, 0);
	return (
		<section className="glass step">
			<div className="section-head">
				<Mark name="reranking" size={20} />
				<h2>Usage, last 30 days</h2>
			</div>
			{error && <p className="form-error">Could not load usage: {error}</p>}
			{loading && total === undefined && (
				<p className="faint">Loading usage...</p>
			)}
			{total !== undefined && (
				<div className="row usage-total">
					<div className="num-l">{usd(total)}</div>
					<span className="muted">{bytesLabel(bytes)} through the gateway</span>
				</div>
			)}
			{total !== undefined && !rows.length && (
				<div className="empty">
					<Mark name="tunnel" size={28} />
					<span>No traffic yet. Connect a client and usage shows up here.</span>
				</div>
			)}
			{rows.length > 0 && (
				<div className="grid-2" style={{ marginBottom: 0, marginTop: 22 }}>
					<BarList title="By provider" groups={groupBy(rows, "provider")} />
					<BarList title="By domain" groups={groupBy(rows, "domain")} />
				</div>
			)}
		</section>
	);
}
