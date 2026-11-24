import { ExitBadge, Meter, PageHead, Sparkline } from "../components/ui";
import { ACCOUNT, ROUTES, type RouteRow } from "../lib/mock";
import { PIN_LOSS_LABEL, usd } from "../lib/policy";

const STATUS: Record<RouteRow["status"], { cls: string; label: string }> = {
	healthy: { cls: "badge-good", label: "Healthy" },
	degraded: { cls: "badge-warn", label: "Degraded" },
	pin_lost: { cls: "badge-bad", label: "Pin lost" },
};

const USE_CASE: Record<RouteRow["useCase"], string> = {
	browser: "Browser session",
	mautrix: "Bridge",
	scrape: "Scrape",
};

export function Dashboard() {
	const lost = ROUTES.filter((r) => r.status === "pin_lost");
	const totalGb = ROUTES.reduce((s, r) => s + r.gb30d, 0);
	const live = ROUTES.reduce((s, r) => s + r.sessions, 0);
	const pinned = ROUTES.filter((r) => r.mode === "sticky" && r.pinnedFor);

	return (
		<>
			<PageHead
				title="Routes"
				sub="Every client connects through a route. Each route's policy decides its exit."
				actions={
					<>
						<button type="button" className="btn">
							Export usage
						</button>
						<a href="#/new" className="btn btn-primary">
							+ New route
						</a>
					</>
				}
			/>

			{lost.map((r) => (
				<div className="callout" key={r.id}>
					<span className="badge badge-bad">
						<span className="dot" />
						Fail closed
					</span>
					<span className="grow">
						<b>{r.name}</b> lost its pinned exit ({r.asn}, {r.geo}) 14m ago.
						Connections are refused until you choose a replacement. Webhook sent
						to <span className="mono">ops@bridge.example</span>.
					</span>
					<button type="button" className="btn">
						Pick replacement in AS3320
					</button>
					<button type="button" className="btn">
						Retry original
					</button>
				</div>
			))}

			<div className="stats">
				<Stat
					label="Spend · 30d"
					value={usd(ACCOUNT.spend30d)}
					sub="incl. 5% fee"
				/>
				<Stat
					label="Bandwidth · 30d"
					value={`${totalGb.toFixed(1)} GB`}
					sub="across 5 routes"
				/>
				<Stat
					label="Live sessions"
					value={String(live)}
					sub={`${pinned.length} long-lived pins`}
				/>
				<Stat
					label="Median fraud score"
					value="18"
					sub="IPQS + Scamalytics blend"
				/>
			</div>

			<div className="card">
				<table className="tbl">
					<thead>
						<tr>
							<th>Route</th>
							<th>Policy</th>
							<th>Exit</th>
							<th>Current IP / pool</th>
							<th>Health · 24h</th>
							<th>Fraud</th>
							<th className="num">GB · 30d</th>
							<th className="num">Spend</th>
							<th>Status</th>
						</tr>
					</thead>
					<tbody>
						{ROUTES.map((r) => (
							<tr key={r.id}>
								<td>
									<div style={{ fontWeight: 600 }}>{r.name}</div>
									<div className="faint mono" style={{ fontSize: 11 }}>
										{USE_CASE[r.useCase]} · {r.id}
									</div>
								</td>
								<td>
									<div>
										{r.mode === "sticky" ? "Sticky" : "Rotating"}
										<span className="faint"> · risk {r.risk}</span>
									</div>
									{r.pinLoss && (
										<div className="faint" style={{ fontSize: 11 }}>
											{PIN_LOSS_LABEL[r.pinLoss]}
										</div>
									)}
								</td>
								<td>
									<ExitBadge type={r.exitType} />
									<div className="faint" style={{ fontSize: 11, marginTop: 3 }}>
										via {r.provider}
									</div>
								</td>
								<td>
									<div className="mono">{r.exitIp}</div>
									<div className="faint" style={{ fontSize: 11 }}>
										{r.geo}
										{r.pinnedFor && ` · pinned ${r.pinnedFor}`}
									</div>
								</td>
								<td>
									<div className="row" style={{ gap: 8 }}>
										<Sparkline points={r.spark} />
										<span className="mono">{r.health}</span>
									</div>
								</td>
								<td>
									{r.status === "pin_lost" ? (
										<span className="faint">n/a</span>
									) : (
										<Meter value={r.fraudScore} invert />
									)}
								</td>
								<td className="num mono">{r.gb30d.toFixed(2)}</td>
								<td className="num mono">{usd(r.spend30d)}</td>
								<td>
									<span className={`badge ${STATUS[r.status].cls}`}>
										<span className="dot" />
										{STATUS[r.status].label}
									</span>
								</td>
							</tr>
						))}
					</tbody>
				</table>
			</div>
		</>
	);
}

function Stat({
	label,
	value,
	sub,
}: {
	label: string;
	value: string;
	sub: string;
}) {
	return (
		<div className="card card-pad">
			<div className="stat-label">{label}</div>
			<div className="stat-value">{value}</div>
			<div className="stat-sub">{sub}</div>
		</div>
	);
}
