# Billing: prepaid top-ups with Stripe Checkout (card and USDC)

humanip is prepaid. Customers top up a balance by card or USDC stablecoin
through Stripe Checkout, and the gateway debits usage at provider cost plus
5%. Money is stored as integer micro-USD (1 USD = 1,000,000 micro).

Code: `crates/billing` (package `humanip-billing`) holds the Stripe client,
webhook signature verification and event handling. `crates/cli/src/billing_routes.rs`
mounts the HTTP routes in `appctl serve`.

## Flow

```
 customer            appctl serve                 Stripe              AccountStore
    |  POST /billing/checkout  |                       |                      |
    |------------------------->|  POST /v1/checkout/   |                      |
    |                          |  sessions (form)      |                      |
    |                          |---------------------->|                      |
    |     {id, url}            |<----------------------|                      |
    |<-------------------------|                       |                      |
    |  redirect to url, pay by card or USDC            |                      |
    |------------------------------------------------->|                      |
    |                          |  POST /billing/webhook|                      |
    |                          |  Stripe-Signature     |                      |
    |                          |<----------------------|                      |
    |                          |  verify HMAC + time   |                      |
    |                          |  credit(account, amount_total * 10_000,      |
    |                          |         external_ref = session id)           |
    |                          |--------------------------------------------->|
    |                          |  200                  |                      |
    |                          |---------------------->|                      |
```

1. `POST /api/v1/billing/checkout` with `{"account_id": "acct_1", "amount_usd": 25}`
   returns `{"id": "cs_...", "url": "https://checkout.stripe.com/..."}`.
   Amounts must be between $5 and $5,000 and are rounded to the cent.
2. The client redirects the customer to `url`.
3. Stripe posts events to `POST /api/v1/billing/webhook`. After the signature
   check, a paid session credits `amount_total` cents (times 10,000 micro) to
   the account in `metadata.account_id` (falling back to `client_reference_id`).

### Checkout Session fields

The request to `POST {STRIPE_API_BASE}/v1/checkout/sessions` is form-encoded
with exactly these fields, in this order:

| Field | Value |
|---|---|
| `mode` | `payment` |
| `line_items[0][quantity]` | `1` |
| `line_items[0][price_data][currency]` | `usd` |
| `line_items[0][price_data][product_data][name]` | `humanip credit` |
| `line_items[0][price_data][unit_amount]` | amount in cents |
| `client_reference_id` | account id |
| `metadata[account_id]` | account id |
| `payment_method_types[]` | `card` |
| `payment_method_types[]` | `crypto` (USDC stablecoin) |
| `success_url` | `{HUMANIP_PUBLIC_URL}/billing/success?session_id={CHECKOUT_SESSION_ID}` |
| `cancel_url` | `{HUMANIP_PUBLIC_URL}/billing/cancel` |

The account id must match `[A-Za-z0-9_-]{1,128}`.

### Webhook events

| Event | Handling |
|---|---|
| `checkout.session.completed` | Credit if `payment_status == "paid"`; otherwise ignore |
| `checkout.session.async_payment_succeeded` | Credit (crypto can settle after the session completes) |
| anything else | 200, ignored |

Non-USD sessions and sessions with no account id (for example ones created
by another product on the same Stripe account) are acknowledged and ignored. Responses: 200 for applied, duplicate or
ignored; 400 for a bad signature or a malformed event; 500 for a store error
(Stripe retries, which is safe because crediting is idempotent); 503 when
billing is not configured.

## Stripe dashboard setup

1. **Webhook endpoint.** Developers, Webhooks, Add endpoint:
   `https://<your host>/api/v1/billing/webhook`. Select the events
   `checkout.session.completed` and `checkout.session.async_payment_succeeded`
   (optionally `checkout.session.async_payment_failed`, which is acknowledged
   and ignored). Copy the signing secret (`whsec_...`) into
   `STRIPE_WEBHOOK_SECRET`.
2. **Enable stablecoins.** Settings, Payments, Payment methods: turn on
   **Stablecoins and crypto** (payment method type `crypto`). Without it,
   Stripe rejects the Checkout Session request with an "invalid payment method
   type" error, and `/billing/checkout` returns 502. Stablecoin acceptance is
   subject to Stripe's eligibility rules (at launch, US-based businesses with
   USD settlement). Customers pay in USDC; the merchant settles in USD, so
   `amount_total` is still USD cents.
