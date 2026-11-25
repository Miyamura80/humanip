/**
 * In-process mock of the appctl control API (docs/product-api.md section 1),
 * on an ephemeral port. Records every request so tests can assert which
 * account ids web-api asked for.
 */

interface Recorded {
	method: string;
	path: string;
	auth: string | null;
	body: unknown;
}

interface MockKey {
	key_id: string;
	name: string;
	kyc_tier: string;
	monthly_cap_micro: number | null;
	revoked: boolean;
}

interface MockAccount {
	name: string;
	balance_micro: number;
	keys: MockKey[];
}

export class MockControl {
	readonly requests: Recorded[] = [];
	readonly accounts = new Map<string, MockAccount>();
	/** When set, `POST /accounts` answers 503 (control API down). */
	failAccounts = false;
	private seq = 0;
	private server: ReturnType<typeof Bun.serve> | null = null;

	constructor(readonly token: string) {}

	get url(): string {
		if (!this.server) throw new Error("mock not started");
		return `http://127.0.0.1:${this.server.port}`;
	}

	start(): this {
		this.server = Bun.serve({
			hostname: "127.0.0.1",
			port: 0,
			fetch: (req) => this.handle(req),
		});
		return this;
	}

	stop(): void {
		this.server?.stop(true);
		this.server = null;
	}

	/** Account ids named in requests recorded since index `from`, in order. */
	accountIdsRequested(from = 0): string[] {
		const ids: string[] = [];
		for (const r of this.requests.slice(from)) {
			const m = r.path.match(/^\/api\/v1\/control\/accounts\/([^/?]+)/);
			if (m?.[1]) ids.push(decodeURIComponent(m[1]));
			const body = r.body as { account_id?: string } | null;
			if (r.path === "/api/v1/billing/checkout" && body?.account_id) {
				ids.push(body.account_id);
			}
		}
		return ids;
	}

	private async handle(req: Request): Promise<Response> {
		const url = new URL(req.url);
		const text = await req.text();
		const body = text ? JSON.parse(text) : null;
		this.requests.push({
			method: req.method,
			path: url.pathname + url.search,
			auth: req.headers.get("authorization"),
			body,
		});
		if (req.headers.get("authorization") !== `Bearer ${this.token}`) {
			return Response.json({ error: "unauthorized" }, { status: 401 });
		}
		return this.route(req.method, url, body);
	}

	private route(method: string, url: URL, body: any): Response {
		const p = url.pathname;
		const nf = () => Response.json({ error: "not found" }, { status: 404 });
		if (method === "POST" && p === "/api/v1/control/accounts") {
			if (this.failAccounts) {
				return Response.json({ error: "down" }, { status: 503 });
			}
			const id = `acct_${++this.seq}`;
			this.accounts.set(id, { name: body.name, balance_micro: 0, keys: [] });
			return Response.json({ account_id: id });
		}
		if (method === "POST" && p === "/api/v1/billing/checkout") {
			if (!this.accounts.has(body.account_id)) return nf();
			const id = `cs_test_${++this.seq}`;
			return Response.json({ id, url: `http://localhost:12111/pay/${id}` });
		}
		const m = p.match(
			/^\/api\/v1\/control\/accounts\/([^/]+)(?:\/(keys|usage))?(?:\/([^/]+)\/(rotate|revoke))?$/,
		);
		if (!m) return nf();
		const [, rawId, sub, keyId, action] = m;
		const acct = this.accounts.get(decodeURIComponent(rawId ?? ""));
		if (!acct) return nf();
		const id = decodeURIComponent(rawId ?? "");
		if (!sub && method === "GET") {
			return Response.json({
				account_id: id,
				name: acct.name,
				balance_micro: acct.balance_micro,
			});
		}
		if (sub === "usage" && method === "GET") {
			return Response.json([
				{
					key_id: "key_a",
					provider: "netnut-isp",
					domain: "example.com",
					bytes: 1024,
					total_micro: 1_500_000,
				},
				{
					key_id: "key_a",
					provider: "iproxy-mobile",
					domain: "example.org",
					bytes: 2048,
					total_micro: 250_000,
				},
			]);
		}
		if (sub === "keys" && !keyId && method === "GET") {
			return Response.json(acct.keys);
		}
		if (sub === "keys" && !keyId && method === "POST") {
			const key: MockKey = {
				key_id: `key_${++this.seq}`,
				name: body.name,
				kyc_tier: "none",
				monthly_cap_micro:
					body.monthly_cap_usd === null
						? null
						: Math.round(body.monthly_cap_usd * 1_000_000),
				revoked: false,
			};
			acct.keys.push(key);
			return Response.json({ key, secret: `hip_secret_${this.seq}` });
		}
		if (sub === "keys" && keyId && method === "POST") {
			const key = acct.keys.find((k) => k.key_id === keyId);
			if (!key) return nf();
			if (action === "rotate") {
				return Response.json({ secret: `hip_rotated_${++this.seq}` });
			}
			key.revoked = true;
			return Response.json({ ok: true });
		}
		return nf();
	}
}
