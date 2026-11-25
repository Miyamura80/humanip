// Phosphor TUI: the console as a text-mode screen. Box-drawing borders,
// an ASCII frontier plotted from the real data, keyboard hints, a cursor.

import { Mark, type MarkName } from "../../components/icons";
import {
	costOf,
	EXIT_SHORT,
	FLOOR,
	frontier,
	HEALTH_MARK,
	offers,
	pct,
	pick,
	routes,
	usd,
	worksOf,
} from "./data";

const W = 84; // inner width in characters

type Seg = { t: string; c?: string } | { mark: MarkName; c?: string };
const len = (s: Seg[]) =>
	s.reduce((n, x) => n + ("t" in x ? x.t.length : 2), 0);

function Line({ segs, edge = "│" }: { segs: Seg[]; edge?: string }) {
	const pad = Math.max(0, W - len(segs));
	return (
		<div>
			<span className="pt-dim">{edge} </span>
			{segs
				.map((s, i) => ({ s, k: `s${i}` }))
				.map(({ s, k }) =>
					"t" in s ? (
						<span key={k} className={s.c}>
							{s.t}
						</span>
					) : (
						<Mark
							key={k}
							name={s.mark}
							size={14}
							className={`pt-mark ${s.c ?? ""}`}
							style={{ width: "2ch", height: "1.1em" }}
						/>
					),
				)}
			{" ".repeat(pad)}
			<span className="pt-dim"> {edge}</span>
		</div>
	);
}

function Rule({
	title,
	left = "├",
	right = "┤",
}: {
	title?: string;
	left?: string;
	right?: string;
}) {
	const t = title ? `─ ${title} ` : "";
	return (
		<div className="pt-dim">
			{left}
			{t}
			{"─".repeat(W + 2 - t.length)}
			{right}
		</div>
	);
}

const col = (s: string, n: number) =>
	s.length > n ? `${s.slice(0, n - 1)}…` : s.padEnd(n);

/** Character-grid frontier: rows = reliability, columns = $/working GB. */
function asciiFrontier(): string[] {
	const R = 9;
	const C = 62;
	const lo = 0.92;
	const hi = 1.0;
	const maxCost = Math.max(...frontier.map(costOf)) * 1.05;
	const grid = Array.from({ length: R }, () => Array(C).fill(" "));
	const rowOf = (w: number) =>
		Math.round(
			(1 - (Math.min(Math.max(w, lo), hi) - lo) / (hi - lo)) * (R - 1),
		);
	const colOf = (c: number) =>
		Math.min(C - 1, Math.round((c / maxCost) * (C - 1)));
	const fr = rowOf(FLOOR);
	for (let c = 0; c < C; c++) grid[fr][c] = "┄";
	for (const o of offers) {
		const w = worksOf(o);
		if (w >= lo && costOf(o) <= maxCost && !frontier.includes(o))
			grid[rowOf(w)][colOf(costOf(o))] = "·";
	}
	const cell = (o: (typeof frontier)[number]) => [
		colOf(costOf(o)),
		rowOf(worksOf(o)),
	];
	// Wires first, then points and labels, so labels are never overdrawn.
	frontier.forEach((o, i) => {
		if (i === 0) return;
		const [c1, r1] = cell(frontier[i - 1]);
		const [c2, r2] = cell(o);
		for (let c = c1 + 1; c < c2; c++) grid[r1][c] = "─";
		grid[r1][c2] = "┘";
		for (let r = r2 + 1; r < r1; r++) grid[r][c2] = "│";
	});
	for (const o of frontier) {
		const [c2, r2] = cell(o);
		grid[r2][c2] = o === pick ? "◉" : "●";
		const label = ` ${o.provider}`;
		const start = c2 + 1 + label.length <= C ? c2 + 1 : c2 - label.length;
		for (let k = 0; k < label.length; k++) grid[r2][start + k] = label[k];
	}
	return grid.map((row, r) => {
		const w = hi - (r / (R - 1)) * (hi - lo);
		const tick = r % 2 === 0 || r === fr ? pct(w).padStart(4) : "    ";
		return `${tick} ┤${row.join("")}`;
	});
}

