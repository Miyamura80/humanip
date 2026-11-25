import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { createHmac, timingSafeEqual } from "node:crypto";
import { createFakeStripe } from "../src/server";
import { signatureHeader, signPayload } from "../src/sign";

const SECRET = "whsec_test_secret";

const j = (r: Response): Promise<any> => r.json();

/** Independent verifier mirroring crates/billing/src/webhook.rs. */
function verify(payload: string, header: string, secret: string): boolean {
	const parts = Object.fromEntries(
		header.split(",").map((p) => p.split("=", 2) as [string, string]),
	);
	const t = parts.t;
	const v1 = parts.v1;
	if (!t || !v1) return false;
	if (Math.abs(Date.now() / 1000 - Number(t)) > 300) return false;
	const want = createHmac("sha256", secret).update(`${t}.${payload}`).digest();
	const got = Buffer.from(v1, "hex");
	return got.length === want.length && timingSafeEqual(got, want);
}

describe("signature", () => {
	test("matches the Rust known vector", () => {
		// Same vector as crates/billing/src/webhook.rs known_vector_matches_hmac_sha256.
		const body = '{"id":"evt_1","type":"checkout.session.completed"}';
		expect(signPayload(body, SECRET, 1_795_608_000)).toBe(
			"34c1ab9e329a6020092b40c34d8773e14b9f0751d4b9d178736c4536361d37e2",
		);
	});

	test("header verifies against a reference HMAC", () => {
		const body = JSON.stringify({ hello: "world" });
		const t = Math.floor(Date.now() / 1000);
		const header = signatureHeader(body, SECRET, t);
		const ref = createHmac("sha256", SECRET)
			.update(`${t}.${body}`)
			.digest("hex");
		expect(header).toBe(`t=${t},v1=${ref}`);
		expect(verify(body, header, SECRET)).toBe(true);
		expect(verify(`${body} `, header, SECRET)).toBe(false);
		expect(verify(body, header, "whsec_other")).toBe(false);
	});
});

describe("checkout flow", () => {
	const received: { headers: Headers; body: string }[] = [];
	let webhookStatus = 200;
	let receiver: ReturnType<typeof Bun.serve>;
	let stripe: ReturnType<typeof createFakeStripe>;

	beforeAll(() => {
		receiver = Bun.serve({
			hostname: "127.0.0.1",
			port: 0,
			fetch: async (req) => {
				received.push({ headers: req.headers, body: await req.text() });
				return Response.json({ received: true }, { status: webhookStatus });
			},
		});
		stripe = createFakeStripe({
			webhookSecret: SECRET,
			webhookUrl: `http://127.0.0.1:${receiver.port}/api/v1/billing/webhook`,
			publicUrl: "http://localhost:12111",
		});
	});
	afterAll(() => receiver.stop(true));

	function form(pairs: [string, string][]): string {
		return new URLSearchParams(pairs).toString();
	}

	// The exact fields crates/billing/src/stripe.rs checkout_form sends.
	const rustForm = form([
		["mode", "payment"],
		["line_items[0][quantity]", "1"],
		["line_items[0][price_data][currency]", "usd"],
		["line_items[0][price_data][product_data][name]", "humanip credit"],
		["line_items[0][price_data][unit_amount]", "2500"],
		["client_reference_id", "acct_1"],
		["metadata[account_id]", "acct_1"],
		["payment_method_types[]", "card"],
		["payment_method_types[]", "crypto"],
		[
			"success_url",
			"http://localhost:1420/billing/success?session_id={CHECKOUT_SESSION_ID}",
		],
		["cancel_url", "http://localhost:1420/billing/cancel"],
	]);

	async function create(body = rustForm, auth = "Bearer sk_test_fake") {
		return stripe.fetch(
			new Request("http://localhost:12111/v1/checkout/sessions", {
				method: "POST",
				headers: {
					authorization: auth,
					"content-type": "application/x-www-form-urlencoded",
				},
				body,
			}),
		);
	}

	test("healthz", async () => {
		const res = await stripe.fetch(new Request("http://x/healthz"));
		expect(await j(res)).toEqual({ ok: true });
	});

	test("requires a Bearer key and valid amount", async () => {
		expect((await create(rustForm, "")).status).toBe(401);
		expect((await create(form([["success_url", "a"]]))).status).toBe(400);
	});

	test("create, view, pay posts a signed webhook and redirects", async () => {
		const res = await create();
		expect(res.status).toBe(200);
		const s = await j(res);
		expect(s.id).toMatch(/^cs_test_[0-9a-f]+$/);
		expect(s.url).toBe(`http://localhost:12111/pay/${s.id}`);
		expect(s.amount_total).toBe(2500);
		expect(s.metadata).toEqual({ account_id: "acct_1" });

		const page = await stripe.fetch(new Request(`http://x/pay/${s.id}`));
		const text = await page.text();
		expect(text).toContain("Fake Stripe Checkout");
		expect(text).toContain("$25.00");
		expect(text).toContain(">Pay</button>");
		expect(text).toContain('href="http://localhost:1420/billing/cancel"');

		const paid = await stripe.fetch(
			new Request(`http://x/pay/${s.id}`, { method: "POST" }),
		);
		expect(paid.status).toBe(303);
		expect(paid.headers.get("location")).toBe(
			`http://localhost:1420/billing/success?session_id=${s.id}`,
		);

		expect(received.length).toBe(1);
		const hook = received[0];
		if (!hook) throw new Error("no webhook");
		const sig = hook.headers.get("stripe-signature") ?? "";
		expect(verify(hook.body, sig, SECRET)).toBe(true);
		const ev = JSON.parse(hook.body);
		expect(ev.type).toBe("checkout.session.completed");
		expect(ev.data.object).toMatchObject({
			id: s.id,
			payment_status: "paid",
			amount_total: 2500,
			currency: "usd",
			client_reference_id: "acct_1",
			metadata: { account_id: "acct_1" },
		});
		expect(stripe.sessions.get(s.id)?.payment_status).toBe("paid");
	});

	test("failed webhook delivery does not redirect", async () => {
		const s = await j(await create());
		webhookStatus = 400;
		const res = await stripe.fetch(
			new Request(`http://x/pay/${s.id}`, { method: "POST" }),
		);
		webhookStatus = 200;
		expect(res.status).toBe(502);
		expect(stripe.sessions.get(s.id)?.payment_status).toBe("unpaid");
	});

	test("unknown session is 404", async () => {
		const res = await stripe.fetch(new Request("http://x/pay/cs_test_nope"));
		expect(res.status).toBe(404);
	});
});
