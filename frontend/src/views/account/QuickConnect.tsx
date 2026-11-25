// Ready-to-copy proxy URLs. The password is the API key (never stored
// client-side, so it stays a placeholder); the username carries per-connection
// settings for the gateway, e.g. `user-session_run1` for a sticky exit.

import type { ApiKey } from "../../api/app";
import { useGatewayConfig } from "../../api/resource";
import { Mark } from "../../components/icons";
import { CopyField } from "../../components/ui";

export function QuickConnect({ keys }: { keys: ApiKey[] }) {
	const cfg = useGatewayConfig();
	const key = [...keys].reverse().find((k) => !k.revoked);
	const user = "user-session_run1";
	const host = cfg?.gateway_host ?? "gw.humanip.io";
	const socks = cfg?.socks_port ?? 1080;
	const http = cfg?.http_port ?? 8080;
	return (
		<section className="glass step">
			<div className="section-head spread">
				<div className="row">
					<Mark name="tunnel" size={20} />
					<h2>Quick connect</h2>
				</div>
				<a href="#/connect" className="btn btn-small">
					All options
				</a>
			</div>
			<div className="stack" style={{ gap: 12 }}>
				<div className="field">
					<span className="field-label">SOCKS5</span>
					<CopyField value={`socks5://${user}:<YOUR_KEY>@${host}:${socks}`} />
				</div>
				<div className="field">
					<span className="field-label">HTTP CONNECT</span>
					<CopyField value={`http://${user}:<YOUR_KEY>@${host}:${http}`} />
				</div>
			</div>
			<p className="faint" style={{ marginTop: 14 }}>
				{key
					? `Replace <YOUR_KEY> with the secret of "${key.name}". Same session_ value, same exit IP; drop it to rotate per connection.`
					: "Create a key first, then replace <YOUR_KEY> with its secret."}
				{!cfg && " Gateway address is a default until the server answers."}
			</p>
		</section>
	);
}
