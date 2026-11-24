import { useEffect, useState } from "react";
import "./styles/app.css";
import "./styles/visual.css";
import { ACCOUNT } from "./lib/mock";
import { usd } from "./lib/policy";
import { Dashboard } from "./views/Dashboard";
import { PriceLookup } from "./views/PriceLookup";
import { Quickstart } from "./views/Quickstart";
import { RouteBuilder } from "./views/RouteBuilder";

const VIEWS = {
	routes: { label: "Routes", icon: "🗺️", el: <Dashboard /> },
	new: { label: "New route", icon: "➕", el: <RouteBuilder /> },
	lookup: { label: "Price lookup", icon: "💲", el: <PriceLookup /> },
	connect: { label: "Connect", icon: "🔌", el: <Quickstart /> },
} as const;

type ViewKey = keyof typeof VIEWS;

const fromHash = (): ViewKey => {
	const h = window.location.hash.replace("#/", "");
	return h in VIEWS ? (h as ViewKey) : "routes";
};

function App() {
	const [view, setView] = useState<ViewKey>(fromHash);

	useEffect(() => {
		const onHash = () => setView(fromHash());
		window.addEventListener("hashchange", onHash);
		return () => window.removeEventListener("hashchange", onHash);
	}, []);

	return (
		<div className="shell">
			<aside className="sidebar">
				<div className="brand">
					<span className="brand-mark">hip</span>
					humanip
				</div>
				{(Object.keys(VIEWS) as ViewKey[]).map((k) => (
					<a
						key={k}
						href={`#/${k}`}
						className={`nav-item${view === k ? " active" : ""}`}
					>
						<span className="mono faint" style={{ width: 14 }}>
							{VIEWS[k].icon}
						</span>
						{VIEWS[k].label}
					</a>
				))}
				<div className="nav-section">Account</div>
				<span className="nav-item">
					<span className="mono faint" style={{ width: 14 }}>
						🔑
					</span>
					API keys
				</span>
				<span className="nav-item">
					<span className="mono faint" style={{ width: 14 }}>
						🧾
					</span>
					Usage & billing
				</span>
				<div className="sidebar-foot">
					<div className="faint" style={{ fontSize: 11 }}>
						💰 balance
					</div>
					<div className="balance">{usd(ACCOUNT.balance)}</div>
					<button type="button" className="btn" style={{ width: "100%" }}>
						💳 / 🪙 Top up
					</button>
				</div>
			</aside>
			<main className="main">{VIEWS[view].el}</main>
		</div>
	);
}

export default App;
