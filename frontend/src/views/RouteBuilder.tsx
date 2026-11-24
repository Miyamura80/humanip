import { useMemo, useState } from "react";
import { Dots, Field, Label, Step, Tile } from "../components/tiles";
import {
	EXIT_ICON,
	Flow,
	Gauge,
	MODE_ICON,
	Pips,
	USE_ICON,
} from "../components/visual";
import { effectivePrice, eligibleOffers } from "../lib/mock";
import {
	EXIT_LABEL,
	type ExitType,
	formatSticky,
	type PinLoss,
	type RouteMode,
	resolveRisk,
	riskLabel,
	usd,
	withTake,
} from "../lib/policy";

type UseCase = keyof typeof USE_ICON;

const PRESET: Record<
	UseCase,
	{ mode: RouteMode; ttl: string; exits: ExitType[]; risk: number; gb: number }
> = {
	browser: {
		mode: "sticky",
		ttl: "session",
		exits: ["residential"],
		risk: 40,
		gb: 20,
	},
	mautrix: {
		mode: "sticky",
		ttl: "forever",
		exits: ["mobile", "isp"],
		risk: 8,
		gb: 1,
	},
	scrape: {
		mode: "rotating",
		ttl: "session",
		exits: ["residential"],
		risk: 65,
		gb: 200,
	},
};
const USE_CAP: Record<UseCase, string> = {
	browser: "Browser agent",
	mautrix: "Bridge",
	scrape: "Scraper",
};

const TTL: { v: string; icon: string; cap: string; min: number }[] = [
	{ v: "session", icon: "⏳", cap: "session", min: 30 },
	{ v: "24h", icon: "🌙", cap: "1 day", min: 24 * 60 },
	{ v: "30d", icon: "📅", cap: "30 days", min: 30 * 24 * 60 },
	{ v: "forever", icon: "♾️", cap: "forever", min: 90 * 24 * 60 },
];
const LOSS: { v: PinLoss; icon: string; cap: string }[] = [
	{ v: "fail_closed", icon: "🛑", cap: "stop + alert" },
	{ v: "same_asn", icon: "🏢", cap: "same ASN" },
	{ v: "same_city", icon: "🏙️", cap: "same city" },
	{ v: "same_country", icon: "🏳️", cap: "same country" },
	{ v: "any", icon: "🎲", cap: "anything" },
];
const EXIT_COST: Record<ExitType, number> = {
	mobile: 3,
	isp: 2,
	residential: 1,
};

