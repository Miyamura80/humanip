// Top-up dialog: presets or a custom amount, then a hosted checkout
// (`POST /app/api/checkout` returns the URL; card or USDC is chosen there).
// One dialog for the whole app, opened from the nav or the Account page.

import { useState, useSyncExternalStore } from "react";
import { checkout } from "../../api/app";
import { useMe } from "../../api/resource";
import { Dialog } from "../../components/dialog";
import { usd } from "../../lib/policy";
import { rememberBalance } from "./topup-return";

const PRESETS = [10, 25, 100];
const MIN = 5;
const MAX = 5000;

let open = false;
const listeners = new Set<() => void>();
const setOpen = (v: boolean) => {
	open = v;
	for (const l of listeners) l();
};
const subscribe = (l: () => void) => {
	listeners.add(l);
	return () => {
		listeners.delete(l);
	};
};

export const openTopUp = () => setOpen(true);

/** Mount once; renders the dialog while it is open. */
export function TopUpHost() {
	const isOpen = useSyncExternalStore(subscribe, () => open);
	return isOpen ? <TopUpDialog onClose={() => setOpen(false)} /> : null;
}

function TopUpDialog({ onClose }: { onClose: () => void }) {
	const [preset, setPreset] = useState<number | null>(25);
	const [custom, setCustom] = useState("");
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState("");
	const me = useMe();

	const amount = preset ?? Number(custom);
	const valid = Number.isFinite(amount) && amount >= MIN && amount <= MAX;

	const pay = async () => {
		if (!valid) return;
		setBusy(true);
		setError("");
		const r = await checkout(Math.round(amount * 100) / 100);
		if (r.ok) {
			rememberBalance(me.data?.account.balance_usd);
			window.location.assign(r.data.url);
			return;
		}
		setBusy(false);
		setError(r.reason);
	};

	return (
		<Dialog title="Top up credit" onClose={onClose}>
			<p className="caption" style={{ marginTop: 0 }}>
				Prepaid. Usage is billed at provider cost plus 5%.
			</p>
			<fieldset className="preset-row" aria-label="Amount">
				{PRESETS.map((p) => (
					<button
						type="button"
						key={p}
						aria-pressed={preset === p}
						className={`preset${preset === p ? " on" : ""}`}
						onClick={() => {
							setPreset(p);
							setCustom("");
						}}
					>
						{usd(p, 0)}
					</button>
				))}
			</fieldset>
			<label className="field" htmlFor="topup-custom">
				<span className="field-label">
					Custom amount ({usd(MIN, 0)} to {usd(MAX, 0)})
				</span>
				<input
					id="topup-custom"
					className="input mono"
					type="number"
					inputMode="decimal"
					min={MIN}
					max={MAX}
					step="1"
					placeholder="e.g. 50"
					value={custom}
					onFocus={() => setPreset(null)}
					onChange={(e) => {
						setPreset(null);
						setCustom(e.target.value);
					}}
				/>
			</label>
			{!valid && preset === null && custom !== "" && (
				<p className="form-error">
					Enter between {usd(MIN, 0)} and {usd(MAX, 0)}.
				</p>
			)}
			{error && (
				<p className="form-error" role="alert">
					{error}
				</p>
			)}
			<button
				type="button"
				className="btn btn-primary wide"
				disabled={!valid || busy}
				onClick={pay}
				style={{ marginTop: 18 }}
			>
				{busy
					? "Opening checkout..."
					: `Pay ${valid ? usd(amount) : ""} with card or USDC`}
			</button>
		</Dialog>
	);
}
