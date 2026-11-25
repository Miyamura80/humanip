import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { j, makeApp, PUBLIC_URL, TOKEN } from "./helpers";
import { MockControl } from "./mock-control";

const mock = new MockControl(TOKEN);
let h: Awaited<ReturnType<typeof makeApp>>;

beforeAll(async () => {
	mock.start();
	h = await makeApp(mock.url);
});
afterAll(() => mock.stop());

describe("auth and /me", () => {
	test("signup then /me returns account_id and balance", async () => {
		const u = h.client();
		const res = await u.signUp("alice@example.com");
		expect(res.status).toBe(200);
		const setCookie = res.headers.getSetCookie().join("\n");
		expect(setCookie).toContain("HttpOnly");
		expect(setCookie).toContain("SameSite=Lax");

		const create = mock.requests.find(
			(r) => r.method === "POST" && r.path === "/api/v1/control/accounts",
		);
		expect(create?.body).toEqual({ name: "alice@example.com" });
		expect(create?.auth).toBe(`Bearer ${TOKEN}`);

		const acctId = [...mock.accounts.entries()].find(
			([, a]) => a.name === "alice@example.com",
		)?.[0];
		if (!acctId) throw new Error("no account created");
		const acct = mock.accounts.get(acctId);
		if (acct) acct.balance_micro = 12_345_678;

		const me = await u.call("GET", "/app/api/me");
		expect(me.status).toBe(200);
		const body = await j(me);
		expect(body.user.email).toBe("alice@example.com");
		expect(body.account).toEqual({
			account_id: acctId,
			balance_usd: 12.345678,
		});
	});

	test("401 without a session on every app route", async () => {
		const anon = h.client();
		const routes: [string, string][] = [
			["GET", "/app/api/me"],
			["GET", "/app/api/keys"],
			["POST", "/app/api/keys"],
			["POST", "/app/api/keys/key_1/rotate"],
			["POST", "/app/api/keys/key_1/revoke"],
			["GET", "/app/api/usage"],
			["POST", "/app/api/checkout"],
		];
		for (const [method, path] of routes) {
			const res = await anon.call(
				method,
				path,
				method === "POST" ? {} : undefined,
			);
			expect(res.status).toBe(401);
		}
	});

	test("/config is public and comes from env", async () => {
		const res = await h.client().call("GET", "/app/api/config");
		expect(await j(res)).toEqual({
			gateway_host: "gw.test",
			socks_port: 1081,
			http_port: 8081,
		});
	});

	test("cross-origin POST is rejected", async () => {
		const u = h.client();
		await u.signUp("csrf@example.com");
		const res = await h.app.request("/app/api/keys", {
			method: "POST",
			headers: {
				origin: "https://evil.example",
				cookie: u.cookie,
				"content-type": "application/json",
			},
			body: JSON.stringify({ name: "x" }),
		});
		expect(res.status).toBe(403);
		const pre = await h.app.request("/app/api/me", {
			method: "OPTIONS",
			headers: {
				origin: "https://evil.example",
				"access-control-request-method": "GET",
			},
		});
		expect(pre.headers.get("access-control-allow-origin")).not.toBe(
			"https://evil.example",
		);
	});

	test("better-auth rate limiting is on", async () => {
		const u = h.client();
		const statuses: number[] = [];
		for (let i = 0; i < 5; i++) {
			const res = await u.call("POST", "/api/auth/sign-in/email", {
				email: "nobody@example.com",
				password: "wrong-password",
			});
			statuses.push(res.status);
		}
		expect(statuses).toContain(429);
	});

	test("oversized bodies are rejected", async () => {
		const u = h.client();
		await u.signUp("big@example.com");
		const res = await u.call("POST", "/app/api/keys", {
			name: "a".repeat(100_000),
		});
		expect(res.status).toBe(413);
	});
});

