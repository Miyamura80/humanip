/**
 * Fake Stripe: just enough of Checkout for local end-to-end runs.
 *
 * ```
 * appctl --POST /v1/checkout/sessions--> fake-stripe   (store session)
 * browser --GET /pay/:id-------------->  fake-stripe   (HTML: Pay / Cancel)
 * browser --POST /pay/:id------------->  fake-stripe --signed webhook--> appctl
 *         <--303 success_url------------
 * ```
 *
 * Never point real customers at this. It takes any Bearer key.
 */
import { signatureHeader } from "./sign";

interface FakeStripeOptions {
	/** Secret shared with appctl's `STRIPE_WEBHOOK_SECRET`. */
	webhookSecret: string;
	/** Where `checkout.session.completed` is POSTed. */
	webhookUrl: string;
	/** Base of the `url` returned for each session, e.g. `http://localhost:12111`. */
	publicUrl: string;
}

interface FakeSession {
	id: string;
	object: "checkout.session";
	mode: string;
	status: "open" | "complete";
	payment_status: "unpaid" | "paid";
	amount_total: number;
	currency: string;
	client_reference_id: string | null;
	metadata: Record<string, string>;
	success_url: string;
	cancel_url: string;
	url: string;
	created: number;
}

function randomId(prefix: string): string {
	const bytes = crypto.getRandomValues(new Uint8Array(12));
	return prefix + Buffer.from(bytes).toString("hex");
}

function stripeError(status: number, message: string): Response {
	return Response.json(
		{ error: { type: "invalid_request_error", message } },
		{ status },
	);
}

function escapeHtml(s: string): string {
	return s
		.replaceAll("&", "&amp;")
		.replaceAll("<", "&lt;")
		.replaceAll(">", "&gt;")
		.replaceAll('"', "&quot;")
		.replaceAll("'", "&#39;");
}

function html(body: string, status = 200): Response {
	return new Response(
		`<!doctype html><html><head><meta charset="utf-8"><title>Fake Stripe Checkout</title>
<style>body{font-family:system-ui,sans-serif;max-width:28rem;margin:4rem auto;padding:0 1rem}
button{font-size:1rem;padding:.6rem 1.4rem;cursor:pointer}</style></head><body>${body}</body></html>`,
		{ status, headers: { "content-type": "text/html; charset=utf-8" } },
	);
}

function formatUsd(cents: number): string {
	return `$${(cents / 100).toFixed(2)}`;
}

export function createFakeStripe(opts: FakeStripeOptions) {
	if (!opts.webhookSecret) throw new Error("webhookSecret is required");
	const sessions = new Map<string, FakeSession>();
	const base = opts.publicUrl.replace(/\/+$/, "");

	async function createSession(req: Request): Promise<Response> {
		const auth = req.headers.get("authorization") ?? "";
		if (!/^Bearer \S+/.test(auth)) {
			return stripeError(401, "You did not provide an API key.");
		}
		const form = new URLSearchParams(await req.text());
		const amount = Number(form.get("line_items[0][price_data][unit_amount]"));
		if (!Number.isInteger(amount) || amount <= 0) {
			return stripeError(400, "Invalid line_items[0][price_data][unit_amount]");
		}
		const success = form.get("success_url");
		const cancel = form.get("cancel_url");
		if (!success || !cancel) {
			return stripeError(400, "Missing success_url or cancel_url");
		}
		const metadata: Record<string, string> = {};
		for (const [k, v] of form) {
			const m = k.match(/^metadata\[(.+)\]$/);
			if (m?.[1]) metadata[m[1]] = v;
		}
		const id = randomId("cs_test_");
		const session: FakeSession = {
			id,
			object: "checkout.session",
			mode: form.get("mode") ?? "payment",
			status: "open",
			payment_status: "unpaid",
			amount_total: amount,
			currency: (
				form.get("line_items[0][price_data][currency]") ?? "usd"
			).toLowerCase(),
			client_reference_id: form.get("client_reference_id"),
			metadata,
			success_url: success,
			cancel_url: cancel,
			url: `${base}/pay/${id}`,
			created: Math.floor(Date.now() / 1000),
		};
		sessions.set(id, session);
		return Response.json(session);
	}

	function payPage(s: FakeSession): Response {
		const account = s.metadata.account_id ?? s.client_reference_id ?? "";
		return html(`<h1>Fake Stripe Checkout</h1>
<p>Amount: <strong>${formatUsd(s.amount_total)}</strong> ${escapeHtml(s.currency.toUpperCase())}</p>
<p>Account: <code>${escapeHtml(account)}</code></p>
<form method="post" action="/pay/${escapeHtml(s.id)}"><button type="submit">Pay</button></form>
<p><a href="${escapeHtml(s.cancel_url)}">Cancel</a></p>`);
	}

	async function pay(s: FakeSession): Promise<Response> {
		const paid: FakeSession = {
			...s,
			status: "complete",
			payment_status: "paid",
		};
		const event = {
			id: randomId("evt_"),
			object: "event",
			type: "checkout.session.completed",
			created: Math.floor(Date.now() / 1000),
			livemode: false,
			data: { object: paid },
		};
		const payload = JSON.stringify(event);
		let status = 0;
		try {
			const res = await fetch(opts.webhookUrl, {
				method: "POST",
				headers: {
					"content-type": "application/json",
					"stripe-signature": signatureHeader(payload, opts.webhookSecret),
				},
				body: payload,
				signal: AbortSignal.timeout(10_000),
			});
			status = res.status;
		} catch {
			status = 0;
		}
		if (status < 200 || status >= 300) {
			const why = status === 0 ? "unreachable" : `HTTP ${status}`;
			console.warn(`fake-stripe: webhook delivery for ${s.id} failed (${why})`);
			return html(
				`<h1>Fake Stripe Checkout</h1><p>Webhook delivery failed (${why}). Is appctl running?</p>
<form method="post" action="/pay/${escapeHtml(s.id)}"><button type="submit">Retry</button></form>`,
				502,
			);
		}
		sessions.set(s.id, paid);
		const target = s.success_url.replaceAll("{CHECKOUT_SESSION_ID}", s.id);
		return new Response(null, { status: 303, headers: { location: target } });
	}

	async function fetchHandler(req: Request): Promise<Response> {
		const url = new URL(req.url);
		const p = url.pathname;
		if (p === "/healthz" && req.method === "GET") {
			return Response.json({ ok: true });
		}
		if (p === "/v1/checkout/sessions" && req.method === "POST") {
			return createSession(req);
		}
		const m = p.match(/^\/pay\/([A-Za-z0-9_]+)$/);
		if (m?.[1]) {
			const s = sessions.get(m[1]);
			if (!s)
				return html(
					"<h1>Fake Stripe Checkout</h1><p>Unknown session.</p>",
					404,
				);
			if (req.method === "GET") return payPage(s);
			if (req.method === "POST") return pay(s);
		}
		return stripeError(404, `Unrecognized request URL (${req.method}: ${p})`);
	}

	return { fetch: fetchHandler, sessions };
}
