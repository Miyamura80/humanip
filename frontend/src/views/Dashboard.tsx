import { Sparkline } from "../components/ui";
import { Flow, MODE_ICON, Ring, USE_ICON } from "../components/visual";
import { ACCOUNT, ROUTES, type RouteRow } from "../lib/mock";
import { PIN_LOSS_LABEL, usd } from "../lib/policy";

export function Dashboard() {
	const totalGb = ROUTES.reduce((s, r) => s + r.gb30d, 0);
	const live = ROUTES.reduce((s, r) => s + r.sessions, 0);
	const pins = ROUTES.filter(
		(r) => r.mode === "sticky" && r.useCase === "mautrix",
	).length;

	return (
		<>
			<div className="page-head">
				<h1>🗺️ Routes</h1>
				<a href="#/new" className="btn btn-primary">
					➕ New route
				</a>
			</div>

			<div className="stat-row">
				<Chip icon="💸" value={usd(ACCOUNT.spend30d, 0)} sub="30d" />
				<Chip icon="📦" value={`${totalGb.toFixed(0)} GB`} sub="30d" />
				<Chip icon="🔗" value={String(live)} sub="live" />
				<Chip icon="📌" value={String(pins)} sub="pins" />
				<Chip icon="🛡️" value="18" sub="median fraud" />
			</div>

			<div className="cards">
				{ROUTES.map((r) => (
					<RouteCard key={r.id} r={r} />
				))}
			</div>
		</>
	);
}

function RouteCard({ r }: { r: RouteRow }) {
	const lost = r.status === "pin_lost";
	return (
		<div className={`rcard${lost ? " lost" : ""}`}>
			<div className="rcard-head">
				<span className="big-icon" title={r.useCase}>
					{USE_ICON[r.useCase]}
				</span>
				<div style={{ flex: 1, minWidth: 0 }}>
					<div className="rcard-name">{r.name}</div>
					<div className="faint" style={{ fontSize: 11 }}>
						{MODE_ICON[r.mode]} {r.mode} · {r.geo}
					</div>
				</div>
				{lost ? (
					<span className="badge badge-bad">✖ pin lost</span>
				) : r.status === "degraded" ? (
					<span className="badge badge-warn">▲ degraded</span>
				) : (
					<span className="badge badge-good">● ok</span>
				)}
			</div>

			<Flow
				from={USE_ICON[r.useCase]}
				exit={r.exitType}
				ip={r.pinnedFor ? `${r.exitIp} · ${r.pinnedFor}` : r.exitIp}
				mode={r.mode}
				broken={lost}
			/>

			{lost ? (
				<div className="row">
					<button type="button" className="btn btn-primary">
						🔁 Replace in AS3320
					</button>
					<button type="button" className="btn">
						↺ Retry
					</button>
				</div>
			) : (
				<div className="rcard-foot">
					<div className="row" style={{ gap: 14 }}>
						<Ring value={r.health} label="health" />
						<Ring value={r.fraudScore} label="fraud" invert />
						<Sparkline points={r.spark} />
					</div>
					<div style={{ textAlign: "right" }}>
						<div className="money">{usd(r.spend30d)}</div>
						<div className="faint mono" style={{ fontSize: 11 }}>
							{r.gb30d.toFixed(1)} GB
						</div>
					</div>
				</div>
			)}

			<details className="dig">
				<summary>
					via {r.provider}
					{r.pinLoss &&
						` · on loss: ${PIN_LOSS_LABEL[r.pinLoss].toLowerCase()}`}
				</summary>
				<div className="dig-body">
					<KV k="Route ID" v={r.id} />
					<KV k="ASN" v={r.asn} />
					<KV k="Risk dial" v={String(r.risk)} />
					<KV k="Live sessions" v={String(r.sessions)} />
					{lost && <KV k="Alert" v="webhook → ops@bridge.example" />}
				</div>
			</details>
		</div>
	);
}

function KV({ k, v }: { k: string; v: string }) {
	return (
		<div className="kv">
			<span className="muted">{k}</span>
			<span className="mono">{v}</span>
		</div>
	);
}

function Chip({
	icon,
	value,
	sub,
}: {
	icon: string;
	value: string;
	sub: string;
}) {
	return (
		<div className="stat-chip" title={sub}>
			<span>{icon}</span>
			{value}
			<small>{sub}</small>
		</div>
	);
}