describe("keys", () => {
	test("create, list, rotate, revoke use the session's account", async () => {
		const u = h.client();
		await u.signUp("keys@example.com");
		const me = await j(await u.call("GET", "/app/api/me"));
		const acctId: string = me.account.account_id;
		const before = mock.requests.length;

		const created = await u.call("POST", "/app/api/keys", {
			name: "laptop",
			monthly_cap_usd: 25,
		});
		expect(created.status).toBe(200);
		expect(created.headers.get("cache-control")).toBe("no-store");
		const c = await j(created);
		expect(c.secret).toStartWith("hip_secret_");
		expect(c.key).toEqual({
			key_id: c.key.key_id,
			name: "laptop",
			revoked: false,
			monthly_cap_usd: 25,
		});

		const list = await j(await u.call("GET", "/app/api/keys"));
		expect(list).toEqual([c.key]);
		expect(JSON.stringify(list)).not.toContain("hip_secret_");

		const rot = await u.call("POST", `/app/api/keys/${c.key.key_id}/rotate`);
		expect((await j(rot)).secret).toStartWith("hip_rotated_");

		const rev = await u.call("POST", `/app/api/keys/${c.key.key_id}/revoke`);
		expect(await j(rev)).toEqual({ ok: true });
		const after = await j(await u.call("GET", "/app/api/keys"));
		expect(after[0].revoked).toBe(true);

		const ids = new Set(mock.accountIdsRequested(before));
		expect([...ids]).toEqual([acctId]);
	});

	test("key input validation", async () => {
		const u = h.client();
		await u.signUp("valid@example.com");
		for (const body of [
			{},
			{ name: "" },
			{ name: "a".repeat(65) },
			{ name: "ok", monthly_cap_usd: 0 },
			{ name: "ok", monthly_cap_usd: 100_001 },
			{ name: "ok", monthly_cap_usd: "5" },
		]) {
			const res = await u.call("POST", "/app/api/keys", body);
			expect(res.status).toBe(400);
		}
		const ok = await u.call("POST", "/app/api/keys", {
			name: "nocap",
			monthly_cap_usd: null,
		});
		expect((await j(ok)).key.monthly_cap_usd).toBeNull();
	});

	test("a user cannot act on another user's account", async () => {
		const a = h.client();
		const b = h.client();
		await a.signUp("owner@example.com");
		await b.signUp("intruder@example.com");
		const aAcct = (await j(await a.call("GET", "/app/api/me"))).account
			.account_id;
		const bAcct = (await j(await b.call("GET", "/app/api/me"))).account
			.account_id;
		expect(aAcct).not.toBe(bAcct);
		const key = (
			await j(await a.call("POST", "/app/api/keys", { name: "mine" }))
		).key;

		const before = mock.requests.length;
		// b tries a's key, and tries to smuggle a's account id in the body/query.
		const rot = await b.call("POST", `/app/api/keys/${key.key_id}/rotate`);
		expect(rot.status).toBe(404);
		const rev = await b.call("POST", `/app/api/keys/${key.key_id}/revoke`, {
			account_id: aAcct,
		});
		expect(rev.status).toBe(404);
		await b.call("GET", `/app/api/keys?account_id=${aAcct}`);
		await b.call("POST", "/app/api/checkout", {
			amount_usd: 10,
			account_id: aAcct,
		});
		const ids = mock.accountIdsRequested(before);
		expect(ids.length).toBeGreaterThan(0);
		expect(ids.every((id) => id === bAcct)).toBe(true);
		const aKeys = await j(await a.call("GET", "/app/api/keys"));
		expect(aKeys[0].revoked).toBe(false);
	});
});

describe("usage and checkout", () => {
	test("usage maps micro-USD to USD and sums", async () => {
		const u = h.client();
		await u.signUp("usage@example.com");
		const t0 = Math.floor(Date.now() / 1000);
		const res = await u.call("GET", "/app/api/usage?days=7");
		const body = await j(res);
		expect(body.total_usd).toBe(1.75);
		expect(body.rows[0]).toEqual({
			key_id: "key_a",
			provider: "netnut-isp",
			domain: "example.com",
			bytes: 1024,
			total_usd: 1.5,
		});
		const req = mock.requests.findLast((r) => r.path.includes("/usage"));
		const since = Number(
			new URL(`http://x${req?.path}`).searchParams.get("since"),
		);
		expect(Math.abs(since - (t0 - 7 * 86_400))).toBeLessThan(5);
		expect((await u.call("GET", "/app/api/usage?days=0")).status).toBe(400);
	});

	test("checkout bounds and URL passthrough", async () => {
		const u = h.client();
		await u.signUp("pay@example.com");
		const me = await j(await u.call("GET", "/app/api/me"));
		for (const amount_usd of [4.99, 5000.01, 0, -5, "10", null]) {
			const res = await u.call("POST", "/app/api/checkout", { amount_usd });
			expect(res.status).toBe(400);
		}
		for (const amount_usd of [5, 5000]) {
			const res = await u.call("POST", "/app/api/checkout", { amount_usd });
			expect(res.status).toBe(200);
			const body = await j(res);
			expect(body.url).toStartWith("http://localhost:12111/pay/cs_test_");
		}
		const sent = mock.requests.findLast(
			(r) => r.path === "/api/v1/billing/checkout",
		);
		expect(sent?.auth).toBe(`Bearer ${TOKEN}`);
		expect(sent?.body).toEqual({
			account_id: me.account.account_id,
			amount_usd: 5000,
			success_url: `${PUBLIC_URL}/#/account?topup=success`,
			cancel_url: `${PUBLIC_URL}/#/account?topup=cancel`,
		});
	});
});

describe("control API down", () => {
	test("signup succeeds, routes 503, account created lazily on recovery", async () => {
		mock.failAccounts = true;
		const u = h.client();
		const res = await u.signUp("lazy@example.com");
		expect(res.status).toBe(200);
		expect(
			[...mock.accounts.values()].some((a) => a.name === "lazy@example.com"),
		).toBe(false);

		const down = await u.call("GET", "/app/api/me");
		expect(down.status).toBe(503);
		expect(await j(down)).toEqual({ error: "account service unavailable" });

		mock.failAccounts = false;
		const up = await u.call("GET", "/app/api/me");
		expect(up.status).toBe(200);
		const acctId = (await j(up)).account.account_id;
		expect(mock.accounts.get(acctId)?.name).toBe("lazy@example.com");
		// Later calls reuse the stored link instead of creating another account.
		await u.call("GET", "/app/api/me");
		const count = [...mock.accounts.values()].filter(
			(a) => a.name === "lazy@example.com",
		).length;
		expect(count).toBe(1);
	});

	test("unreachable control API: signup ok, app routes 503", async () => {
		const dead = await makeApp("http://127.0.0.1:1");
		const u = dead.client();
		expect((await u.signUp("offline@example.com")).status).toBe(200);
		expect((await u.call("GET", "/app/api/me")).status).toBe(503);
		expect((await u.call("GET", "/app/api/keys")).status).toBe(503);
	});
});
