/**
 * `/app/api/*`: browser-facing routes (docs/product-api.md section 2).
 *
 * Every route except `/config` needs a better-auth session. The account id
 * always comes from the session user's stored link, never from the request.
 */
import { type Context, Hono } from "hono";
import type { AccountLinks } from "./accounts";
import type { Auth } from "./auth";
import { type ControlClient, ControlError, type ControlKey } from "./control";
import type { WebApiEnv } from "./env";

const MICROS_PER_USD = 1_000_000;
const MIN_TOPUP_USD = 5;
const MAX_TOPUP_USD = 5_000;
const MAX_CAP_USD = 100_000;

interface SessionUser {
	id: string;
	email: string;
	name: string;
}

type Vars = { user: SessionUser; accountId: string };

function microToUsd(micro: number): number {
	return micro / MICROS_PER_USD;
}

function publicKey(k: ControlKey) {
	return {
		key_id: k.key_id,
		name: k.name,
		revoked: k.revoked,
		monthly_cap_usd:
			k.monthly_cap_micro === null || k.monthly_cap_micro === undefined
				? null
				: microToUsd(k.monthly_cap_micro),
	};
}

function fail(c: Context, status: 400 | 401 | 404 | 502 | 503, error: string) {
	return c.json({ error }, status);
}

function controlFailure(c: Context, e: unknown) {
	if (e instanceof ControlError) {
		if (e.unavailable) return fail(c, 503, "account service unavailable");
		if (e.status === 404) return fail(c, 404, "not found");
		if (e.status === 400 || e.status === 422) return fail(c, 400, e.message);
		return fail(c, 502, "account service error");
	}
	console.error("web-api: unexpected error", (e as Error)?.name ?? "unknown");
	return fail(c, 502, "internal error");
}

async function jsonBody(c: Context): Promise<Record<string, unknown> | null> {
	try {
		const body = await c.req.json();
		return body && typeof body === "object" && !Array.isArray(body)
			? (body as Record<string, unknown>)
			: null;
	} catch {
		return null;
	}
}

/** `null` and absent mean no cap; otherwise 0 < usd <= 100000. */
function parseCap(
	v: unknown,
): { ok: true; cap: number | null } | { ok: false } {
	if (v === undefined || v === null) return { ok: true, cap: null };
	if (typeof v !== "number" || !Number.isFinite(v)) return { ok: false };
	if (v <= 0 || v > MAX_CAP_USD) return { ok: false };
	return { ok: true, cap: v };
}

export function appRoutes(
	env: WebApiEnv,
	auth: Auth,
	links: AccountLinks,
	control: ControlClient,
) {
	const r = new Hono<{ Variables: Vars }>();

	// Public: the connect-snippet values are not secret.
	r.get("/config", (c) =>
		c.json({
			gateway_host: env.gatewayHost,
			socks_port: env.socksPort,
			http_port: env.httpPort,
		}),
	);

	r.use("*", async (c, next) => {
		const session = await auth.api.getSession({ headers: c.req.raw.headers });
		if (!session) return fail(c, 401, "not signed in");
		const { id, email, name } = session.user;
		let accountId: string;
		try {
			accountId = await links.ensure(id, email);
		} catch (e) {
			// Any failure to provision (down, misconfigured token, route not
			// deployed) is the service's problem, never the user's: 503.
			const status = e instanceof ControlError ? e.status : "error";
			console.warn(`web-api: account provisioning failed (${status})`);
			return fail(c, 503, "account service unavailable");
		}
		c.set("user", { id, email, name });
		c.set("accountId", accountId);
		c.header("cache-control", "no-store");
		await next();
	});

	r.get("/me", async (c) => {
		const user = c.get("user");
		try {
			const acct = await control.getAccount(c.get("accountId"));
			return c.json({
				user,
				account: {
					account_id: acct.account_id,
					balance_usd: microToUsd(acct.balance_micro),
				},
			});
		} catch (e) {
			return controlFailure(c, e);
		}
	});

	r.get("/keys", async (c) => {
		try {
			const keys = await control.listKeys(c.get("accountId"));
			return c.json(keys.map(publicKey));
		} catch (e) {
			return controlFailure(c, e);
		}
	});

	r.post("/keys", async (c) => {
		const body = await jsonBody(c);
		if (!body) return fail(c, 400, "body must be a JSON object");
		const name = typeof body.name === "string" ? body.name.trim() : "";
		if (name.length < 1 || name.length > 64) {
			return fail(c, 400, "name must be 1-64 characters");
		}
		const cap = parseCap(body.monthly_cap_usd);
		if (!cap.ok) {
			return fail(c, 400, "monthly_cap_usd must be null or 0 < usd <= 100000");
		}
		try {
			const out = await control.createKey(c.get("accountId"), name, cap.cap);
			// The secret is passed straight through and never stored or logged.
			return c.json({ key: publicKey(out.key), secret: out.secret });
		} catch (e) {
			return controlFailure(c, e);
		}
	});

	r.post("/keys/:keyId/rotate", async (c) => {
		try {
			const out = await control.rotateKey(
				c.get("accountId"),
				c.req.param("keyId"),
			);
			return c.json({ secret: out.secret });
		} catch (e) {
			return controlFailure(c, e);
		}
	});

	r.post("/keys/:keyId/revoke", async (c) => {
		try {
			await control.revokeKey(c.get("accountId"), c.req.param("keyId"));
			return c.json({ ok: true });
		} catch (e) {
			return controlFailure(c, e);
		}
	});

	r.get("/usage", async (c) => {
		const raw = c.req.query("days");
		const days = raw === undefined ? 30 : Number(raw);
		if (!Number.isInteger(days) || days < 1 || days > 366) {
			return fail(c, 400, "days must be an integer from 1 to 366");
		}
		const since = Math.floor(Date.now() / 1000) - days * 86_400;
		try {
			const rows = await control.usage(c.get("accountId"), since);
			let totalMicro = 0;
			const out = rows.map((row) => {
				totalMicro += row.total_micro;
				return {
					key_id: row.key_id,
					provider: row.provider,
					domain: row.domain,
					bytes: row.bytes,
					total_usd: microToUsd(row.total_micro),
				};
			});
			return c.json({ rows: out, total_usd: microToUsd(totalMicro) });
		} catch (e) {
			return controlFailure(c, e);
		}
	});

	r.post("/checkout", async (c) => {
		const body = await jsonBody(c);
		const amount = body?.amount_usd;
		if (
			typeof amount !== "number" ||
			!Number.isFinite(amount) ||
			amount < MIN_TOPUP_USD ||
			amount > MAX_TOPUP_USD
		) {
			return fail(c, 400, "amount_usd must be between 5 and 5000");
		}
		try {
			const session = await control.checkout({
				account_id: c.get("accountId"),
				amount_usd: amount,
				success_url: `${env.publicUrl}/#/account?topup=success`,
				cancel_url: `${env.publicUrl}/#/account?topup=cancel`,
			});
			return c.json({ url: session.url });
		} catch (e) {
			return controlFailure(c, e);
		}
	});

	return r;
}
