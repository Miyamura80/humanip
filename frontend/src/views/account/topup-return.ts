// Detect the browser coming back from checkout. The contract returns to
// `/#/account?topup=success`; a `/billing/success` path or a `?topup=success`
// before the hash are accepted too, in case the server's success URL differs.

const hashQuery = () => {
	const h = window.location.hash;
	const q = h.indexOf("?");
	return new URLSearchParams(q >= 0 ? h.slice(q + 1) : "");
};

export function isTopUpReturn(): boolean {
	const { pathname, search } = window.location;
	return (
		pathname.replace(/\/+$/, "").endsWith("/billing/success") ||
		new URLSearchParams(search).get("topup") === "success" ||
		hashQuery().get("topup") === "success"
	);
}

const BALANCE_KEY = "humanip.balanceBeforeTopUp";

/** Drop the return markers so a reload doesn't show the banner again. */
export function clearTopUpReturn(): void {
	window.history.replaceState(null, "", "/#/account");
	try {
		window.sessionStorage.removeItem(BALANCE_KEY);
	} catch {
		// Nothing was stored.
	}
}

/** Remember the balance before leaving for checkout (per tab). */
export function rememberBalance(balance: number | undefined): void {
	try {
		if (balance === undefined) window.sessionStorage.removeItem(BALANCE_KEY);
		else window.sessionStorage.setItem(BALANCE_KEY, String(balance));
	} catch {
		// Storage blocked; the return page falls back to its first reading.
	}
}

/** The balance saved by `rememberBalance`, if any. */
export function rememberedBalance(): number | undefined {
	try {
		const v = window.sessionStorage.getItem(BALANCE_KEY);
		return v === null || Number.isNaN(Number(v)) ? undefined : Number(v);
	} catch {
		return undefined;
	}
}
