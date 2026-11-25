/**
 * Stripe webhook signing, matching `crates/billing/src/webhook.rs`:
 * `Stripe-Signature: t=<unix>,v1=hex(HMAC_SHA256(secret, "{t}.{payload}"))`.
 */
import { createHmac } from "node:crypto";

export function signPayload(
	payload: string,
	secret: string,
	t: number,
): string {
	return createHmac("sha256", secret).update(`${t}.${payload}`).digest("hex");
}

export function signatureHeader(
	payload: string,
	secret: string,
	t: number = Math.floor(Date.now() / 1000),
): string {
	return `t=${t},v1=${signPayload(payload, secret, t)}`;
}
