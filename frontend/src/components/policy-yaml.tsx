// The New route policy as a valid `routes.yaml`, with a Validate button that
// asks `appctl serve` (`validate_routes`) what the limits resolve to.

import { useState } from "react";
import { validateRoutes } from "../api/client";
import type { ValidatedRoute } from "../api/types";
import type { ExitType, PinLoss, RouteMode } from "../lib/policy";
import { formatSticky } from "../lib/policy";

/** UI TTL choice -> `StickyTtl` in YAML. */
const TTL_YAML: Record<string, string> = {
	session: "session",
	"24h": "{ hours: 24 }",
	"30d": "{ days: 30 }",
	forever: "forever",
};

interface Policy {
	name: string;
	mode: RouteMode;
	ttl: string;
	loss: PinLoss;
	exits: ExitType[];
	risk: number;
	floor: number;
	targetUrl: string;
}

export function policyYaml(p: Policy): string {
	const sticky =
		p.mode === "sticky"
			? `    sticky:
      ttl: ${TTL_YAML[p.ttl] ?? "session"}
      on_pin_loss: ${p.loss}
`
			: "";
	return `routes:
  - name: ${p.name}
    mode: ${p.mode}
${sticky}    exit_types: [${p.exits.join(", ")}]
    geo:
      country: DE
      city: Berlin
      asn: 3320
      match_account: true
    risk: ${p.risk}
    reliability_floor: ${p.floor}
    target_url: ${p.targetUrl}
`;
}

type Outcome =
	| { kind: "idle" }
	| { kind: "busy" }
	| { kind: "ok"; routes: ValidatedRoute[] }
	| { kind: "err"; reason: string };

export function PolicyYaml({ yaml }: { yaml: string }) {
	const [out, setOut] = useState<Outcome>({ kind: "idle" });
	const [checked, setChecked] = useState("");
	const run = async () => {
		setOut({ kind: "busy" });
		const r = await validateRoutes(yaml);
		setChecked(yaml);
		setOut(
			r.ok
				? { kind: "ok", routes: r.data.routes }
				: { kind: "err", reason: r.reason },
		);
	};
	const stale = out.kind !== "idle" && out.kind !== "busy" && checked !== yaml;
	return (
		<details className="dig glass step">
			<summary>Policy as YAML</summary>
			<div className="dig-body">
				<pre className="code">{yaml}</pre>
				<div className="validate-row">
					<button
						type="button"
						className="btn btn-small"
						onClick={run}
						disabled={out.kind === "busy"}
					>
						{out.kind === "busy" ? "Validating" : "Validate"}
					</button>
					<span className="faint">
						{stale
							? "Policy changed since the last check"
							: "via appctl validate_routes"}
					</span>
				</div>
				{out.kind === "err" && (
					<div className="validate-out bad" role="alert">
						{out.reason === "unreachable"
							? "appctl serve is not reachable. Start it with make run."
							: out.reason}
					</div>
				)}
				{out.kind === "ok" &&
					out.routes.map((r) => (
						<div key={r.name} className="validate-out kvs">
							<span>Route</span>
							<b className="mono">{r.name}</b>
							<span>Fraud at most</span>
							<b className="mono">{r.limits.max_fraud_score}</b>
							<span>Exit classes</span>
							<b className="mono">{r.limits.allowed_exit_types.join(", ")}</b>
							<span>Sourcing</span>
							<b className="mono">{r.limits.sourcing_tiers.join(", ")}</b>
							<span>Pin guarantee</span>
							<b className="mono">
								{formatSticky(r.limits.min_sticky_minutes)}
							</b>
						</div>
					))}
			</div>
		</details>
	);
}
