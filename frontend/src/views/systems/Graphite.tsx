// Graphite Console: dense table + inspector, command bar, status bar.
// Linear/Vercel-like information density; 12px type; keyboard-first.

import { Logo, Mark } from "../../components/icons";
import LiquidMetal from "../../vendor/LiquidMetal";
import ThinkingOrb from "../../vendor/ThinkingOrb";
import {
	costOf,
	EXIT_MARK,
	frontier,
	HEALTH_MARK,
	offers,
	pct,
	pick,
	routes,
	usd,
	worksOf,
} from "./data";

export function Graphite() {
	const sel = routes[0];
	return (
		<div className="gc">
			<div className="gc-bar">
				<span className="gc-brand">
					<Mark name="tunnel" size={15} /> humanip
				</span>
				<span className="gc-crumb">
					routes <i>/</i> all
				</span>
				<span className="gc-search">
					<Mark name="reranking" size={13} /> Search routes, IPs, providers
					<kbd>⌘K</kbd>
				</span>
				<span className="gc-pill on">All 4</span>
				<span className="gc-pill">Sticky 3</span>
				<span className="gc-pill">Alerts 1</span>
			</div>

			<div className="gc-body">
				<table className="gc-table">
					<thead>
						<tr>
							<th />
							<th>Route</th>
							<th>Exit</th>
							<th>IP</th>
							<th>24h</th>
							<th className="r">Fraud</th>
							<th className="r">30d</th>
						</tr>
					</thead>
					<tbody>
						{routes.map((r) => (
							<tr key={r.id} className={r === sel ? "sel" : ""}>
								<td>
									<Mark
										name={HEALTH_MARK[r.status]}
										size={16}
										className={`gc-${r.status}`}
										label={r.status}
									/>
								</td>
								<td>
									<div className="gc-name">
										{r.useCase === "mautrix" ? (
											<Logo name="beeper" size={14} label="bridge" />
										) : r.useCase === "scrape" ? (
											<Logo name="apify" size={14} label="scraper" />
										) : (
											<Logo name="claude-code" size={14} label="agent" />
										)}
										{r.name}
									</div>
								</td>
								<td>
									<Mark name={EXIT_MARK[r.exitType]} size={14} />
								</td>
								<td className="m">{r.exitIp}</td>
								<td>
									<span className="gc-bars">
										{r.spark
											.map((v, h) => ({ v, k: `${r.id}-${h}` }))
											.map(({ v, k }) => (
												<i
													key={k}
													style={{ height: `${Math.max(v, 4) * 0.16}px` }}
													className={v < 50 ? "bad" : v < 85 ? "warn" : ""}
												/>
											))}
									</span>
								</td>
								<td className="r m">
									{r.status === "pin_lost" ? "-" : r.fraudScore}
								</td>
								<td className="r m">{usd(r.spend30d)}</td>
							</tr>
						))}
					</tbody>
				</table>

				<aside className="gc-inspect">
					<div className="gc-ih">
						<Logo name="beeper" size={18} label="bridge" />
						<b>{sel.name}</b>
						<Mark
							name="robot"
							size={16}
							className="gc-healthy"
							label="healthy"
						/>
					</div>
					<dl>
						<dt>Pinned</dt>
						<dd className="m">{sel.exitIp}</dd>
						<dt>Held for</dt>
						<dd className="m">{sel.pinnedFor}</dd>
						<dt>On loss</dt>
						<dd>Fail closed</dd>
						<dt>ASN</dt>
						<dd className="m">21928</dd>
					</dl>
					<div className="gc-sec">Frontier · whatsapp</div>
					<svg
						viewBox="0 0 280 90"
						className="gc-chart"
						role="img"
						aria-label="frontier"
					>
						{offers.map((o) => {
							const x = 10 + (costOf(o) / 10) * 260;
							const y = 80 - Math.max(0, (worksOf(o) - 0.9) / 0.1) * 70;
							const on = frontier.includes(o);
							return (
								<circle
									key={o.provider}
									cx={x}
									cy={y}
									r={o === pick ? 4 : 2.2}
									className={o === pick ? "pk" : on ? "on" : "off"}
								/>
							);
						})}
						<line
							x1="10"
							x2="270"
							y1={80 - 0.5 * 70}
							y2={80 - 0.5 * 70}
							className="fl"
						/>
					</svg>
					<div className="gc-pick">
						<Mark name="exit-isp" size={16} />
						<span>{pick.provider}</span>
						<span className="m">{pct(worksOf(pick))}</span>
						<span className="m">{usd(costOf(pick))}</span>
					</div>
					<LiquidMetal
						preset="silver"
						theme="dark"
						radius={6}
						ringWidth={1.5}
						strength={0.7}
					>
						<button type="button" className="gc-btn">
							Apply pick <kbd>↵</kbd>
						</button>
					</LiquidMetal>
				</aside>
			</div>

			<div className="gc-status">
				<ThinkingOrb state="connecting" size={14} color="#7cf0b4" />
				<span>3 live</span>
				<span className="bad">1 pin lost</span>
				<span>p50 210ms</span>
				<span className="gc-spacer" />
				<span>
					<kbd>J</kbd>
					<kbd>K</kbd> move
				</span>
				<span>
					<kbd>E</kbd> edit
				</span>
			</div>
		</div>
	);
}
