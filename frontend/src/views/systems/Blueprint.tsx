// Blueprint Schematic: the product as an engineering drawing. A wiring
// diagram with numbered callouts, a title block, and a technical plot with
// dimension lines that price each step up the frontier.

import type { ReactNode } from "react";
import { Logo, Mark, type MarkName } from "../../components/icons";
import LiquidMetal from "../../vendor/LiquidMetal";
import { costOf, frontier, pct, pick, stepBelow, usd, worksOf } from "./data";

const VW = 1000;
const VH = 330;
const at = (x: number, y: number) => ({
	left: `${(x / VW) * 100}%`,
	top: `${(y / VH) * 100}%`,
});

function Node({
	x,
	y,
	children,
	cls = "",
}: {
	x: number;
	y: number;
	children: ReactNode;
	cls?: string;
}) {
	return (
		<div className={`bp-node ${cls}`} style={at(x, y)}>
			{children}
		</div>
	);
}

function Callout({ n, x, y }: { n: number; x: number; y: number }) {
	return (
		<span className="bp-callout" style={at(x, y)}>
			{n}
		</span>
	);
}

const EXITS: {
	y: number;
	mark: MarkName;
	name: string;
	meta: string;
	on?: boolean;
	lost?: boolean;
}[] = [
	{
		y: 60,
		mark: "exit-mobile",
		name: "iProxy farm",
		meta: "AS21928 · 41d pin",
	},
	{ y: 165, mark: "exit-isp", name: "NetNut", meta: "default pick", on: true },
	{
		y: 270,
		mark: "exit-isp",
		name: "Rayobyte",
		meta: "AS3320 · lost",
		lost: true,
	},
];

export function Blueprint() {
	return (
		<div className="bp">
			<div className="bp-sheet">
				<div className="bp-diagram">
					<svg
						viewBox={`0 0 ${VW} ${VH}`}
						className="bp-wires"
						aria-hidden="true"
					>
						<defs>
							<marker
								id="bp-arrow"
								viewBox="0 0 10 10"
								refX="9"
								refY="5"
								markerWidth="7"
								markerHeight="7"
								orient="auto-start-reverse"
							>
								<path d="M0 0 L10 5 L0 10 z" className="bp-arrowhead" />
							</marker>
						</defs>
						<path d="M200 60 C320 60 330 165 440 165" className="bp-wire" />
						<path d="M200 165 L440 165" className="bp-wire" />
						<path
							d="M200 270 C320 270 330 165 440 165"
							className="bp-wire bp-wire-bad"
						/>
						<path
							d="M520 165 C620 165 640 60 760 60"
							className="bp-wire"
							markerEnd="url(#bp-arrow)"
						/>
						<path
							d="M520 165 L760 165"
							className="bp-wire bp-wire-on"
							markerEnd="url(#bp-arrow)"
						/>
						<path
							d="M520 165 C620 165 640 270 700 270"
							className="bp-wire bp-wire-bad"
						/>
						<path d="M700 258 l16 24 M716 258 l-16 24" className="bp-x" />
						<path d="M880 165 h40" className="bp-leader" />
					</svg>

					<Node x={30} y={60}>
						<Logo name="beeper" size={22} label="bridge" />
						<div>
							<b>mautrix-whatsapp</b>
							<span>sticky · forever</span>
						</div>
					</Node>
					<Node x={30} y={165}>
						<Logo name="claude-code" size={22} label="agent" />
						<div>
							<b>agent-browser</b>
							<span>sticky · session</span>
						</div>
					</Node>
					<Node x={30} y={270} cls="bad">
						<Logo name="beeper" size={22} label="bridge" />
						<div>
							<b>mautrix-telegram</b>
							<span>fail closed</span>
						</div>
					</Node>
					<div className="bp-hub" style={at(480, 165)}>
						<Mark name="tunnel" size={34} />
						<span>humanip</span>
					</div>
					{EXITS.map((e) => (
						<Node
							key={e.name}
							x={e.lost ? 730 : 770}
							y={e.y}
							cls={e.on ? "on" : e.lost ? "bad" : ""}
						>
							<Mark name={e.mark} size={22} />
							<div>
								<b>{e.name}</b>
								<span>{e.meta}</span>
							</div>
						</Node>
					))}
					<Callout n={1} x={300} y={38} />
					<Callout n={2} x={300} y={292} />
					<Callout n={3} x={640} y={143} />
					<Callout n={4} x={450} y={112} />
				</div>

				<div className="bp-lower">
					<div className="bp-plot">
						<div className="bp-plot-title">
							FIG. 2 · Frontier, web.whatsapp.com
						</div>
						<Plot />
					</div>
					<ol className="bp-notes">
						<li>
							Hard pin on a mobile exit. Same IP for the life of the bridge.
						</li>
						<li>
							Pinned ISP exit died. Route refuses traffic until a human picks a
							replacement.
						</li>
						<li>
							Default pick: cheapest exit at or above the {pct(0.95)}{" "}
							reliability floor.
						</li>
						<li>
							Every route passes through one gateway: SOCKS5 :1080 and HTTP
							:8080.
						</li>
					</ol>
					<div className="bp-titleblock">
						<div className="bp-tb-row big">
							<span>PROJECT</span>
							<b>humanip</b>
						</div>
						<div className="bp-tb-row">
							<span>SHEET</span>
							<b>2 / 4</b>
							<span>REV</span>
							<b>A</b>
						</div>
						<div className="bp-tb-row">
							<span>PICK</span>
							<b>
								{pick.provider} · {usd(costOf(pick))}
							</b>
						</div>
						<LiquidMetal
							preset="silver"
							theme="light"
							radius={0}
							ringWidth={1.5}
							strength={0.8}
						>
							<button type="button" className="bp-btn">
								APPROVE &amp; CREATE
							</button>
						</LiquidMetal>
					</div>
				</div>
			</div>
		</div>
	);
}

