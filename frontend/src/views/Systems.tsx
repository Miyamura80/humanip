// Design-system exploration: four directions that differ in layout,
// density, chart form, type and interaction model, not only colour.

import { Blueprint } from "./systems/Blueprint";
import { Graphite } from "./systems/Graphite";
import { Orbital } from "./systems/Orbital";
import { Phosphor } from "./systems/Phosphor";

const SYSTEMS = [
	{
		id: "graphite",
		name: "Graphite Console",
		tag: "Dense table + inspector. Keyboard-first. 12px.",
		El: Graphite,
	},
	{
		id: "phosphor",
		name: "Phosphor TUI",
		tag: "Text-mode screen. ASCII frontier. Box-drawing.",
		El: Phosphor,
	},
	{
		id: "blueprint",
		name: "Blueprint Schematic",
		tag: "Engineering drawing. Callouts, title block, dimension lines.",
		El: Blueprint,
	},
	{
		id: "orbital",
		name: "Cyan Orbital",
		tag: "Spatial + ambient. Polar frontier, orb avatars, chromatic metal.",
		El: Orbital,
	},
];

export function Systems() {
	return (
		<div className="ds-page">
			<div className="page-head">
				<h1>Design systems</h1>
			</div>
			{SYSTEMS.map(({ id, name, tag, El }) => (
				<section key={id} id={`ds-${id}`} className="ds-wrap">
					<div className="ds-caption">
						<b>{name}</b>
						<span>{tag}</span>
					</div>
					<El />
				</section>
			))}
		</div>
	);
}
