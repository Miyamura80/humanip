// Ready-to-copy proxy URLs for the newest active key. Only the key id is
// shown; the secret is never stored client-side, so it stays a placeholder.

import type { ApiKey } from "../../api/app";
import { useGatewayConfig } from "../../api/resource";
import { Mark } from "../../components/icons";
import { CopyField } from "../../components/ui";

export function QuickConnect({ keys }: { keys: ApiKey[] }) {
	const cfg = useGatewayConfig();
	const key = [...keys].reverse().find((k) => !k.revoked);
	const user = key ? key.key_id : "<KEY_ID>";
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
					? `Using key "${key.name}". Replace <YOUR_KEY> with its secret.`
					: "Create a key first, then replace <YOUR_KEY> with its secret."}
				{!cfg && " Gateway address is a default until the server answers."}
			</p>
		</section>
	);
}
