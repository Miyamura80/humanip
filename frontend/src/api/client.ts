// Typed fetch client for `appctl serve` (/api/v1) and the gateway admin API
// (/admin/v1). Each call has a timeout and returns a Result, never throws,
// so a screen can fall back to demo data without try/catch of its own.

import type {
	GatewayStatus,
	QuoteInput,
	QuoteOutput,
	Result,
	UsageRow,
	ValidateRoutesOutput,
} from "./types";

const TIMEOUT_MS = 4000;
const TOKEN_KEY = "humanip.adminToken";

/** Token saved in the settings popover, if any. */
export function savedAdminToken(): string {
	try {
		return window.localStorage.getItem(TOKEN_KEY) ?? "";
	} catch {
		// Storage blocked (private mode, sandboxed iframe).
		return "";
	}
}

export function saveAdminToken(token: string): void {
	try {
		if (token) window.localStorage.setItem(TOKEN_KEY, token);
		else window.localStorage.removeItem(TOKEN_KEY);
	} catch {
		// Not persisted; the next reload falls back to VITE_ADMIN_TOKEN.
	}
}

/** Admin bearer token: the settings popover wins over VITE_ADMIN_TOKEN. */
const adminToken = () =>
	savedAdminToken() || (import.meta.env.VITE_ADMIN_TOKEN ?? "");

/** Pull a readable message out of an error body (`{error:{message}}`). */
function errorText(body: unknown, status: number): string {
	if (isObj(body) && "error" in body) {
		const e = body.error;
		if (isObj(e) && "message" in e) return String(e.message);
		if (typeof e === "string") return e;
	}
	// The dev proxy answers an empty 500/502 when nothing listens upstream.
	if (body === null && (status === 500 || status === 502)) return "unreachable";
	return status === 503 ? "HTTP 503, unavailable" : `HTTP ${status}`;
}

const isObj = (v: unknown): v is Record<string, unknown> =>
	!!v && typeof v === "object";

async function request<T>(
	path: string,
	init: RequestInit,
	check: (v: unknown) => boolean,
): Promise<Result<T>> {
	const ctl = new AbortController();
	const timer = setTimeout(() => ctl.abort(), TIMEOUT_MS);
	try {
		const res = await fetch(path, { ...init, signal: ctl.signal });
		const text = await res.text();
		let body: unknown;
		try {
			body = text ? JSON.parse(text) : null;
		} catch {
			return { ok: false, reason: `HTTP ${res.status}, not JSON` };
		}
		if (!res.ok) return { ok: false, reason: errorText(body, res.status) };
		if (!check(body)) return { ok: false, reason: "unexpected response shape" };
		return { ok: true, data: body as T };
	} catch (e) {
		const aborted = e instanceof DOMException && e.name === "AbortError";
		return {
			ok: false,
			reason: aborted ? `timed out after ${TIMEOUT_MS / 1000}s` : "unreachable",
		};
	} finally {
		clearTimeout(timer);
	}
}

function command<T>(
	name: string,
	body: unknown,
	check: (v: unknown) => boolean,
): Promise<Result<T>> {
	return request<T>(
		`/api/v1/commands/${name}`,
		{
			method: "POST",
			headers: { "content-type": "application/json" },
			body: JSON.stringify(body),
		},
		check,
	);
}

function admin<T>(
	path: string,
	check: (v: unknown) => boolean,
): Promise<Result<T>> {
	const token = adminToken();
	if (!token) return Promise.resolve({ ok: false, reason: "no admin token" });
	return request<T>(
		`/admin/v1/${path}`,
		{ headers: { authorization: `Bearer ${token}` } },
		check,
	);
}

export const quote = (input: QuoteInput) =>
	command<QuoteOutput>(
		"quote",
		input,
		(v) => isObj(v) && Array.isArray(v.offers),
	);

export const validateRoutes = (yaml: string) =>
	command<ValidateRoutesOutput>(
		"validate_routes",
		{ yaml },
		(v) => isObj(v) && Array.isArray(v.routes),
	);

export const status = () =>
	admin<GatewayStatus>(
		"status",
		(v) => isObj(v) && Array.isArray(v.routes) && Array.isArray(v.providers),
	);

/** Usage since a UNIX time in seconds. */
export const usage = (since: number) =>
	admin<UsageRow[]>(`usage?since=${Math.floor(since)}`, Array.isArray);