function Plot() {
	const W = 560;
	const H = 210;
	const x = (c: number) => 50 + (c / 10) * (W - 70);
	const y = (w: number) => 20 + (1 - (Math.max(w, 0.9) - 0.9) / 0.1) * (H - 60);
	let d = "";
	frontier.forEach((o, i) => {
		d +=
			i === 0
				? `M${x(costOf(o))} ${y(worksOf(o))}`
				: ` H${x(costOf(o))} V${y(worksOf(o))}`;
	});
	const a = stepBelow;
	return (
		<svg
			viewBox={`0 0 ${W} ${H}`}
			className="bp-svg"
			role="img"
			aria-label="Frontier with dimension lines"
		>
			{[0.9, 0.95, 1].map((w) => (
				<g key={w}>
					<line
						x1="50"
						x2={W - 20}
						y1={y(w)}
						y2={y(w)}
						className={w === 0.95 ? "bp-floor" : "bp-grid"}
					/>
					<text x="44" y={y(w) + 4} textAnchor="end" className="bp-tick">
						{pct(w)}
					</text>
				</g>
			))}
			{[0, 2, 4, 6, 8, 10].map((c) => (
				<text
					key={c}
					x={x(c)}
					y={H - 22}
					textAnchor="middle"
					className="bp-tick"
				>
					${c}
				</text>
			))}
			<path d={`${d} H${W - 20}`} className="bp-stair" />
			{frontier.map((o) => (
				<g key={o.provider}>
					<rect
						x={x(costOf(o)) - 4}
						y={y(worksOf(o)) - 4}
						width="8"
						height="8"
						className={o === pick ? "bp-pt on" : "bp-pt"}
					/>
					<text
						x={x(costOf(o)) > W - 110 ? x(costOf(o)) - 8 : x(costOf(o)) + 8}
						y={y(worksOf(o)) - 8}
						textAnchor={x(costOf(o)) > W - 110 ? "end" : "start"}
						className="bp-lbl"
					>
						{o.provider}
					</text>
				</g>
			))}
			{a && (
				<g className="bp-dim">
					<line
						x1={x(costOf(a))}
						x2={x(costOf(pick))}
						y1={y(0.905)}
						y2={y(0.905)}
						markerStart="url(#bp-arrow)"
						markerEnd="url(#bp-arrow)"
					/>
					<text
						x={(x(costOf(a)) + x(costOf(pick))) / 2}
						y={y(0.905) - 6}
						textAnchor="middle"
					>
						+{usd(costOf(pick) - costOf(a))}/GB
					</text>
					<line
						x1={x(costOf(pick)) + 26}
						x2={x(costOf(pick)) + 26}
						y1={y(worksOf(a))}
						y2={y(worksOf(pick))}
						markerStart="url(#bp-arrow)"
						markerEnd="url(#bp-arrow)"
					/>
					<text
						x={x(costOf(pick)) + 32}
						y={(y(worksOf(a)) + y(worksOf(pick))) / 2 + 4}
					>
						+{Math.round((worksOf(pick) - worksOf(a)) * 100)} pts works
					</text>
				</g>
			)}
			<text x={W - 20} y={H - 4} textAnchor="end" className="bp-tick">
				$ / WORKING GB
			</text>
		</svg>
	);
}