3. **API key.** Use a restricted key with write access to Checkout Sessions
   where possible, in `STRIPE_SECRET_KEY`.

Note: `docs.stripe.com` was not reachable from the build environment when this
was written, so the `crypto` payment method type follows the commonly
documented form for Stripe's stablecoin payments. Check the current Stripe
docs ("Stablecoin payments" and "Pay with Crypto") before going live.

## Environment variables

| Variable | Required for | Notes |
|---|---|---|
| `STRIPE_SECRET_KEY` | checkout | `sk_live_...` or `sk_test_...` |
| `HUMANIP_PUBLIC_URL` | checkout | Base for success and cancel URLs, e.g. `https://humanip.example` |
| `STRIPE_WEBHOOK_SECRET` | webhook | `whsec_...` from the endpoint (or from `stripe listen`) |
| `STRIPE_API_BASE` | optional | Defaults to `https://api.stripe.com`; point at a mock in tests |

If the variables a route needs are missing or empty, it returns
`503 {"error": {"code": "not_configured", "message": "billing not configured"}}`.

## Idempotency

Stripe delivers webhooks at least once, and a crypto payment can produce both
`checkout.session.completed` (paid) and `checkout.session.async_payment_succeeded`
for the same session. Every credit calls
`AccountStore::credit(account_id, amount_micro, external_ref)` with the
Checkout Session id (`cs_...`) as `external_ref`. The store applies a given
`external_ref` once and returns `false` afterwards, which the webhook reports
as `{"result": {"outcome": "duplicate"}}` with status 200 so Stripe stops
retrying.

Signatures are HMAC-SHA256 over `"{t}.{raw body}"`, compared in constant time
against every `v1` value in `Stripe-Signature` (several are present while a
secret is being rolled). Timestamps more than 300 seconds from now are
rejected to block replays. The route verifies the raw bytes before parsing
JSON, so do not put a body-rewriting proxy in front of it.

## Store integration point

`appctl serve` opens the SQLite ledger (`humanip-ledger`) at
`HUMANIP_ACCOUNTS_DB` once at startup and shares it: the control API
(`/api/v1/control`) uses it directly, and `billing_store()` in
`crates/cli/src/billing_routes.rs` hands the same `Arc` to billing as
`dyn AccountStore`. Without `HUMANIP_ACCOUNTS_DB`, billing falls back to an
in-memory `MemoryAccounts` (credits are lost on restart) and logs a warning.

The checkout route requires `Authorization: Bearer <HUMANIP_CONTROL_TOKEN>`
(401 without it, 503 when the token is unset) and returns 404 when
`account_id` does not exist. The webhook is authenticated only by its Stripe
signature. See `docs/product-api.md` section 1.

## Testing

Automated tests never call Stripe. They use a loopback mock server and
locally signed payloads:

```bash
cargo test -p humanip-billing -p appctl
```

### Manual end-to-end with the Stripe CLI (test mode)

Use a **test-mode** key (`sk_test_...`) for this.

```bash
stripe login
stripe listen --forward-to localhost:8080/api/v1/billing/webhook \
  --events checkout.session.completed,checkout.session.async_payment_succeeded
# copy the printed whsec_... into STRIPE_WEBHOOK_SECRET

export STRIPE_SECRET_KEY=sk_test_...
export STRIPE_WEBHOOK_SECRET=whsec_...
export HUMANIP_PUBLIC_URL=http://localhost:5173
make run

curl -s localhost:8080/api/v1/billing/checkout \
  -H 'content-type: application/json' \
  -d '{"account_id":"acct_1","amount_usd":10}'
# open the returned url and pay with card 4242 4242 4242 4242

stripe trigger checkout.session.completed   # synthetic event (no humanip metadata)
```

`stripe trigger` events carry no humanip `metadata.account_id`, so the
webhook verifies the signature and answers 200 with outcome `ignored`; use a
real test-mode Checkout payment to exercise crediting.
