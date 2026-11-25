// Typed client for the web-api app routes (`/app/api/*`, docs/product-api.md
// section 2). Every route but `/config` needs the session cookie; a 401 sends
// the browser to `#/login`. Calls return a Result and never throw.

import type { Result } from "./types";

const TIMEOUT_MS = 8000;

export interface Me {
	user: { id: string; email: string; name: string };
	account: { account_id: string; balance_usd: number };
}

export interface ApiKey {
	key_id: string;
	name: string;
	revoked: boolean;
	monthly_cap_usd: number | null;
}

export interface AppUsageRow {
	key_id: string;
	provider: string;
	domain: string;
	bytes: number;
	total_usd: number;
}

interface Usage {
	rows: AppUsageRow[];
	total_usd: number;
}

export interface GatewayConfig {
	gateway_host: string;
	socks_port: number;
	http_port: number;
}

const isObj = (v: unknown): v is Record<string, unknown> =>
	!!v && typeof v === "object";

const UNREACHABLE = "The account service is unreachable.";

function errorText(body: unknown, status: number): string {
	if (isObj(body) && typeof body.error === "string") return body.error;
	if (isObj(body) && isObj(body.error) && "message" in body.error)
		return String(body.error.message);
	if (body === null && (status === 500 || status === 502)) return UNREACHABLE;
	return `HTTP ${status}`;
}

async function call<T>(
	path: string,
	check: (v: unknown) => boolean,
	init: RequestInit = {},
): Promise<Result<T>> {
	const ctl = new AbortController();
	const timer = setTimeout(() => ctl.abort(), TIMEOUT_MS);
	try {
		const res = await fetch(`/app/api/${path}`, {
			credentials: "same-origin",
			...init,
			signal: ctl.signal,
		});
		if (res.status === 401) {
			if (!window.location.hash.startsWith("#/login"))
				window.location.hash = "#/login";
			return { ok: false, reason: "Signed out" };
		}
		const text = await res.text();
		let body: unknown = null;
		try {
			body = text ? JSON.parse(text) : null;
		} catch {
			return { ok: false, reason: `HTTP ${res.status}, not JSON` };
		}
		if (!res.ok) return { ok: false, reason: errorText(body, res.status) };
		if (!check(body)) return { ok: false, reason: "Unexpected response" };
		return { ok: true, data: body as T };
	} catch (e) {
		const aborted = e instanceof DOMException && e.name === "AbortError";
		return {
			ok: false,
			reason: aborted ? "The account service timed out." : UNREACHABLE,
		};
	} finally {
		clearTimeout(timer);
	}
}

const post = (body?: unknown): RequestInit => ({
	method: "POST",
	headers: { "content-type": "application/json" },
	body: JSON.stringify(body ?? {}),
});

const hasSecret = (v: unknown) => isObj(v) && typeof v.secret === "string";

export const getMe = () =>
	call<Me>("me", (v) => isObj(v) && isObj(v.user) && isObj(v.account));

export const listKeys = () => call<ApiKey[]>("keys", Array.isArray);

export const createKey = (name: string, monthly_cap_usd: number | null) =>
	call<{ key: ApiKey; secret: string }>(
		"keys",
		(v) => hasSecret(v) && isObj((v as { key?: unknown }).key),
		post({ name, monthly_cap_usd }),
	);

export const rotateKey = (id: string) =>
	call<{ secret: string }>(
		`keys/${encodeURIComponent(id)}/rotate`,
		hasSecret,
		post(),
	);

export const revokeKey = (id: string) =>
	call<{ ok: true }>(`keys/${encodeURIComponent(id)}/revoke`, isObj, post());

export const getUsage = (days = 30) =>
	call<Usage>(`usage?days=${days}`, (v) => isObj(v) && Array.isArray(v.rows));

export const checkout = (amount_usd: number) =>
	call<{ url: string }>(
		"checkout",
		(v) => isObj(v) && typeof v.url === "string",
		post({ amount_usd }),
	);

export const getConfig = () =>
	call<GatewayConfig>(
		"config",
		(v) => isObj(v) && typeof v.gateway_host === "string",
	);
