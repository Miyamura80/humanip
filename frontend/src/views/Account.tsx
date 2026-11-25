// Account: balance and top-up, API keys, 30-day usage and a quick-connect
// snippet. All data comes from the web-api (`/app/api/*`).

import { useEffect, useState } from "react";
import { getUsage, listKeys } from "../api/app";
import { refreshMe, useMe, useResource } from "../api/resource";
import { Mark } from "../components/icons";
import { usd } from "../lib/policy";
import { KeysPanel } from "./account/Keys";
import { QuickConnect } from "./account/QuickConnect";
import { openTopUp } from "./account/TopUp";
import {
	clearTopUpReturn,
	isTopUpReturn,
	rememberedBalance,
} from "./account/topup-return";
import { UsagePanel } from "./account/Usage";

const POLL_MS = 2000;
const POLL_FOR_MS = 20000;

type Return = "none" | "waiting" | "credited" | "slow";

/** After checkout, poll `/me` until the balance moves (up to 20 s). */
function useTopUpReturn(): Return {
	const [state, setState] = useState<Return>(() =>
		isTopUpReturn() ? "waiting" : "none",
	);
	const [before] = useState(rememberedBalance);
	useEffect(() => {
		if (state !== "waiting") return;
		clearTopUpReturn();
		let cancelled = false;
		(async () => {
			const first = await refreshMe();
			const now = first.ok ? first.data.account.balance_usd : undefined;
			const base = before ?? now;
			if (base !== undefined && now !== undefined && now !== base) {
				if (!cancelled) setState("credited");
				return;
			}
			const until = Date.now() + POLL_FOR_MS;
			while (!cancelled && Date.now() < until) {
				await new Promise((r) => setTimeout(r, POLL_MS));
				const r = await refreshMe();
				if (r.ok && base !== undefined && r.data.account.balance_usd !== base) {
					if (!cancelled) setState("credited");
					return;
				}
			}
			if (!cancelled) setState("slow");
		})();
		return () => {
			cancelled = true;
		};
	}, [state, before]);
	return state;
}

const RETURN_COPY: Record<Exclude<Return, "none">, [string, string]> = {
	waiting: ["Payment received", "Waiting for the credit to land..."],
	credited: ["Top-up complete", "Your new balance is shown below."],
	slow: [
		"Payment is processing",
		"Credit can take a minute to post. It will appear here once it does.",
	],
};

export function Account() {
	const me = useMe();
	const ret = useTopUpReturn();
	const keys = useResource(listKeys);
	const usage = useResource(() => getUsage(30));
	const balance = me.data?.account.balance_usd;

	return (
		<>
			<div className="page-head">
				<div>
					<div className="eyebrow">Account</div>
					<h1>
						{me.data
							? `Hi, ${me.data.user.name || me.data.user.email}`
							: "Your account"}
					</h1>
				</div>
			</div>

			{ret !== "none" && (
				<output
					className={`note ${ret === "slow" ? "note-warn" : "note-good"}`}
				>
					<Mark
						name={ret === "slow" ? "shield-alert" : "shield-check"}
						size={18}
					/>
					<span>
						<b>{RETURN_COPY[ret][0]}.</b> {RETURN_COPY[ret][1]}
					</span>
				</output>
			)}

			<div className="account-top">
				<section className="glass step balance-card">
					<div className="eyebrow">Prepaid balance</div>
					{balance !== undefined ? (
						<div className="num-xl balance-num">
							{usd(balance)}
							<small>Provider cost + 5%, deducted per GB as you use it</small>
						</div>
					) : me.status === "error" ? (
						<p className="form-error">Could not load balance: {me.error}</p>
					) : (
						<div className="num-xl balance-num skeleton">$-.--</div>
					)}
					<div className="row" style={{ marginTop: 26 }}>
						<button
							type="button"
							className="btn btn-primary"
							onClick={openTopUp}
						>
							Top up
						</button>
						<span className="faint">Card or USDC</span>
					</div>
				</section>
				<QuickConnect keys={keys.data ?? []} />
			</div>

			<KeysPanel
				keys={keys.data ?? []}
				loading={keys.status === "loading"}
				error={keys.status === "error" ? keys.error : undefined}
				onChange={keys.reload}
			/>
			<UsagePanel
				rows={usage.data?.rows ?? []}
				total={usage.data?.total_usd}
				loading={usage.status === "loading"}
				error={usage.status === "error" ? usage.error : undefined}
			/>
		</>
	);
}
