// Top navs. Public: brand plus sign-in / sign-up. App: product items, an
// "Operator" group for the gateway-admin screens, the real balance from
// `/app/api/me`, Top up, and the signed-in email with Sign out.

import { useState } from "react";
import { signOut } from "../api/auth";
import { resetMe, useMe } from "../api/resource";
import { usd } from "../lib/policy";
import { openTopUp } from "../views/account/TopUp";
import { Mark } from "./icons";
import { LivePill, SettingsPopover } from "./livepill";

const PRODUCT = [
	["account", "Account"],
	["lookup", "Price lookup"],
	["connect", "Connect"],
] as const;

const OPERATOR = [
	["routes", "Routes"],
	["new", "New route"],
	["systems", "Systems"],
] as const;

const OP_KEY = "humanip.operator";
const readOp = () => {
	try {
		return window.localStorage.getItem(OP_KEY) === "1";
	} catch {
		return false;
	}
};
const writeOp = (on: boolean) => {
	try {
		window.localStorage.setItem(OP_KEY, on ? "1" : "0");
	} catch {
		// Not remembered; harmless.
	}
};

function Brand() {
	return (
		<a href="#/" className="brand">
			<span className="brand-mark">
				<Mark name="tunnel" size={18} />
			</span>
			humanip
		</a>
	);
}

function NavLink({
	k,
	label,
	view,
}: {
	k: string;
	label: string;
	view: string;
}) {
	const on = view === k;
	return (
		<a
			href={`#/${k}`}
			className={on ? "on" : ""}
			aria-current={on ? "page" : undefined}
		>
			{label}
		</a>
	);
}

export function PublicNav({
	signedIn,
	view,
}: {
	signedIn: boolean;
	view: string;
}) {
	return (
		<header className="topnav">
			<Brand />
			<span style={{ flex: 1 }} />
			{signedIn ? (
				<a href="#/account" className="btn btn-small btn-primary">
					Open console
				</a>
			) : (
				<div className="row" style={{ gap: 6 }}>
					<a
						href="#/login"
						className={`btn btn-small${view === "login" ? " btn-quiet-on" : " btn-quiet"}`}
					>
						Sign in
					</a>
					<a href="#/signup" className="btn btn-small btn-primary">
						Create account
					</a>
				</div>
			)}
		</header>
	);
}

export function AppNav({ view, email }: { view: string; email: string }) {
	const isOpView = OPERATOR.some(([k]) => k === view);
	const [op, setOp] = useState(readOp);
	const showOp = op || isOpView;
	const me = useMe();
	const balance = me.data?.account.balance_usd;

	const out = async () => {
		await signOut();
		resetMe();
		window.location.hash = "#/";
	};

	return (
		<header className="topnav">
			<Brand />
			<nav className="nav" aria-label="Main">
				{PRODUCT.map(([k, label]) => (
					<NavLink key={k} k={k} label={label} view={view} />
				))}
				<button
					type="button"
					className={`op-toggle${showOp ? " on" : ""}`}
					aria-expanded={showOp}
					title="Gateway operator screens (admin token)"
					onClick={() => {
						setOp(!showOp);
						writeOp(!showOp);
					}}
				>
					<Mark name="administrator" size={14} />
					Operator
				</button>
				{showOp && (
					<span className="op-group">
						{OPERATOR.map(([k, label]) => (
							<NavLink key={k} k={k} label={label} view={view} />
						))}
					</span>
				)}
			</nav>
			<LivePill />
			{showOp && <SettingsPopover />}
			<div className="balance-pill">
				<span className="muted">Balance</span>
				<b title={me.status === "error" ? me.error : undefined}>
					{balance !== undefined
						? usd(balance)
						: me.status === "error"
							? "--"
							: "..."}
				</b>
				<button type="button" className="btn btn-small" onClick={openTopUp}>
					Top up
				</button>
			</div>
			<div className="user-pill">
				<span className="muted user-email" title={email}>
					{email}
				</span>
				<button type="button" className="btn btn-small" onClick={out}>
					Sign out
				</button>
			</div>
		</header>
	);
}
