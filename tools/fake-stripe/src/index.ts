/**
 * `bun run tools/fake-stripe/src/index.ts`
 *
 * Env: `PORT` (12111), `STRIPE_WEBHOOK_SECRET` (required), `WEBHOOK_URL`
 * (`http://127.0.0.1:8787/api/v1/billing/webhook`), `FAKE_STRIPE_URL`
 * (`http://localhost:<PORT>`, the base of checkout URLs shown to the browser).
 */
import { createFakeStripe } from "./server";

const port = Number(process.env.PORT ?? 12111);
const webhookSecret = process.env.STRIPE_WEBHOOK_SECRET ?? "";
if (!webhookSecret) {
	console.error("fake-stripe: STRIPE_WEBHOOK_SECRET must be set");
	process.exit(1);
}
const webhookUrl =
	process.env.WEBHOOK_URL ?? "http://127.0.0.1:8787/api/v1/billing/webhook";
const publicUrl = process.env.FAKE_STRIPE_URL ?? `http://localhost:${port}`;

const stripe = createFakeStripe({ webhookSecret, webhookUrl, publicUrl });
const server = Bun.serve({
	hostname: process.env.HOST ?? "127.0.0.1",
	port,
	fetch: stripe.fetch,
});
console.log(
	`fake-stripe listening on http://127.0.0.1:${server.port}, webhooks to ${webhookUrl}`,
);
