import { useEffect, useState } from "react";
import "./styles/app.css";
import "./styles/systems.css";
import { Mark } from "./components/icons";
import { ACCOUNT } from "./lib/mock";
import { usd } from "./lib/policy";
import { Dashboard } from "./views/Dashboard";
import { PriceLookup } from "./views/PriceLookup";
import { Quickstart } from "./views/Quickstart";
import { RouteBuilder } from "./views/RouteBuilder";
import { Systems } from "./views/Systems";

const VIEWS = {
	routes: { label: "Routes", el: <Dashboard /> },
	new: { label: "New route", el: <RouteBuilder /> },
	lookup: { label: "Price lookup", el: <PriceLookup /> },
	connect: { label: "Connect", el: <Quickstart /> },
	systems: { label: "Systems", el: <Systems /> },
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
			<header className="topnav">
				<a href="#/routes" className="brand">
					<span className="brand-mark">
						<Mark name="tunnel" size={18} />
					</span>
					humanip
				</a>
				<nav className="nav" aria-label="Main">
					{(Object.keys(VIEWS) as ViewKey[]).map((k) => (
						<a
							key={k}
							href={`#/${k}`}
							className={view === k ? "on" : ""}
							aria-current={view === k ? "page" : undefined}
						>
							{VIEWS[k].label}
						</a>
					))}
				</nav>
				<div className="balance-pill">
					<span className="muted">Balance</span>
					<b>{usd(ACCOUNT.balance)}</b>
					<button type="button" className="btn btn-small">
						Top up
					</button>
				</div>
			</header>
			<main className="main">{VIEWS[view].el}</main>
		</div>
	);
}

export default App;
