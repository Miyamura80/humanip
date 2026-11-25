// Provider strip on the Routes screen: one chip per upstream provider, with
// an "Unverified" pill when its adapter has not been checked end to end.

import type { GatewayProvider } from "../api/types";
import { EXIT_LABEL, usd } from "../lib/policy";
import { Mark } from "./icons";
import { ExitMark } from "./visual";

export function ProviderStrip({ providers }: { providers: GatewayProvider[] }) {
	return (
		<section className="provider-strip" aria-label="Providers">
			{providers.map((p) => (
				<div
					key={p.name}
					className="glass provider-chip"
					title={`${p.name}: ${EXIT_LABEL[p.exit_type]}, ${p.sourcing} sourcing, fraud ${p.fraud_score}${p.countries.length ? `, ${p.countries.join(" ")}` : ""}`}
				>
					<ExitMark type={p.exit_type} size={16} />
					<b>{p.name}</b>
					<span className="faint mono">{usd(p.price_per_gb)}/GB</span>
					{p.unverified_adapter && (
						<span
							className="pill pill-warn"
							title={`The ${p.unverified_adapter} adapter has not been verified against the live provider API.`}
						>
							<Mark name="shield-alert" size={12} />
							Unverified
						</span>
					)}
				</div>
			))}
		</section>
	);
}
