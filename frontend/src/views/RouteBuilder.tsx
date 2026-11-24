import { useMemo, useState } from "react";
import { ExitBadge, PageHead, Segmented } from "../components/ui";
import { effectivePrice, eligibleOffers } from "../lib/mock";
import {
	EXIT_LABEL,
	type ExitType,
	formatSticky,
	PIN_LOSS_LABEL,
	type PinLoss,
	type RouteMode,
	resolveRisk,
	riskLabel,
	usd,
	withTake,
} from "../lib/policy";

interface Template {
	key: string;
	name: string;
	desc: string;
	mode: RouteMode;
	ttl: string;
	exits: ExitType[];
	risk: number;
	gb: number;
}

const TEMPLATES: Template[] = [
	{
		key: "browser",
		name: "Browser agent session",
		desc: "Sticky for one computer-use / browser-use session, then released.",
		mode: "sticky",
		ttl: "session",
		exits: ["residential"],
		risk: 40,
		gb: 20,
	},
	{
		key: "mautrix",
		name: "mautrix bridge",
		desc: "Hard-pinned IP for months, geo-matched to the account, mobile-first.",
		mode: "sticky",
		ttl: "forever",
		exits: ["mobile", "isp"],
		risk: 8,
		gb: 1,
	},
	{
		key: "rotating",
		name: "Rotating pool",
		desc: "New exit per request or per N minutes. Cheapest per GB.",
		mode: "rotating",
		ttl: "per-request",
		exits: ["residential"],
		risk: 65,
		gb: 200,
	},
];

const TTL_MINUTES: Record<string, number> = {
	session: 30,
	"24h": 24 * 60,
	"30d": 30 * 24 * 60,
	forever: 90 * 24 * 60,
	"per-request": 0,
};

