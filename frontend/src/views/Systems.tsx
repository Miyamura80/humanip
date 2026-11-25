// Design-system exploration: the same slice of product (two routes, the
// default pick, the cost/reliability frontier) rendered in four systems.

import { Logo, Mark, type MarkName } from "../components/icons";
import { OFFERS, successRate } from "../lib/mock";
import { axis, costReliabilityFrontier, defaultPick } from "../lib/pareto";
import type { ExitType } from "../lib/policy";
import LiquidMetal, { type MetalPreset } from "../vendor/LiquidMetal";
import ThinkingOrb from "../vendor/ThinkingOrb";

interface System {
	id: string;
	name: string;
	tag: string;
	fonts: string;
	orb: string;
	metal?: MetalPreset;
	theme: "dark" | "light";
	swatches: string[];
}

const SYSTEMS: System[] = [
	{
		id: "graphite",
		name: "Graphite",
		tag: "Dense dev console. Quiet greys, one mint signal.",
		fonts: "Inter · JetBrains Mono",
		orb: "#7cf0b4",
		metal: "silver",
		theme: "dark",
		swatches: ["#0a0c0f", "#161a1f", "#e6e9ed", "#7cf0b4", "#ff6b6b"],
	},
	{
		id: "phosphor",
		name: "Phosphor",
		tag: "Terminal. Mono everywhere, amber on black, square edges.",
		fonts: "IBM Plex Mono",
		orb: "#ffb000",
		theme: "dark",
		swatches: ["#050505", "#0d0b06", "#ffb000", "#ffd27a", "#ff5f3d"],
	},
	{
		id: "blueprint",
		name: "Blueprint",
		tag: "Light technical drawing. Ink hairlines, signal blue.",
		fonts: "Space Grotesk · IBM Plex Mono",
		orb: "#2a4bff",
		metal: "silver",
		theme: "light",
		swatches: ["#f4f1ea", "#ffffff", "#14171a", "#2a4bff", "#e5484d"],
	},
	{
		id: "cyan",
		name: "Cyan Signal",
		tag: "eito.me house style. Near-black, cyan, chromatic metal.",
		fonts: "System UI · JetBrains Mono",
		orb: "#7fdfff",
		metal: "chromatic",
		theme: "dark",
		swatches: ["#0a0a0a", "#101314", "#e6e6e6", "#7fdfff", "#c3fffd"],
	},
];

const EXIT_MARK: Record<ExitType, MarkName> = {
	mobile: "exit-mobile",
	isp: "exit-isp",
	residential: "exit-residential",
};

const DOMAIN = "web.whatsapp.com";

export function Systems() {
	return (
		<div className="ds-page">
			<div className="page-head">
				<h1>Design systems</h1>
			</div>
			{SYSTEMS.map((s) => (
				<Specimen key={s.id} s={s} />
			))}
		</div>
	);
}

function Specimen({ s }: { s: System }) {
	const pick = defaultPick(OFFERS, DOMAIN);
	return (
		<section className="ds" data-ds={s.id} id={`ds-${s.id}`}>
			<header className="ds-head">
				<div className="ds-brand">
					<Mark name="tunnel" size={22} />
					<span>humanip</span>
				</div>
				<div className="ds-title">
					<b>{s.name}</b>
					<span>{s.tag}</span>
				</div>
				<div className="ds-swatches">
					{s.swatches.map((c) => (
						<i key={c} style={{ background: c }} title={c} />
					))}
					<span className="ds-fonts">{s.fonts}</span>
				</div>
			</header>

			<div className="ds-grid">
				<RouteCard
					s={s}
					client={<Logo name="beeper" size={26} label="Beeper bridge" />}
					name="mautrix-whatsapp"
					exit="mobile"
					ip="172.58.201.44"
					pinned="41d"
					health="robot"
					shield="shield-check"
					fraud={4}
				/>
				<RouteCard
					s={s}
					client={<Logo name="beeper" size={26} label="Beeper bridge" />}
					name="mautrix-telegram"
					exit="isp"
					ip="no exit"
					pinned="lost 14m"
					health="broken-robot"
					shield="shield-off"
					lost
				/>
				<div className="ds-card ds-pick">
					<div className="ds-label">Default pick</div>
					{pick && (
						<>
							<div className="ds-pick-row">
								<Mark name={EXIT_MARK[pick.exitType]} size={34} />
								<div>
									<div className="ds-strong">{pick.provider}</div>
									<div className="ds-muted ds-mono">
										{Math.round(successRate(DOMAIN, pick.provider) * 100)}%
										works · fraud {pick.fraudScore}
									</div>
								</div>
							</div>
							<div className="ds-price">
								{axis("cost").fmt(axis("cost").get(pick, DOMAIN))}
								<span className="ds-muted"> / working GB</span>
							</div>
						</>
					)}
					<Cta s={s} />
				</div>
			</div>

			<div className="ds-grid ds-grid-2">
				<div className="ds-card">
					<div className="ds-row">
						<Mark name="reranking" size={22} />
						<span className="ds-label">Frontier · {DOMAIN}</span>
					</div>
					<MiniFrontier />
				</div>
				<div className="ds-card ds-legend">
					<div className="ds-label">Vocabulary</div>
					<Legend mark="robot" text="healthy" />
					<Legend mark="glitch-robot" text="degraded" tone="warn" />
					<Legend mark="broken-robot" text="pin lost" tone="bad" />
					<Legend mark="devil-robot" text="IP flagged" tone="bad" />
					<Legend mark="shield-check" text="clean reputation" />
					<Legend mark="shield-alert" text="borderline" tone="warn" />
					<Legend mark="exit-mobile" text="mobile" />
					<Legend mark="exit-isp" text="ISP static" />
					<Legend mark="exit-residential" text="residential" />
					<div className="ds-row ds-muted">
						<ThinkingOrb state="connecting" size={22} color={s.orb} />
						live session
					</div>
				</div>
			</div>
		</section>
	);
}

