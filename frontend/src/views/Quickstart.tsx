import { useState } from "react";
import { CopyField, PageHead, Segmented } from "../components/ui";

type Proto = "socks5" | "http";
type Snippet = "playwright" | "browser-use" | "mautrix" | "curl";

const GW = "gw.humanip.io";
const KEY = "hip_live_7Qx2Fv9c";

export function Quickstart() {
	const [proto, setProto] = useState<Proto>("socks5");
	const [snippet, setSnippet] = useState<Snippet>("playwright");
	const port = proto === "socks5" ? 1080 : 8080;

	const adhoc = `${proto}://user-route_agent-browser-session_\${RUN_ID}-risk_40:${KEY}@${GW}:${port}`;
	const named = `${proto}://rt_wa_bridge:${KEY}@rt-wa-bridge.${GW}:${port}`;

	return (
		<>
			<PageHead
				title="Connect"
				sub="Any SOCKS5 or HTTP client works. No SDK needed: the policy lives in the username or in a named route."
				actions={
					<Segmented
						value={proto}
						onChange={setProto}
						options={[
							{ value: "socks5", label: "SOCKS5 :1080" },
							{ value: "http", label: "HTTP CONNECT :8080" },
						]}
					/>
				}
			/>

			<div className="split" style={{ marginBottom: 16 }}>
				<div className="card card-pad">
					<div className="card-title">A · Ad-hoc session (username params)</div>
					<p className="muted" style={{ marginTop: 0 }}>
						Best for browser agents. Same{" "}
						<span className="mono">session_*</span> means same exit IP until the
						session goes idle.
					</p>
					<CopyField value={adhoc} />
					<table className="tbl" style={{ marginTop: 12 }}>
						<tbody>
							<Param
								k="route_<name>"
								v="Inherit a saved route policy (optional)"
							/>
							<Param k="session_<id>" v="Sticky key. Omit for rotating." />
							<Param k="risk_<0-100>" v="Override the route's risk dial" />
							<Param
								k="country_<cc> / city_<x> / asn_<n>"
								v="Geo constraints"
							/>
							<Param k="exit_<mobile|isp|residential>" v="Restrict exit type" />
							<Param
								k="target_<host>"
								v="Hint for cheapest-exit lookup (else read from CONNECT/SNI)"
							/>
						</tbody>
					</table>
				</div>
				<div className="card card-pad">
					<div className="card-title">B · Named route endpoint</div>
					<p className="muted" style={{ marginTop: 0 }}>
						Best for long-lived clients like bridges. The policy stays
						server-side and the client config never changes.
					</p>
					<CopyField value={named} />
					<div className="kv" style={{ marginTop: 12 }}>
						<span className="muted">Route</span>
						<span className="mono">mautrix-whatsapp</span>
					</div>
					<div className="kv">
						<span className="muted">Pinned exit</span>
						<span className="mono">172.58.201.44 · AS21928</span>
					</div>
					<div className="kv">
						<span className="muted">On pin loss</span>
						<span>Fail closed + webhook</span>
					</div>
					<div className="kv">
						<span className="muted">Check exit</span>
						<span className="mono">curl -x … https://{GW}/whoami</span>
					</div>
				</div>
			</div>

			<div className="card card-pad">
				<div className="tabs">
					{(["playwright", "browser-use", "mautrix", "curl"] as Snippet[]).map(
						(s) => (
							<button
								type="button"
								key={s}
								className={snippet === s ? "on" : ""}
								onClick={() => setSnippet(s)}
							>
								{s}
							</button>
						),
					)}
				</div>
				<pre className="code">{SNIPPETS[snippet](proto, port)}</pre>
			</div>
		</>
	);
}

function Param({ k, v }: { k: string; v: string }) {
	return (
		<tr>
			<td className="mono" style={{ color: "var(--accent)", width: "45%" }}>
				{k}
			</td>
			<td className="muted">{v}</td>
		</tr>
	);
}

const SNIPPETS: Record<Snippet, (p: Proto, port: number) => string> = {
	playwright: (p, port) => `import { chromium } from "playwright";

// One sticky exit per agent run; released when the session idles out.
const browser = await chromium.launch({
  proxy: {
    server: "${p}://${GW}:${port}",
    username: \`user-route_agent-browser-session_\${runId}-risk_40\`,
    password: process.env.HUMANIP_KEY,
  },
});`,
	"browser-use": (p, port) => `from browser_use import Agent, BrowserProfile

profile = BrowserProfile(proxy={
    "server": "${p}://${GW}:${port}",
    "username": f"user-route_agent-browser-session_{run_id}-target_linkedin.com",
    "password": os.environ["HUMANIP_KEY"],
})
agent = Agent(task=task, llm=llm, browser_profile=profile)`,
	mautrix: (p, port) => `# mautrix-whatsapp config.yaml
# The named route keeps the same mobile IP for the bridge's lifetime.
proxy:
  url: ${p}://rt_wa_bridge:\${HUMANIP_KEY}@rt-wa-bridge.${GW}:${port}
  # Optional: fetch the proxy URL dynamically (rotates creds, never the IP)
  get_proxy_url: https://api.humanip.io/v1/routes/rt_wa_bridge/proxy-url`,
	curl: (p, port) => `# Quote before connecting
curl -H "Authorization: Bearer $HUMANIP_KEY" \\
  "https://api.humanip.io/v1/quote?url=https://linkedin.com&risk=35"

# Connect through a sticky session
curl -x ${p}://user-session_demo-risk_35:$HUMANIP_KEY@${GW}:${port} https://${GW}/whoami`,
};
