import { useState } from "react";
import { Logo, Mark } from "../components/icons";
import { CopyField, Segmented } from "../components/ui";
import { Flow } from "../components/visual";
import ThinkingOrb from "../vendor/ThinkingOrb";

type Proto = "socks5" | "http";
type Snippet = "playwright" | "browser-use" | "mautrix" | "curl";

const GW = "gw.humanip.io";
const KEY = "hip_live_7Qx2Fv9c";

/** Each username segment, colour-coded with what it controls. */
const RAW_PARTS: { text: string; color: string; label?: string }[] = [
	{ text: "user-", color: "var(--text-3)" },
	{ text: "route_agent", color: "var(--accent)", label: "saved policy" },
	{ text: "-", color: "var(--text-3)" },
	{ text: "session_r42", color: "var(--mobile)", label: "same IP" },
	{ text: "-", color: "var(--text-3)" },
	{ text: "risk_40", color: "var(--warn)", label: "risk dial" },
	{ text: "-", color: "var(--text-3)" },
	{ text: "country_us", color: "var(--residential)", label: "geo" },
	{ text: ":", color: "var(--text-3)" },
	{ text: "KEY", color: "var(--bad)", label: "API key" },
	{ text: "@", color: "var(--text-3)" },
	{ text: GW, color: "var(--text-2)", label: "gateway" },
];
const PARTS = RAW_PARTS.map((p, n) => ({ ...p, id: `part-${n}` }));

const MORE = [
	"city_berlin",
	"asn_3320",
	"exit_mobile",
	"exit_isp",
	"exit_residential",
	"target_linkedin.com",
];

export function Quickstart() {
	const [proto, setProto] = useState<Proto>("socks5");
	const [snippet, setSnippet] = useState<Snippet>("playwright");
	const port = proto === "socks5" ? 1080 : 8080;

	return (
		<>
			<div className="page-head">
				<div>
					<div className="eyebrow">Connect</div>
					<h1>Any SOCKS5 or HTTP client. No SDK.</h1>
				</div>
				<Segmented
					value={proto}
					onChange={setProto}
					options={[
						{ value: "socks5", label: "SOCKS5 :1080" },
						{ value: "http", label: "HTTP CONNECT :8080" },
					]}
				/>
			</div>

			<section className="glass step">
				<div className="section-head">
					<Logo name="claude-code" size={22} label="Agent" />
					<h2>Ad-hoc: settings ride in the username</h2>
				</div>
				<div className="anatomy">
					<span className="plain">{proto}://</span>
					{PARTS.map((p) => (
						<span key={p.id} className="seg-part" style={{ color: p.color }}>
							<code
								style={
									p.label ? undefined : { borderBottomColor: "transparent" }
								}
							>
								{p.text}
							</code>
							{p.label && <span className="seg-lbl">{p.label}</span>}
						</span>
					))}
					<span className="plain">:{port}</span>
				</div>
				<details className="dig" style={{ marginTop: 14 }}>
					<summary>
						More keys: city, ASN, exit type, target. Leave out session for
						rotating.
					</summary>
					<div className="dig-body chips">
						{MORE.map((k) => (
							<span key={k} className="chip mono">
								{k}
							</span>
						))}
					</div>
				</details>
			</section>

			<div className="grid-2">
				<section className="glass step">
					<div className="section-head">
						<Logo name="beeper" size={22} label="Bridge" />
						<h2>Named route: policy stays server-side</h2>
					</div>
					<Flow
						use="mautrix"
						exit="mobile"
						target="wa"
						mode="sticky"
						caption="172.58.201.44 · pinned 41d"
					/>
					<div style={{ marginTop: 26 }}>
						<CopyField
							value={`${proto}://rt_wa_bridge:${KEY}@rt-wa-bridge.${GW}:${port}`}
						/>
					</div>
				</section>
				<section className="glass step">
					<div className="section-head">
						<ThinkingOrb state="searching" size={24} color="#7fdfff" />
						<h2>Check your exit</h2>
					</div>
					<pre className="code">{`$ curl -x ${proto}://…@${GW}:${port} https://${GW}/whoami
{ "ip": "172.58.201.44", "type": "mobile",
  "asn": 21928, "fraud": 4, "pinned": "41d" }`}</pre>
					<div className="row faint" style={{ gap: 8, marginTop: 12 }}>
						<Mark
							name="shield-check"
							size={14}
							style={{ color: "var(--accent)" }}
						/>
						Reputation is checked against IPQS and Scamalytics before an exit is
						handed out.
					</div>
				</section>
			</div>

			<section className="glass step">
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
			</section>
		</>
	);
}

const SNIPPETS: Record<Snippet, (p: Proto, port: number) => string> = {
	playwright: (p, port) => `const browser = await chromium.launch({
  proxy: {
    server: "${p}://${GW}:${port}",
    username: \`user-route_agent-session_\${runId}-risk_40\`,
    password: process.env.HUMANIP_KEY,
  },
});`,
	"browser-use": (p, port) => `profile = BrowserProfile(proxy={
    "server": "${p}://${GW}:${port}",
    "username": f"user-session_{run_id}-target_linkedin.com",
    "password": os.environ["HUMANIP_KEY"],
})`,
	mautrix: (p, port) => `# config.yaml
proxy:
  url: ${p}://rt_wa_bridge:\${HUMANIP_KEY}@rt-wa-bridge.${GW}:${port}`,
	curl: (p, port) => `curl -H "Authorization: Bearer $HUMANIP_KEY" \\
  "https://api.humanip.io/v1/quote?url=https://linkedin.com"

curl -x ${p}://user-session_demo:$HUMANIP_KEY@${GW}:${port} https://${GW}/whoami`,
};
