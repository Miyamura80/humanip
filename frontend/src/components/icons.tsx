// Icon layer. Marks come from https://eito.me/icons (robots, shields,
// personas, concepts, tool logos) plus three exit-type glyphs drawn in the
// same 24px stroke style. Monochrome marks render through a CSS mask so they
// take `currentColor` in any theme; brand logos keep their own colours.

import type { CSSProperties } from "react";

/** Monochrome marks, recoloured by the surrounding text colour. */
export type MarkName =
	| "robot"
	| "glitch-robot"
	| "broken-robot"
	| "dead-robot"
	| "devil-robot"
	| "shattered-robot"
	| "shield"
	| "shield-check"
	| "shield-alert"
	| "shield-x"
	| "shield-off"
	| "shield-user"
	| "developer"
	| "administrator"
	| "hacker-1"
	| "hacker-2"
	| "tunnel"
	| "reranking"
	| "mcp"
	| "exit-mobile"
	| "exit-isp"
	| "exit-residential";

/** Full-colour brand logos. */
type LogoName = "beeper" | "apify" | "claude-code" | "codex" | "cursor";

export function Mark({
	name,
	size = 20,
	label,
	style,
	className,
}: {
	name: MarkName;
	size?: number;
	/** Accessible name; omit for purely decorative use next to a text label. */
	label?: string;
	style?: CSSProperties;
	className?: string;
}) {
	const url = `url(/icons/${name}.svg)`;
	const props = {
		className: `mark${className ? ` ${className}` : ""}`,
		style: {
			width: size,
			height: size,
			WebkitMaskImage: url,
			maskImage: url,
			...style,
		},
	};
	return label ? (
		<span role="img" aria-label={label} {...props} />
	) : (
		<span aria-hidden="true" {...props} />
	);
}

export function Logo({
	name,
	size = 20,
	label,
}: {
	name: LogoName;
	size?: number;
	label: string;
}) {
	return (
		<img
			src={`/icons/${name}.svg`}
			width={size}
			height={size}
			alt={label}
			style={{ display: "block" }}
		/>
	);
}
