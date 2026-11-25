import { useEffect, useState } from "react";
import "./styles/app.css";
import "./styles/systems.css";
import "./styles/live.css";
import "./styles/product.css";
import { useSession } from "./api/auth";
import { Mark } from "./components/icons";
import { AppNav, PublicNav } from "./components/nav";
import { Account } from "./views/Account";
import { Login, Signup } from "./views/Auth";
import { TopUpHost } from "./views/account/TopUp";
import { Dashboard } from "./views/Dashboard";
import { Landing } from "./views/Landing";
import { PriceLookup } from "./views/PriceLookup";
import { Quickstart } from "./views/Quickstart";
import { RouteBuilder } from "./views/RouteBuilder";
import { Systems } from "./views/Systems";

const PUBLIC = {
	"": <Landing />,
	signup: <Signup />,
	login: <Login />,
} as const;

/** Everything here needs a session. */
const APP = {
	account: <Account />,
	lookup: <PriceLookup />,
	connect: <Quickstart />,
	new: <RouteBuilder />,
	routes: <Dashboard />,
	systems: <Systems />,
} as const;

type PublicKey = keyof typeof PUBLIC;
type AppKey = keyof typeof APP;
type ViewKey = PublicKey | AppKey;

const fromLocation = (): ViewKey => {
	// Checkout may return to a `/billing/success` path instead of the hash.
	if (window.location.pathname.replace(/\/+$/, "").endsWith("/billing/success"))
		return "account";
	const h = window.location.hash.replace(/^#\/?/, "").split("?")[0];
	return h in PUBLIC || h in APP ? (h as ViewKey) : "";
};

const isApp = (v: ViewKey): v is AppKey => v in APP;

function App() {
	const [view, setView] = useState<ViewKey>(fromLocation);
	const session = useSession();
	const signedIn = !!session.data;
	const checking = session.isPending;

	useEffect(() => {
		const onHash = () => setView(fromLocation());
		window.addEventListener("hashchange", onHash);
		return () => window.removeEventListener("hashchange", onHash);
	}, []);

	// Gate the app area; bounce signed-in users off the auth pages.
	useEffect(() => {
		if (checking) return;
		if (isApp(view) && !signedIn) window.location.hash = "#/login";
		if ((view === "login" || view === "signup") && signedIn)
			window.location.hash = "#/account";
	}, [checking, signedIn, view]);

	const appView = isApp(view);

	return (
		<div className="shell">
			{appView && signedIn ? (
				<AppNav view={view} email={session.data?.user.email ?? ""} />
			) : (
				<PublicNav signedIn={signedIn} view={view} />
			)}
			<main className={`main${appView ? "" : " main-public"}`}>
				{appView ? (
					signedIn ? (
						APP[view]
					) : (
						<div className="gate faint">
							<Mark name="shield" size={28} />
							{checking
								? "Checking your session..."
								: "Redirecting to sign in..."}
						</div>
					)
				) : (
					PUBLIC[view]
				)}
			</main>
			{signedIn && <TopUpHost />}
		</div>
	);
}

export default App;
