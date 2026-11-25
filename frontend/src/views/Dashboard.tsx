import { Mark } from "../components/icons";
import { Sparkline } from "../components/ui";
import {
	ClientLogo,
	Flow,
	Ring,
	StatusAvatar,
	USE_LABEL,
} from "../components/visual";
import { ACCOUNT, ROUTES, type RouteRow } from "../lib/mock";
import { PIN_LOSS_LABEL, usd } from "../lib/policy";

const TARGET: Record<RouteRow["useCase"], string> = {
	mautrix: "wa",
	browser: "web",
	scrape: "shop",
};

export function Dashboard() {
	const totalGb = ROUTES.reduce((s, r) => s + r.gb30d, 0);
	const live = ROUTES.reduce((s, r) => s + r.sessions, 0);
	const pins = ROUTES.filter((r) => r.useCase === "mautrix").length;
	const lost = ROUTES.filter((r) => r.status === "pin_lost");

	return (
		<>
			<div className="page-head">
				<div>
					<div className="eyebrow">Routes</div>
					<h1>Every client, a human exit.</h1>
				</div>
				<a href="#/new" className="btn btn-primary">
					New route
				</a>
			</div>

			<div className="hero-stats">
				<Stat value={usd(ACCOUNT.spend30d, 0)} label="spend, 30 days" />
				<Stat value={`${totalGb.toFixed(0)} GB`} label="through exits" />
				<Stat value={String(live)} label="live sessions" />
				<Stat value={String(pins)} label="long-lived pins" />
				<Stat value="18" label="median fraud score" />
			</div>

			{lost.map((r) => (
				<div key={r.id} className="glass alert">
					<span className="avatar avatar-bad" style={{ width: 44, height: 44 }}>
						<Mark name="broken-robot" size={28} label="Pin lost" />
					</span>
					<div className="alert-text">
						<b>{r.name} lost its pinned exit 14 minutes ago.</b>
						<span>
							{r.asn}, {r.geo}. Traffic is refused until you pick a replacement.
							The webhook has fired.
						</span>
					</div>
					<button type="button" className="btn btn-primary">
						Replace in AS3320
					</button>
					<button type="button" className="btn">
						Retry original
					</button>
				</div>
			))}

			<div className="route-grid">
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
		<article className={`glass route-card ${r.status}`}>
			<header className="route-head">
				<StatusAvatar route={r} />
				<div className="route-title">
					<b>{r.name}</b>
					<span>
						<ClientLogo use={r.useCase} size={14} />
						{USE_LABEL[r.useCase]} · {r.mode} · {r.geo}
					</span>
				</div>
				<span
					className={`pill ${lost ? "pill-bad" : r.status === "degraded" ? "pill-warn" : "pill-good"}`}
				>
					{lost ? "Pin lost" : r.status === "degraded" ? "Degraded" : "Healthy"}
				</span>
			</header>

			<Flow
				use={r.useCase}
				exit={r.exitType}
				target={TARGET[r.useCase]}
				mode={r.mode}
				broken={lost}
				caption={r.pinnedFor ? `${r.exitIp} · ${r.pinnedFor}` : r.exitIp}
			/>

			<footer className="route-foot">
				<div className="row" style={{ gap: 18 }}>
					<Ring value={lost ? null : r.health} label="health" />
					<Ring value={lost ? null : r.fraudScore} label="fraud" invert />
					<Sparkline points={r.spark} />
				</div>
				<div className="route-spend">
					<span className="num-m">{usd(r.spend30d)}</span>
					<span className="faint">{r.gb30d.toFixed(1)} GB · 30d</span>
				</div>
			</footer>

			<details className="dig">
				<summary>
					via {r.provider}
					{r.pinLoss
						? ` · on loss: ${PIN_LOSS_LABEL[r.pinLoss].toLowerCase()}`
						: ""}
				</summary>
				<div className="dig-body kvs">
					<span>Route ID</span>
					<b className="mono">{r.id}</b>
					<span>ASN</span>
					<b className="mono">{r.asn}</b>
					<span>Risk dial</span>
					<b className="mono">{r.risk}</b>
					<span>Live sessions</span>
					<b className="mono">{r.sessions}</b>
				</div>
			</details>
		</article>
	);
}

function Stat({ value, label }: { value: string; label: string }) {
	return (
		<div className="stat">
			<span className="num-l">{value}</span>
			<span className="faint">{label}</span>
		</div>
	);
}
