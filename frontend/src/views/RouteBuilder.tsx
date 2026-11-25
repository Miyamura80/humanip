import { useMemo, useState } from "react";
import { ReliabilityBadge } from "../components/frontier";
import { Logo, Mark, type MarkName } from "../components/icons";
import { OrbitalFrontier } from "../components/orbit";
import { PolicyYaml, policyYaml } from "../components/policy-yaml";
import { Dots, Field, Step, Tile } from "../components/tiles";
import {
	ExitMark,
	Flow,
	Gauge,
	Pips,
	USE_LABEL,
	type UseCase,
} from "../components/visual";
import {
	eligibleOffers,
	OFFERS,
	type ProviderOffer,
	successRate,
} from "../lib/mock";
import { axis, defaultPick, RELIABILITY_FLOOR } from "../lib/pareto";
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
import LiquidMetal from "../vendor/LiquidMetal";

const PRESET: Record<
	UseCase,
	{ mode: RouteMode; ttl: string; exits: ExitType[]; gb: number }
> = {
	browser: { mode: "sticky", ttl: "session", exits: ["residential"], gb: 20 },
	mautrix: { mode: "sticky", ttl: "forever", exits: ["mobile", "isp"], gb: 1 },
	scrape: { mode: "rotating", ttl: "session", exits: ["residential"], gb: 200 },
};
const USE_LOGO: Record<UseCase, "claude-code" | "beeper" | "apify"> = {
	browser: "claude-code",
	mautrix: "beeper",
	scrape: "apify",
};
const NAME: Record<UseCase, string> = {
	browser: "agent-browser-us",
	mautrix: "mautrix-whatsapp-eu",
	scrape: "price-scrape",
};
const TARGET: Record<UseCase, string> = {
	browser: "linkedin.com",
	mautrix: "web.whatsapp.com",
	scrape: "amazon.com",
};

const TTL = [
	{ v: "session", big: "30m", cap: "per session", min: 30 },
	{ v: "24h", big: "1d", cap: "one day", min: 24 * 60 },
	{ v: "30d", big: "30d", cap: "thirty days", min: 30 * 24 * 60 },
	{ v: "forever", big: "∞", cap: "hard pin", min: 90 * 24 * 60 },
];
const LOSS: { v: PinLoss; mark: MarkName; cap: string }[] = [
	{ v: "fail_closed", mark: "shield-x", cap: "Stop and alert" },
	{ v: "same_asn", mark: "exit-isp", cap: "Same ASN" },
	{ v: "same_city", mark: "exit-residential", cap: "Same city" },
	{ v: "same_country", mark: "shield", cap: "Same country" },
	{ v: "any", mark: "shield-off", cap: "Anything" },
];
const EXIT_COST: Record<ExitType, number> = {
	mobile: 3,
	isp: 2,
	residential: 1,
};
const DEFAULT_RISK = 20;

