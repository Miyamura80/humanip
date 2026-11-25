/**
 * Typed client for the appctl control API (docs/product-api.md section 1).
 *
 * Every call sends `Authorization: Bearer <HUMANIP_CONTROL_TOKEN>`. The token,
 * request bodies and responses (which can hold key secrets) are never logged.
 */

export interface ControlKey {
	key_id: string;
	name: string;
	kyc_tier?: string;
	monthly_cap_micro: number | null;
	revoked: boolean;
}

interface ControlAccount {
	account_id: string;
	name: string;
	balance_micro: number;
}

interface ControlUsageRow {
	key_id: string;
	provider: string;
	domain: string;
	bytes: number;
	total_micro: number;
}

/** A control API failure. `status` 0 means the service could not be reached. */
export class ControlError extends Error {
	constructor(
		readonly status: number,
		message: string,
	) {
		super(message);
		this.name = "ControlError";
	}

	/** True when the control service is down or failing, not a client error. */
	get unavailable(): boolean {
		return this.status === 0 || this.status >= 500;
	}
}

export interface ControlClientOptions {
	baseUrl: string;
	token: string;
	/** Per-attempt timeout. */
	timeoutMs?: number;
	/** Extra attempts for calls that are safe to retry. */
	retries?: number;
	/** First backoff delay; doubles per attempt. */
	backoffMs?: number;
}

const RETRYABLE_STATUS = new Set([502, 503, 504]);

function sleep(ms: number): Promise<void> {
	return new Promise((r) => setTimeout(r, ms));
}

function errorMessage(body: unknown, status: number): string {
	if (body && typeof body === "object" && "error" in body) {
		const e = (body as { error: unknown }).error;
		if (typeof e === "string") return e;
		if (e && typeof e === "object" && "message" in e) {
			const m = (e as { message: unknown }).message;
			if (typeof m === "string") return m;
		}
	}
	return `control API returned HTTP ${status}`;
}

export class ControlClient {
	private readonly baseUrl: string;
	private readonly token: string;
	private readonly timeoutMs: number;
	private readonly retries: number;
	private readonly backoffMs: number;

	constructor(opts: ControlClientOptions) {
		this.baseUrl = opts.baseUrl.replace(/\/+$/, "");
		this.token = opts.token;
		this.timeoutMs = opts.timeoutMs ?? 5000;
		this.retries = opts.retries ?? 2;
		this.backoffMs = opts.backoffMs ?? 200;
	}

	private async once<T>(method: string, path: string, body?: unknown) {
		let res: Response;
		try {
			res = await fetch(`${this.baseUrl}${path}`, {
				method,
				headers: {
					authorization: `Bearer ${this.token}`,
					...(body === undefined ? {} : { "content-type": "application/json" }),
				},
				body: body === undefined ? undefined : JSON.stringify(body),
				signal: AbortSignal.timeout(this.timeoutMs),
			});
		} catch {
			throw new ControlError(0, "control API unreachable");
		}
		const text = await res.text();
		let json: unknown = null;
		try {
			json = text ? JSON.parse(text) : null;
		} catch {
			// Fall through: a non-JSON body is reported by status below.
		}
		if (!res.ok)
			throw new ControlError(res.status, errorMessage(json, res.status));
		if (json === null) throw new ControlError(502, "control API sent no JSON");
		return json as T;
	}

	/**
	 * Send a request. `retry` is only set for calls where a repeat is harmless:
	 * reads, and account creation (a duplicate empty account is never linked).
	 */
	private async request<T>(
		method: string,
		path: string,
		body?: unknown,
		retry = false,
	): Promise<T> {
		const attempts = retry ? this.retries + 1 : 1;
		let last: ControlError = new ControlError(0, "control API unreachable");
		for (let i = 0; i < attempts; i++) {
			if (i > 0) await sleep(this.backoffMs * 2 ** (i - 1));
			try {
				return await this.once<T>(method, path, body);
			} catch (e) {
				if (!(e instanceof ControlError)) throw e;
				last = e;
				if (e.status !== 0 && !RETRYABLE_STATUS.has(e.status)) throw e;
			}
		}
		throw last;
	}

	private acct(accountId: string): string {
		return `/api/v1/control/accounts/${encodeURIComponent(accountId)}`;
	}

	createAccount(name: string): Promise<{ account_id: string }> {
		return this.request("POST", "/api/v1/control/accounts", { name }, true);
	}

	getAccount(accountId: string): Promise<ControlAccount> {
		return this.request("GET", this.acct(accountId), undefined, true);
	}

	listKeys(accountId: string): Promise<ControlKey[]> {
		return this.request("GET", `${this.acct(accountId)}/keys`, undefined, true);
	}

	createKey(
		accountId: string,
		name: string,
		monthlyCapUsd: number | null,
	): Promise<{ key: ControlKey; secret: string }> {
		return this.request("POST", `${this.acct(accountId)}/keys`, {
			name,
			monthly_cap_usd: monthlyCapUsd,
		});
	}

	rotateKey(accountId: string, keyId: string): Promise<{ secret: string }> {
		return this.request(
			"POST",
			`${this.acct(accountId)}/keys/${encodeURIComponent(keyId)}/rotate`,
		);
	}

	revokeKey(accountId: string, keyId: string): Promise<{ ok: boolean }> {
		return this.request(
			"POST",
			`${this.acct(accountId)}/keys/${encodeURIComponent(keyId)}/revoke`,
		);
	}

	usage(accountId: string, sinceUnix: number): Promise<ControlUsageRow[]> {
		return this.request(
			"GET",
			`${this.acct(accountId)}/usage?since=${sinceUnix}`,
			undefined,
			true,
		);
	}

	checkout(req: {
		account_id: string;
		amount_usd: number;
		success_url: string;
		cancel_url: string;
	}): Promise<{ id: string; url: string }> {
		return this.request("POST", "/api/v1/billing/checkout", req);
	}
}