export function RouteBuilder() {
	const [tpl, setTpl] = useState("mautrix");
	const t = TEMPLATES.find((x) => x.key === tpl) ?? TEMPLATES[0];
	const [mode, setMode] = useState<RouteMode>(t.mode);
	const [ttl, setTtl] = useState(t.ttl);
	const [pinLoss, setPinLoss] = useState<PinLoss>("fail_closed");
	const [exits, setExits] = useState<ExitType[]>(t.exits);
	const [risk, setRisk] = useState(t.risk);
	const [gb, setGb] = useState(t.gb);
	const [matchAccount, setMatchAccount] = useState(true);
	const [advanced, setAdvanced] = useState(false);

	const pick = (next: Template) => {
		setTpl(next.key);
		setMode(next.mode);
		setTtl(next.ttl);
		setExits(next.exits);
		setRisk(next.risk);
		setGb(next.gb);
	};

	const resolved = useMemo(() => resolveRisk(risk, exits), [risk, exits]);
	const stickyMin = mode === "sticky" ? (TTL_MINUTES[ttl] ?? 0) : 0;
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
	const monthly = best ? withTake(best.pricePerGb * gb) : 0;

	const toggleExit = (e: ExitType) =>
		setExits((cur) =>
			cur.includes(e)
				? cur.length > 1
					? cur.filter((x) => x !== e)
					: cur
				: [...cur, e],
		);

	return (
		<>
			<PageHead
				title="New route"
				sub="A route is a saved policy. Clients reference it by name in the proxy username or use its dedicated endpoint."
			/>
			<div className="builder">
				<div className="stack">
					<div className="card card-pad">
						<div className="card-title">Start from</div>
						<div className="templates">
							{TEMPLATES.map((x) => (
								<button
									type="button"
									key={x.key}
									className={`tpl${tpl === x.key ? " on" : ""}`}
									onClick={() => pick(x)}
								>
									<div className="tpl-name">{x.name}</div>
									<div className="tpl-desc">{x.desc}</div>
								</button>
							))}
						</div>
					</div>

					<div className="card card-pad">
						<div className="card-title">Identity & stickiness</div>
						<div className="grid-2">
							<div className="field">
								<label htmlFor="rname">Route name</label>
								<input
									id="rname"
									className="input mono"
									defaultValue="mautrix-whatsapp-eu"
								/>
							</div>
							<div className="field">
								<span className="field-label">Mode</span>
								<Segmented
									value={mode}
									onChange={setMode}
									options={[
										{ value: "sticky", label: "Sticky" },
										{ value: "rotating", label: "Rotating" },
									]}
								/>
							</div>
						</div>
						{mode === "sticky" ? (
							<div className="grid-2">
								<div className="field">
									<label htmlFor="ttl">Pin duration</label>
									<select
										id="ttl"
										className="input"
										value={ttl}
										onChange={(e) => setTtl(e.target.value)}
									>
										<option value="session">
											Until session closes (≤30m idle)
										</option>
										<option value="24h">24 hours</option>
										<option value="30d">30 days</option>
										<option value="forever">Forever (hard pin)</option>
									</select>
								</div>
								<div className="field">
									<label htmlFor="pinloss">If the pinned IP dies</label>
									<select
										id="pinloss"
										className="input"
										value={pinLoss}
										onChange={(e) => setPinLoss(e.target.value as PinLoss)}
									>
										{(Object.keys(PIN_LOSS_LABEL) as PinLoss[]).map((k) => (
											<option key={k} value={k}>
												{PIN_LOSS_LABEL[k]}
											</option>
										))}
									</select>
								</div>
							</div>
						) : (
							<div className="field">
								<label htmlFor="rot">Rotate</label>
								<select id="rot" className="input">
									<option>Every request</option>
									<option>Every 5 minutes</option>
									<option>On block / CAPTCHA</option>
								</select>
							</div>
						)}
					</div>

					<div className="card card-pad">
						<div className="card-title">Exit & geo</div>
						<div className="field">
							<span className="field-label">
								Preferred exit types (in order)
							</span>
							<div className="row">
								{(Object.keys(EXIT_LABEL) as ExitType[]).map((e) => (
									<button
										type="button"
										key={e}
										className={`chip${exits.includes(e) ? " on" : ""}`}
										onClick={() => toggleExit(e)}
									>
										<span className={`exit-${e}`}>●</span> {EXIT_LABEL[e]}
									</button>
								))}
							</div>
						</div>
						<div
							className="grid-2"
							style={{ gridTemplateColumns: "1fr 1fr 1fr" }}
						>
							<div className="field">
								<label htmlFor="cc">Country</label>
								<select id="cc" className="input" defaultValue="DE">
									<option value="US">United States</option>
									<option value="DE">Germany</option>
									<option value="GB">United Kingdom</option>
								</select>
							</div>
							<div className="field">
								<label htmlFor="city">City</label>
								<input id="city" className="input" defaultValue="Berlin" />
							</div>
							<div className="field">
								<label htmlFor="asn">ASN / carrier</label>
								<input
									id="asn"
									className="input mono"
									defaultValue="AS3320 Telekom"
								/>
							</div>
						</div>
						<label className="row muted" style={{ gap: 8 }}>
							<input
								type="checkbox"
								checked={matchAccount}
								onChange={(e) => setMatchAccount(e.target.checked)}
							/>
							Geo-match to account region (phone number +49 → DE)
						</label>
					</div>

					<div className="card card-pad">
						<div className="row" style={{ justifyContent: "space-between" }}>
							<div className="card-title" style={{ margin: 0 }}>
								Risk budget
							</div>
							<span className="badge">{riskLabel(risk)}</span>
						</div>
						<div
							className="row"
							style={{ alignItems: "baseline", gap: 8, marginTop: 8 }}
						>
							<span className="big-cost">{risk}</span>
							<span className="muted">
								/ 100 · how much risk to take for a lower price
							</span>
						</div>
						<input
							type="range"
							min={0}
							max={100}
							value={risk}
							className="risk-slider"
							aria-label="Risk"
							onChange={(e) => setRisk(Number(e.target.value))}
						/>
						<div className="risk-scale">
							<span>Clean & expensive</span>
							<span>Cheap & noisy</span>
						</div>
						<div className="dims">
							<Dim k="Max fraud score" v={`≤ ${resolved.maxFraudScore}`} />
							<Dim
								k="Exit types allowed"
								v={resolved.allowedExitTypes
									.map((e) => EXIT_LABEL[e])
									.join(" → ")}
							/>
							<Dim k="Sourcing tiers" v={resolved.sourcingTiers.join(", ")} />
							<Dim
								k="Min sticky guarantee"
								v={formatSticky(resolved.minStickyMinutes)}
							/>
						</div>
						<button
							type="button"
							className="btn"
							style={{ marginTop: 12 }}
							onClick={() => setAdvanced((a) => !a)}
						>
							{advanced ? "Hide" : "Override"} individual dimensions
						</button>
						{advanced && (
							<div className="grid-2" style={{ marginTop: 12 }}>
								<div className="field">
									<label htmlFor="ovf">max_fraud_score</label>
									<input
										id="ovf"
										className="input mono"
										placeholder={String(resolved.maxFraudScore)}
									/>
								</div>
								<div className="field">
									<label htmlFor="ovs">min_sticky</label>
									<input
										id="ovs"
										className="input mono"
										placeholder={formatSticky(resolved.minStickyMinutes)}
									/>
								</div>
							</div>
						)}
					</div>

					<div className="card card-pad">
						<div className="card-title">Targets & limits</div>
						<div className="grid-2">
							<div className="field">
								<label htmlFor="url">
									Target URL (optional, for cheapest-exit lookup)
								</label>
								<input
									id="url"
									className="input mono"
									defaultValue="https://web.whatsapp.com"
								/>
							</div>
							<div className="field">
								<label htmlFor="cap">Monthly spend cap</label>
								<input id="cap" className="input mono" defaultValue="$25.00" />
							</div>
						</div>
						<div className="field" style={{ marginBottom: 0 }}>
							<label htmlFor="hook">
								Alert webhook (pin loss, cap reached, health drop)
							</label>
							<input
								id="hook"
								className="input mono"
								defaultValue="https://ops.bridge.example/hooks/humanip"
							/>
						</div>
					</div>
				</div>

				<div className="sticky-col stack">
					<div className="card card-pad">
						<div className="card-title">Estimate</div>
						<div className="big-cost">{usd(monthly)}</div>
						<div className="muted" style={{ marginBottom: 12 }}>
							per month at{" "}
							<input
								className="input mono"
								style={{
									width: 64,
									height: 24,
									display: "inline",
									padding: "0 6px",
								}}
								value={gb}
								onChange={(e) => setGb(Number(e.target.value) || 0)}
							/>{" "}
							GB
						</div>
						<div className="kv">
							<span className="muted">Selected exit</span>
							<span>{best ? best.provider : "no match"}</span>
						</div>
						{best && (
							<>
								<div className="kv">
									<span className="muted">Type</span>
									<ExitBadge type={best.exitType} />
								</div>
								<div className="kv">
									<span className="muted">Provider price</span>
									<span className="mono">{usd(best.pricePerGb)}/GB</span>
								</div>
								<div className="kv">
									<span className="muted">humanip fee (5%)</span>
									<span className="mono">{usd(best.pricePerGb * 0.05)}/GB</span>
								</div>
								<div className="kv">
									<span className="muted">Pool fraud score</span>
									<span className="mono">{best.fraudScore}</span>
								</div>
							</>
						)}
						<div className="kv">
							<span className="muted">Eligible providers</span>
							<span className="mono">{offers.length} / 9</span>
						</div>
						<button
							type="button"
							className="btn btn-primary"
							style={{ width: "100%", marginTop: 14, justifyContent: "center" }}
						>
							Create route
						</button>
					</div>
					<div className="card card-pad">
						<div className="card-title">Policy (YAML)</div>
						<pre className="code">
							{`route: mautrix-whatsapp-eu
mode: ${mode}
${mode === "sticky" ? `sticky:\n  ttl: ${ttl}\n  on_pin_loss: ${pinLoss}\n` : ""}exit_types: [${exits.join(", ")}]
geo: { country: DE, city: Berlin,
       asn: 3320, match_account: ${matchAccount} }
risk: ${risk}
target_url: https://web.whatsapp.com
spend_cap_usd: 25`}
						</pre>
					</div>
				</div>
			</div>
		</>
	);
}

function Dim({ k, v }: { k: string; v: string }) {
	return (
		<div className="dim">
			<div className="dim-k">{k}</div>
			<div className="dim-v">{v}</div>
		</div>
	);
}