export function RouteBuilder() {
	const [use, setUse] = useState<UseCase>("mautrix");
	const p = PRESET[use];
	const [mode, setMode] = useState<RouteMode>(p.mode);
	const [ttl, setTtl] = useState(p.ttl);
	const [loss, setLoss] = useState<PinLoss>("fail_closed");
	const [exits, setExits] = useState<ExitType[]>(p.exits);
	const [risk, setRisk] = useState(p.risk);
	const [gb, setGb] = useState(p.gb);

	const pick = (u: UseCase) => {
		const n = PRESET[u];
		setUse(u);
		setMode(n.mode);
		setTtl(n.ttl);
		setExits(n.exits);
		setRisk(n.risk);
		setGb(n.gb);
	};
	const toggle = (e: ExitType) =>
		setExits((c) =>
			c.includes(e) ? (c.length > 1 ? c.filter((x) => x !== e) : c) : [...c, e],
		);

	const resolved = useMemo(() => resolveRisk(risk, exits), [risk, exits]);
	const stickyMin =
		mode === "sticky" ? (TTL.find((t) => t.v === ttl)?.min ?? 0) : 0;
	const offers = useMemo(
		() =>
			eligibleOffers(resolved, stickyMin).sort(
				(a, b) =>
					effectivePrice(a, "web.whatsapp.com") -
					effectivePrice(b, "web.whatsapp.com"),
			),
		[resolved, stickyMin],
	);
	const best = offers[0];

	// 0-4 pips per risk dimension: more pips = more risk taken.
	const dims = [
		{
			icon: "🛡️",
			k: "fraud ≤",
			v: String(resolved.maxFraudScore),
			n: Math.round(((resolved.maxFraudScore - 15) / 60) * 4),
		},
		{
			icon: "⬇️",
			k: "exits",
			v: resolved.allowedExitTypes.map((e) => EXIT_ICON[e]).join(""),
			n: resolved.allowedExitTypes.length - exits.length + 1,
		},
		{
			icon: "🏷️",
			k: "sourcing",
			v:
				resolved.sourcingTiers.length === 1
					? "verified"
					: `${resolved.sourcingTiers.length} tiers`,
			n: resolved.sourcingTiers.length,
		},
		{
			icon: "⏱️",
			k: "sticky ≥",
			v: formatSticky(resolved.minStickyMinutes),
			n:
				resolved.minStickyMinutes >= 1440
					? 1
					: resolved.minStickyMinutes
						? 2
						: 4,
		},
	];

	return (
		<>
			<div className="page-head">
				<h1>➕ New route</h1>
			</div>
			<div className="builder">
				<div className="stack">
					<Step n={1} title="Who connects?">
						<div
							className="tiles"
							style={{ gridTemplateColumns: "repeat(3, 1fr)" }}
						>
							{(Object.keys(USE_ICON) as UseCase[]).map((u) => (
								<Tile
									key={u}
									on={use === u}
									icon={USE_ICON[u]}
									cap={USE_CAP[u]}
									onClick={() => pick(u)}
								/>
							))}
						</div>
					</Step>

					<Step n={2} title="Same IP or fresh IPs?">
						<div className="tiles" style={{ gridTemplateColumns: "1fr 1fr" }}>
							<Tile
								on={mode === "sticky"}
								icon={MODE_ICON.sticky}
								cap="Sticky"
								onClick={() => setMode("sticky")}
							>
								<Dots
									colors={[
										"var(--accent)",
										"var(--accent)",
										"var(--accent)",
										"var(--accent)",
										"var(--accent)",
									]}
								/>
							</Tile>
							<Tile
								on={mode === "rotating"}
								icon={MODE_ICON.rotating}
								cap="Rotating"
								onClick={() => setMode("rotating")}
							>
								<Dots
									colors={[
										"var(--accent)",
										"var(--isp)",
										"var(--mobile)",
										"var(--warn)",
										"var(--bad)",
									]}
								/>
							</Tile>
						</div>
						{mode === "sticky" && (
							<>
								<Label>⏱️ hold for</Label>
								<div
									className="tiles"
									style={{ gridTemplateColumns: "repeat(4, 1fr)" }}
								>
									{TTL.map((t) => (
										<Tile
											key={t.v}
											small
											on={ttl === t.v}
											icon={t.icon}
											cap={t.cap}
											onClick={() => setTtl(t.v)}
										/>
									))}
								</div>
								<Label>💥 if the IP dies</Label>
								<div
									className="tiles"
									style={{ gridTemplateColumns: "repeat(5, 1fr)" }}
								>
									{LOSS.map((l) => (
										<Tile
											key={l.v}
											small
											on={loss === l.v}
											icon={l.icon}
											cap={l.cap}
											onClick={() => setLoss(l.v)}
										/>
									))}
								</div>
							</>
						)}
					</Step>

					<Step n={3} title="Exit type & place">
						<div
							className="tiles"
							style={{ gridTemplateColumns: "repeat(3, 1fr)" }}
						>
							{(Object.keys(EXIT_ICON) as ExitType[]).map((e) => (
								<Tile
									key={e}
									on={exits.includes(e)}
									icon={EXIT_ICON[e]}
									cap={EXIT_LABEL[e]}
									onClick={() => toggle(e)}
								>
									<span className="mono" style={{ color: "var(--warn)" }}>
										{"$".repeat(EXIT_COST[e])}
										<span className="faint">
											{"$".repeat(3 - EXIT_COST[e])}
										</span>
									</span>
								</Tile>
							))}
						</div>
						<div className="row" style={{ marginTop: 12, flexWrap: "wrap" }}>
							<span className="chip on">🇩🇪 Berlin</span>
							<span className="chip on">🏢 AS3320</span>
							<span className="chip on">📞 match +49</span>
							<span className="chip">＋ geo</span>
						</div>
					</Step>

					<Step n={4} title="Risk for price">
						<div className="row" style={{ gap: 24, alignItems: "center" }}>
							<div style={{ width: 240, textAlign: "center" }}>
								<Gauge value={risk} />
								<div className="big-cost" style={{ marginTop: -6 }}>
									{risk}
								</div>
								<div className="faint">{riskLabel(risk)}</div>
								<input
									type="range"
									min={0}
									max={100}
									value={risk}
									className="risk-slider"
									aria-label="Risk"
									onChange={(e) => setRisk(Number(e.target.value))}
								/>
							</div>
							<div style={{ flex: 1 }}>
								{dims.map((d) => (
									<div
										key={d.k}
										className="kv"
										style={{ alignItems: "center" }}
									>
										<span>
											{d.icon} <span className="muted">{d.k}</span> <b>{d.v}</b>
										</span>
										<Pips
											n={Math.max(0, Math.min(4, d.n))}
											color={
												d.n >= 3
													? "var(--bad)"
													: d.n === 2
														? "var(--warn)"
														: "var(--accent)"
											}
										/>
									</div>
								))}
							</div>
						</div>
					</Step>

					<details className="dig card card-pad">
						<summary>
							⚙️ name · target URL · 💸 cap · 🔔 webhook · overrides
						</summary>
						<div className="dig-body grid-2">
							<Field id="n" label="name" v="mautrix-whatsapp-eu" />
							<Field
								id="u"
								label="🎯 target URL"
								v="https://web.whatsapp.com"
							/>
							<Field id="c" label="💸 monthly cap" v="$25.00" />
							<Field
								id="w"
								label="🔔 webhook"
								v="https://ops.bridge.example/hooks"
							/>
							<Field
								id="f"
								label="🛡️ max_fraud_score"
								v=""
								ph={String(resolved.maxFraudScore)}
							/>
							<Field
								id="s"
								label="⏱️ min_sticky"
								v=""
								ph={formatSticky(resolved.minStickyMinutes)}
							/>
						</div>
					</details>
				</div>

				<div className="sticky-col stack">
					<div className="card card-pad" style={{ textAlign: "center" }}>
						<div className="big-cost">
							{usd(best ? withTake(best.pricePerGb * gb) : 0)}
						</div>
						<div className="muted">
							/ mo ·{" "}
							<input
								className="input mono"
								style={{
									width: 52,
									height: 24,
									display: "inline",
									padding: "0 6px",
								}}
								value={gb}
								aria-label="GB per month"
								onChange={(e) => setGb(Number(e.target.value) || 0)}
							/>{" "}
							GB
						</div>
						<div style={{ margin: "18px 0 6px" }}>
							<Flow
								from={USE_ICON[use]}
								exit={best?.exitType ?? exits[0]}
								ip={best?.provider ?? "no match"}
								mode={mode}
							/>
						</div>
						<div
							className="row"
							style={{
								justifyContent: "center",
								gap: 16,
								margin: "6px 0 14px",
							}}
						>
							<span title="providers that fit">🧮 {offers.length}/9</span>
							<span title="pool fraud">🛡️ {best?.fraudScore ?? "–"}</span>
							<span title="fee">➕5%</span>
						</div>
						<button
							type="button"
							className="btn btn-primary"
							style={{ width: "100%", justifyContent: "center" }}
						>
							Create route
						</button>
					</div>
					<details className="dig card card-pad">
						<summary>📄 YAML</summary>
						<pre className="code dig-body">{`route: mautrix-whatsapp-eu
mode: ${mode}
${mode === "sticky" ? `sticky: { ttl: ${ttl}, on_pin_loss: ${loss} }\n` : ""}exit_types: [${exits.join(", ")}]
geo: { country: DE, city: Berlin, asn: 3320,
       match_account: true }
risk: ${risk}`}</pre>
					</details>
				</div>
			</div>
		</>
	);
}
