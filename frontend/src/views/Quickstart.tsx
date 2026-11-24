import { useState } from "react";
import { CopyField, Segmented } from "../components/ui";
import { EXIT_ICON, Flow } from "../components/visual";

type Proto = "socks5" | "http";
type Snippet = "playwright" | "browser-use" | "mautrix" | "curl";

const GW = "gw.humanip.io";
const KEY = "hip_live_7Qx2Fv9c";

/** Each username segment, colour-coded with what it controls. */
const PARTS: { text: string; color: string; label?: string }[] = [
	{ text: "user-", color: "var(--text-3)" },
	{ text: "route_agent", color: "var(--accent)", label: "🗺️ policy" },
	{ text: "-", color: "var(--text-3)" },
	{ text: "session_r42", color: "var(--mobile)", label: "📌 same IP" },
	{ text: "-", color: "var(--text-3)" },
	{ text: "risk_40", color: "var(--warn)", label: "🛡️ risk" },
	{ text: "-", color: "var(--text-3)" },
	{ text: "country_us", color: "var(--isp)", label: "🌍 geo" },
	{ text: ":", color: "var(--text-3)" },
	{ text: "KEY", color: "var(--bad)", label: "🔑 key" },
	{ text: "@", color: "var(--text-3)" },
	{ text: GW, color: "var(--text-2)", label: "🚪 gateway" },
];

export function Quickstart() {
	const [proto, setProto] = useState<Proto>("socks5");
	const [snippet, setSnippet] = useState<Snippet>("playwright");
	const port = proto === "socks5" ? 1080 : 8080;

	return (
		<>
			<div className="page-head">
				<h1>🔌 Connect</h1>
				<Segmented
					value={proto}
					onChange={setProto}
					options={[
						{ value: "socks5", label: "🧦 SOCKS5 :1080" },
						{ value: "http", label: "🌐 HTTP :8080" },
					]}
				/>
			</div>

			<div className="card card-pad" style={{ marginBottom: 16 }}>
				<div className="row" style={{ marginBottom: 12, gap: 8 }}>
					<span className="big-icon">🤖</span>
					<b>Ad-hoc: settings ride in the username</b>
				</div>
				<div className="anatomy">
					<span className="plain">{proto}://</span>
					{PARTS.map((p, i) => (
						// biome-ignore lint/suspicious/noArrayIndexKey: static diagram
						<span key={i} className="seg-part" style={{ color: p.color }}>
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
				<details className="dig" style={{ marginTop: 12 }}>
					<summary>➕ more knobs: city_ · asn_ · exit_📱🏢🏠 · target_</summary>
					<div className="dig-body row" style={{ flexWrap: "wrap" }}>
						<span className="chip mono">city_berlin</span>
						<span className="chip mono">asn_3320</span>
						{(Object.keys(EXIT_ICON) as (keyof typeof EXIT_ICON)[]).map((e) => (
							<span key={e} className="chip mono">
								{EXIT_ICON[e]} exit_{e}
							</span>
						))}
						<span className="chip mono">🎯 target_linkedin.com</span>
						<span className="faint">omit session_ → 🔄 rotating</span>
					</div>
				</details>
			</div>

			<div className="split" style={{ marginBottom: 16 }}>
				<div className="card card-pad">
					<div className="row" style={{ marginBottom: 12, gap: 8 }}>
						<span className="big-icon">🌉</span>
						<b>Named route: policy stays server-side</b>
					</div>
					<Flow
						from="🌉"
						exit="mobile"
						ip="172.58.201.44 · 41d"
						mode="sticky"
					/>
					<div style={{ marginTop: 12 }}>
						<CopyField
							value={`${proto}://rt_wa_bridge:${KEY}@rt-wa-bridge.${GW}:${port}`}
						/>
					</div>
				</div>
				<div className="card card-pad">
					<div className="row" style={{ marginBottom: 12, gap: 8 }}>
						<span className="big-icon">🔍</span>
						<b>Check your exit</b>
					</div>
					<pre className="code">{`$ curl -x ${proto}://…@${GW}:${port} https://${GW}/whoami
{ "ip": "172.58.201.44", "type": "📱 mobile",
  "asn": 21928, "fraud": 4, "pinned": "41d" }`}</pre>
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
  "https://api.humanip.io/v1/quote?url=https://linkedin.com&risk=35"

curl -x ${p}://user-session_demo:$HUMANIP_KEY@${GW}:${port} https://${GW}/whoami`,
};