function Cta({ s }: { s: System }) {
	const btn = (
		<button type="button" className="ds-cta">
			Create route
		</button>
	);
	return s.metal ? (
		<LiquidMetal
			preset={s.metal}
			theme={s.theme}
			radius={s.id === "blueprint" ? 2 : 10}
			ringWidth={2}
			glow={false}
			strength={0.9}
		>
			{btn}
		</LiquidMetal>
	) : (
		btn
	);
}

function RouteCard(props: {
	s: System;
	client: React.ReactNode;
	name: string;
	exit: ExitType;
	ip: string;
	pinned: string;
	health: MarkName;
	shield: MarkName;
	fraud?: number;
	lost?: boolean;
}) {
	const { s, lost } = props;
	return (
		<div className={`ds-card${lost ? " ds-lost" : ""}`}>
			<div className="ds-row ds-spread">
				<div className="ds-row">
					{props.client}
					<div>
						<div className="ds-strong">{props.name}</div>
						<div className="ds-muted ds-mono">
							sticky · pinned {props.pinned}
						</div>
					</div>
				</div>
				<Mark
					name={props.health}
					size={30}
					className={lost ? "ds-bad" : "ds-accent"}
					label={lost ? "pin lost" : "healthy"}
				/>
			</div>
			<div className={`ds-flow${lost ? " ds-flow-cut" : ""}`}>
				<span className="ds-node">
					<Mark name="developer" size={18} />
				</span>
				<i />
				<span className="ds-node ds-hub">
					<Mark name="tunnel" size={16} />
				</span>
				<i />
				<span className="ds-node">
					<Mark name={EXIT_MARK[props.exit]} size={18} />
				</span>
				<i />
				<span className="ds-node ds-mono ds-target">wa</span>
			</div>
			<div className="ds-row ds-spread">
				<span className="ds-mono">{props.ip}</span>
				<span className="ds-row">
					{!lost && <ThinkingOrb state="connecting" size={20} color={s.orb} />}
					<Mark
						name={props.shield}
						size={18}
						className={lost ? "ds-muted" : "ds-accent"}
					/>
					<span className="ds-mono">{props.fraud ?? "n/a"}</span>
				</span>
			</div>
		</div>
	);
}

function Legend({
	mark,
	text,
	tone,
}: {
	mark: MarkName;
	text: string;
	tone?: "warn" | "bad";
}) {
	return (
		<div className="ds-row">
			<Mark name={mark} size={18} className={tone ? `ds-${tone}` : undefined} />
			<span className="ds-muted">{text}</span>
		</div>
	);
}

function MiniFrontier() {
	const W = 900;
	const H = 250;
	const cost = axis("cost");
	const works = axis("success");
	const xs = OFFERS.map((o) => cost.get(o, DOMAIN));
	const [x0, x1] = [Math.min(...xs), Math.max(...xs)];
	// Zoom on the band that matters; weaker offers sit on the bottom edge.
	const [y0, y1] = [0.9, 1];
	const sx = (v: number) => 30 + ((v - x0) / (x1 - x0)) * (W - 50);
	const sy = (v: number) =>
		10 + (1 - (Math.max(v, y0) - y0) / (y1 - y0)) * (H - 30);
	const front = costReliabilityFrontier(OFFERS, DOMAIN);
	const pick = defaultPick(OFFERS, DOMAIN);
	let d = "";
	front.forEach((o, i) => {
		const px = sx(cost.get(o, DOMAIN));
		const py = sy(works.get(o, DOMAIN));
		d += i === 0 ? `M${px} ${py}` : ` H${px} V${py}`;
	});
	return (
		<svg
			viewBox={`0 0 ${W} ${H}`}
			className="ds-chart"
			role="img"
			aria-label="Cost versus reliability frontier"
		>
			<line
				x1="30"
				x2={W - 20}
				y1={sy(0.95)}
				y2={sy(0.95)}
				className="ds-floor"
			/>
			<text x="34" y={sy(0.95) + 15} className="ds-axis">
				95% floor
			</text>
			<path d={`${d} H${W - 20}`} className="ds-stair" />
			{OFFERS.map((o) => {
				const cx = sx(cost.get(o, DOMAIN));
				const cy = sy(works.get(o, DOMAIN));
				const on = front.includes(o);
				return (
					<g key={o.provider}>
						{o === pick && (
							<circle cx={cx} cy={cy} r="10" className="ds-pickring" />
						)}
						<circle
							cx={cx}
							cy={cy}
							r={on ? 4.5 : 3}
							className={`ds-pt ds-pt-${o.exitType}${on ? "" : " ds-pt-off"}`}
						/>
						{on && (
							<text
								x={cx > W - 140 ? cx - 10 : cx + 10}
								y={cy - 8}
								textAnchor={cx > W - 140 ? "end" : "start"}
								className="ds-axis ds-ptlabel"
							>
								{o.provider}
							</text>
						)}
					</g>
				);
			})}
			<text x="30" y={H - 4} className="ds-axis">
				cheaper
			</text>
			<text x={W - 20} y={H - 4} textAnchor="end" className="ds-axis">
				$ / working GB →
			</text>
		</svg>
	);
}