export function RouteBuilder() {
	const [use, setUse] = useState<UseCase>("mautrix");
	const [mode, setMode] = useState<RouteMode>(PRESET.mautrix.mode);
	const [ttl, setTtl] = useState(PRESET.mautrix.ttl);
	const [loss, setLoss] = useState<PinLoss>("fail_closed");
	const [exits, setExits] = useState<ExitType[]>(PRESET.mautrix.exits);
	const [risk, setRisk] = useState(DEFAULT_RISK);
	const [gb, setGb] = useState(PRESET.mautrix.gb);
	const [floor, setFloor] = useState(RELIABILITY_FLOOR);
	const [manual, setManual] = useState<ProviderOffer>();

	const choose = (u: UseCase) => {
		const n = PRESET[u];
		setUse(u);
		setMode(n.mode);
		setTtl(n.ttl);
		setExits(n.exits);
		setGb(n.gb);
		setManual(undefined);
	};
	const toggle = (e: ExitType) =>
		setExits((c) =>
			c.includes(e) ? (c.length > 1 ? c.filter((x) => x !== e) : c) : [...c, e],
		);

	const stickyMin =
		mode === "sticky" ? (TTL.find((t) => t.v === ttl)?.min ?? 0) : 0;
	// The route's own pin length is the sticky requirement; the dial only
	// tunes fraud, exit downgrade and sourcing.
	const resolved = useMemo(
		() => resolveRisk(risk, exits, { minStickyMinutes: stickyMin }),
		[risk, exits, stickyMin],
	);
	const domain = TARGET[use];
	const offers = useMemo(
		() => eligibleOffers(resolved, stickyMin),
		[resolved, stickyMin],
	);
	const eligible = useMemo(() => new Set(offers), [offers]);
	const auto = useMemo(
		() => defaultPick(offers, domain, floor),
		[offers, domain, floor],
	);
	const best = manual && eligible.has(manual) ? manual : auto;
	const edited = risk !== DEFAULT_RISK || floor !== RELIABILITY_FLOOR;
	const works = best ? successRate(domain, best.provider) : undefined;

	const dims: { mark: MarkName; k: string; v: string; n: number }[] = [
		{
			mark: "shield",
			k: "Fraud score at most",
			v: String(resolved.maxFraudScore),
			n: Math.round(((resolved.maxFraudScore - 15) / 60) * 4),
		},
		{
			mark: "exit-residential",
			k: "Exit classes",
			v: resolved.allowedExitTypes.map((e) => EXIT_LABEL[e]).join(", "),
			n: resolved.allowedExitTypes.length - exits.length + 1,
		},
		{
			mark: "shield-user",
			k: "Provider sourcing",
			v: resolved.sourcingTiers.join(", "),
			n: resolved.sourcingTiers.length,
		},
		{
			mark: "tunnel",
			k: "Pin guarantee",
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
				<div>
					<div className="eyebrow">New route</div>
					<h1>Shape where a client exits.</h1>
				</div>
			</div>
			<div className="builder">
				<div className="stack">
					<Step n={1} title="Who connects?">
						<div className="tiles cols-3">
							{(Object.keys(PRESET) as UseCase[]).map((u) => (
								<Tile
									key={u}
									on={use === u}
									icon={
										<Logo name={USE_LOGO[u]} size={34} label={USE_LABEL[u]} />
									}
									cap={USE_LABEL[u]}
									onClick={() => choose(u)}
								/>
							))}
						</div>
					</Step>

					<Step n={2} title="Same IP, or fresh ones?">
						<div className="tiles cols-2">
							<Tile
								on={mode === "sticky"}
								icon={<Dots colors={["#7fdfff"]} />}
								cap="Sticky"
								sub="one exit, held"
								onClick={() => setMode("sticky")}
							/>
							<Tile
								on={mode === "rotating"}
								icon={
									<Dots
										colors={[
											"#7fdfff",
											"#c9b8ff",
											"#8af0c8",
											"#ffd479",
											"#ff7a85",
										]}
									/>
								}
								cap="Rotating"
								sub="new exit each time"
								onClick={() => setMode("rotating")}
							/>
						</div>
						{mode === "sticky" && (
							<>
								<div className="sub-label">Hold it for</div>
								<div className="tiles cols-4">
									{TTL.map((t) => (
										<Tile
											key={t.v}
											on={ttl === t.v}
											icon={<span className="num-m">{t.big}</span>}
											cap={t.cap}
											onClick={() => setTtl(t.v)}
										/>
									))}
								</div>
								<div className="sub-label">If the IP dies</div>
								<div className="chips">
									{LOSS.map((l) => (
										<button
											type="button"
											key={l.v}
											className={`chip${loss === l.v ? " on" : ""}`}
											onClick={() => setLoss(l.v)}
											aria-pressed={loss === l.v}
										>
											<Mark name={l.mark} size={14} />
											{l.cap}
										</button>
									))}
								</div>
							</>
						)}
					</Step>

					<Step n={3} title="Exit type and place">
						<div className="tiles cols-3">
							{(Object.keys(EXIT_COST) as ExitType[]).map((e) => (
								<Tile
									key={e}
									on={exits.includes(e)}
									icon={<ExitMark type={e} size={30} />}
									cap={EXIT_LABEL[e]}
									sub={
										<span className="price-level">
											{"$".repeat(EXIT_COST[e])}
											<span className="faint">
												{"$".repeat(3 - EXIT_COST[e])}
											</span>
										</span>
									}
									onClick={() => toggle(e)}
								/>
							))}
						</div>
						<div className="chips" style={{ marginTop: 14 }}>
							<span className="chip on">DE · Berlin</span>
							<span className="chip on">AS3320 Telekom</span>
							<span className="chip on">Match account +49</span>
							<span className="chip">Add constraint</span>
						</div>
					</Step>

					<Step n={4} title="How reliable?">
						<div className="row spread" style={{ marginBottom: 6 }}>
							<ReliabilityBadge
								edited={edited}
								risk={risk}
								floor={floor}
								achieved={works}
							/>
							<span className="faint">{domain} · tap an exit to override</span>
						</div>
						<div className="orbit-wrap">
							<OrbitalFrontier
								offers={OFFERS}
								domain={domain}
								eligible={eligible}
								pick={best}
								auto={auto}
								floor={floor}
								onPick={setManual}
								size={460}
							/>
						</div>
						<p className="caption">
							Closer to the centre works more often. Further round the dial
							costs more. The bright arc is the frontier.
						</p>
						<details className="dig">
							<summary>Advanced: reliability floor and risk dial</summary>
							<div className="dig-body advanced">
								<div className="field">
									<span className="field-label">
										Reliability floor · {Math.round(floor * 100)}%
									</span>
									<input
										type="range"
										min={70}
										max={99}
										value={Math.round(floor * 100)}
										aria-label="Reliability floor"
										onChange={(e) => setFloor(Number(e.target.value) / 100)}
									/>
								</div>
								<div className="row" style={{ gap: 28, alignItems: "center" }}>
									<div style={{ width: 220, textAlign: "center" }}>
										<Gauge value={risk} />
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
									<div className="dims">
										{dims.map((d) => (
											<div key={d.k} className="dim">
												<Mark name={d.mark} size={16} />
												<span className="muted">{d.k}</span>
												<b>{d.v}</b>
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
							</div>
						</details>
					</Step>

					<details className="dig glass step">
						<summary>Name, target URL, spend cap, webhook, overrides</summary>
						<div className="dig-body grid-2">
							<Field id="n" label="Name" v={NAME[use]} />
							<Field id="u" label="Target URL" v={`https://${domain}`} />
							<Field id="c" label="Monthly cap" v="$25.00" />
							<Field
								id="w"
								label="Webhook"
								v="https://ops.bridge.example/hooks"
							/>
							<Field
								id="f"
								label="max_fraud_score"
								v=""
								ph={String(resolved.maxFraudScore)}
							/>
							<Field
								id="s"
								label="min_sticky"
								v=""
								ph={formatSticky(resolved.minStickyMinutes)}
							/>
						</div>
					</details>
				</div>

				<aside className="sticky-col stack">
					<div className="eyebrow">Estimate</div>
					<LiquidMetal
						preset="chromatic"
						theme="dark"
						radius={24}
						ringWidth={2}
					>
						<div className="estimate">
							<div className="num-xl">
								{usd(best ? withTake(best.pricePerGb * gb) : 0)}
							</div>
							<div className="muted">
								per month at{" "}
								<input
									className="input inline mono"
									value={gb}
									aria-label="GB per month"
									onChange={(e) => setGb(Number(e.target.value) || 0)}
								/>{" "}
								GB
							</div>
							<Flow
								use={use}
								exit={best?.exitType ?? exits[0]}
								target={domain.split(".")[0]}
								mode={mode}
								caption={best?.provider ?? "no match"}
							/>
							<div className="est-stats">
								<div>
									<b>
										{works !== undefined ? `${Math.round(works * 100)}%` : "-"}
									</b>
									<span>works</span>
								</div>
								<div>
									<b>{best?.fraudScore ?? "-"}</b>
									<span>fraud</span>
								</div>
								<div>
									<b>
										{offers.length}/{OFFERS.length}
									</b>
									<span>eligible</span>
								</div>
							</div>
							<div className="faint">
								{best
									? `${usd(best.pricePerGb)}/GB + 5% fee · ${axis("cost").fmt(axis("cost").get(best, domain))} per working GB`
									: "Loosen the risk dial or exit types"}
							</div>
							<button type="button" className="btn btn-primary wide">
								Create route
							</button>
						</div>
					</LiquidMetal>
					<PolicyYaml
						yaml={policyYaml({
							name: NAME[use],
							mode,
							ttl,
							loss,
							exits,
							risk,
							floor,
							targetUrl: `https://${domain}`,
						})}
					/>
				</aside>
			</div>
		</>
	);
}