const meter = (v: number, n = 16) =>
	"█".repeat(Math.round(v * n)) + "░".repeat(n - Math.round(v * n));

export function Phosphor() {
	const plot = asciiFrontier();
	const lost = routes.filter((r) => r.status === "pin_lost").length;
	return (
		<div className="pt">
			<pre className="pt-screen">
				<Rule
					left="┌"
					right="┐"
					title={`humanip ─ routes ─ ${routes.length} routes · ${lost} alert`}
				/>
				<Line
					segs={[
						{
							t: "STAT  ROUTE              CLIENT  EXIT  IP                FRAUD  PIN/RATE",
							c: "pt-dim",
						},
					]}
				/>
				{routes.map((r) => (
					<Line
						key={r.id}
						segs={[
							{
								mark: HEALTH_MARK[r.status],
								c:
									r.status === "pin_lost"
										? "pt-bad"
										: r.status === "degraded"
											? "pt-warn"
											: "",
							},
							{
								t:
									r.status === "pin_lost"
										? " !! "
										: r.status === "degraded"
											? " ~~ "
											: " OK ",
								c:
									r.status === "pin_lost"
										? "pt-bad"
										: r.status === "degraded"
											? "pt-warn"
											: "pt-hi",
							},
							{ t: col(r.name, 19) },
							{
								t: col(
									r.useCase === "mautrix"
										? "bridge"
										: r.useCase === "scrape"
											? "apify"
											: "agent",
									8,
								),
								c: "pt-dim",
							},
							{ t: col(EXIT_SHORT[r.exitType], 6) },
							{
								t: col(r.exitIp, 18),
								c: r.status === "pin_lost" ? "pt-bad" : "",
							},
							{
								t: col(
									r.status === "pin_lost"
										? "--"
										: String(r.fraudScore).padStart(2, "0"),
									7,
								),
							},
							{ t: r.pinnedFor ? col(r.pinnedFor, 12) : r.mode },
						]}
					/>
				))}
				<Rule
					title={`frontier :: ${"web.whatsapp.com"} :: works vs $/working GB`}
				/>
				{plot.map((l) => {
					const i = l.indexOf(pick.provider);
					return (
						<Line
							key={l}
							segs={
								i < 0
									? [{ t: l, c: l.includes("┄") ? "pt-dim" : "" }]
									: [
											{ t: l.slice(0, i) },
											{ t: pick.provider, c: "pt-inv" },
											{ t: l.slice(i + pick.provider.length) },
										]
							}
						/>
					);
				})}
				<Line segs={[{ t: `     └${"─".repeat(62)}`, c: "pt-dim" }]} />
				<Line
					segs={[
						{
							t: `      $0${" ".repeat(48)}$/GB → ${usd(Math.max(...frontier.map(costOf)))}`,
							c: "pt-dim",
						},
					]}
				/>
				<Rule title="default pick · reliability first" />
				<Line
					segs={[
						{ t: "> ", c: "pt-hi" },
						{ t: col(pick.provider, 10), c: "pt-inv" },
						{ t: ` ${EXIT_SHORT[pick.exitType]}  works ` },
						{ t: meter(worksOf(pick)), c: "pt-hi" },
						{
							t: ` ${pct(worksOf(pick))}   ${usd(costOf(pick))}/GB   fraud ${pick.fraudScore}`,
						},
					]}
				/>
				<Line
					segs={[
						{
							t: "  floor 95% met · cheaper Rayobyte skipped (93%)",
							c: "pt-dim",
						},
					]}
				/>
				<Line segs={[{ t: "" }]} />
				<Line
					segs={[
						{ t: " ENTER ", c: "pt-inv" },
						{ t: " create route   " },
						{ t: " TAB ", c: "pt-key" },
						{ t: " walk frontier   " },
						{ t: " R ", c: "pt-key" },
						{ t: " risk dial   " },
						{ t: " ? ", c: "pt-key" },
						{ t: " help   " },
						{ t: "_", c: "pt-cursor" },
					]}
				/>
				<Rule left="└" right="┘" />
			</pre>
		</div>
	);
}
