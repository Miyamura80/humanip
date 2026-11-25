# Self-hosting humanip

Everything runs on one machine with no external services except Stripe, and
for local testing Stripe is replaced by `tools/fake-stripe`. The API contracts
between the services are in [product-api.md](product-api.md).

## Architecture

```
                      browser
                         |
                         v
 +-----------------------------------------------------+
 | console origin (PUBLIC_URL)                         |
 |   local: Vite :1420     prod: reverse proxy :443    |
 +-----------------------------------------------------+
   | /api/auth/*, /app/*        | /api/v1/billing/webhook   | static console
   v                            |  (from Stripe only)       v
 +---------------------------+  |                     frontend/dist
 | web-api :3000             |  |
 | Bun + Hono + better-auth  |  |
 | SQLite: auth.db           |  |
 +---------------------------+  |
   | Bearer HUMANIP_CONTROL_TOKEN
   | /api/v1/control/*, /api/v1/billing/checkout
   v                            v
 +-----------------------------------------------------+
 | appctl serve :8787  (control API + billing)         |
 +-----------------------------------------------------+
   | SQLite ledger (HUMANIP_ACCOUNTS_DB, WAL)        ^
   v                                                 | POST /v1/checkout/sessions
 +--------------------------------------+   +---------------------------+
 | humanip-gateway                      |   | Stripe, or fake-stripe    |
 | SOCKS5 :1080  HTTP :8080  admin :9090|   | :12111 (local)            |
 | reads keys/balances from the ledger  |   | signed webhook -> appctl  |
 +--------------------------------------+   +---------------------------+
```

Two SQLite files hold all state:

| File | Owner | Contents |
|---|---|---|
| `auth.db` (`DATABASE_PATH`) | web-api | better-auth users, sessions, password hashes, and the `humanip_account` user-to-account table |
| ledger (`HUMANIP_ACCOUNTS_DB`, gateway `accounts_db`) | appctl + gateway | accounts, API keys (hashed), balances, usage, credited checkout sessions |

## Services and environment

### web-api (`apps/web-api`)

`bun run apps/web-api/src/index.ts`. Runs better-auth's migrations on every
start, so a fresh checkout needs no manual step.

| Variable | Default | Meaning |
|---|---|---|
| `HOST` | `127.0.0.1` | Bind address (`0.0.0.0` in a container) |
| `PORT` | `3000` | Listen port |
| `DATABASE_PATH` | `./data/auth.db` | better-auth SQLite file (`bun:sqlite`), created if missing |
| `BETTER_AUTH_SECRET` | required | At least 32 chars; signs session cookies. `openssl rand -hex 32` |
| `PUBLIC_URL` | `http://localhost:1420` | Console origin: better-auth `baseURL`, the only `trustedOrigins` entry, the only CORS origin, and the base of checkout return URLs |
| `CONTROL_URL` | `http://127.0.0.1:8787` | `appctl serve` base URL |
| `HUMANIP_CONTROL_TOKEN` | empty | Bearer token for the control API; must equal appctl's |
| `GATEWAY_HOST` | `localhost` | Shown in connect snippets (`/app/api/config`) |
| `SOCKS_PORT` | `1080` | Shown in connect snippets |
| `HTTP_PORT` | `8080` | Shown in connect snippets |
| `REQUIRE_EMAIL_VERIFICATION` | unset | `1` turns on email verification (see below) |

Security defaults: session cookies are HttpOnly and SameSite=Lax (Secure when
`PUBLIC_URL` is https); request bodies over 64 KiB get 413; mutating `/app/api`
calls from another origin get 403; better-auth's rate limiter is always on
(100 requests per minute per IP, 3 per 10 s on sign-in and sign-up). The
request log prints method, path and status only, never bodies, query strings,
cookies, tokens or key secrets. Key secrets pass through `POST /app/api/keys`
and `/rotate` once and are not stored by web-api.

The rate limiter keys on the client IP from `X-Forwarded-For`. In production
the reverse proxy must overwrite that header with the real client address.

**Signup and accounts.** After better-auth writes a new user, a hook calls
`POST /api/v1/control/accounts` with `name` set to the email and stores the
returned `account_id` in the `humanip_account` table. If the control API is
down the signup still succeeds and the account is created on the user's first
`/app/api/*` request (with retries); until then those routes answer 503. No
route ever runs without an account id, and the id always comes from that
table, never from the request.

**Email verification.** Off by default for local use. To turn it on, set
`REQUIRE_EMAIL_VERIFICATION=1` and pass a mail sender to `createApp` in
`apps/web-api/src/index.ts`:

```ts
const { app } = await createApp(env, {
	logRequests: true,
	sendVerificationEmail: async (to, url) => {
		await mailer.send({ to, subject: "Verify your email", text: url });
	},
});
```

web-api refuses to start with the flag set and no sender.

### appctl serve (`crates/cli`)

`target/debug/appctl serve --host 127.0.0.1 --port 8787`.

| Variable | Meaning |
|---|---|
| `HUMANIP_ACCOUNTS_DB` | Path of the SQLite ledger (shared with the gateway) |
| `HUMANIP_CONTROL_TOKEN` | Bearer token for `/api/v1/control/*` and checkout |
| `STRIPE_SECRET_KEY` | Stripe secret key (`sk_test_fake` with fake-stripe) |
| `STRIPE_WEBHOOK_SECRET` | Webhook signing secret (`whsec_...`) |
| `STRIPE_API_BASE` | `https://api.stripe.com`; `http://127.0.0.1:12111` for fake-stripe |
| `HUMANIP_PUBLIC_URL` | Must be the console origin (`http://localhost:1420` locally) |

`HUMANIP_PUBLIC_URL` builds the Stripe return URLs. appctl currently sends the
customer back to `{HUMANIP_PUBLIC_URL}/billing/success?session_id=...` (cancel:
`/billing/cancel`). web-api also sends `success_url`
(`{PUBLIC_URL}/#/account?topup=success`) and `cancel_url` in the checkout body,
which appctl ignores today; once appctl honours them the return path matches
the contract in product-api.md. Keep `HUMANIP_PUBLIC_URL` and web-api's
`PUBLIC_URL` equal.

### humanip-gateway (`crates/gateway`)

`target/debug/humanip-gateway --config gateway.yaml`. Set `accounts_db` to the
same ledger file as `HUMANIP_ACCOUNTS_DB` so keys and balances created through
the console work at once. See `examples/gateway.yaml` and
`deploy/gateway.prod.example.yaml`. `${VAR}` in the file is expanded from env.

### fake-stripe (`tools/fake-stripe`)

`bun run tools/fake-stripe/src/index.ts`. Local only; it accepts any API key.

| Variable | Default | Meaning |
|---|---|---|
| `PORT` | `12111` | Listen port |
| `STRIPE_WEBHOOK_SECRET` | required | Same value as appctl's |
| `WEBHOOK_URL` | `http://127.0.0.1:8787/api/v1/billing/webhook` | Where paid events go |
| `FAKE_STRIPE_URL` | `http://localhost:<PORT>` | Base of the checkout `url` given to browsers |

Routes: `POST /v1/checkout/sessions` (form-encoded, like Stripe), `GET
/pay/:id` (a page with Pay and Cancel), `POST /pay/:id` (signs and posts
`checkout.session.completed` with `Stripe-Signature: t=..,v1=..`, then 303 to
`success_url`), `GET /healthz`. If the webhook is not accepted, the pay page
shows the error and a Retry button instead of redirecting.

## Local stack

```bash
scripts/dev-stack.sh          # cargo build, then start all five services
scripts/dev-stack.sh status
scripts/dev-stack.sh stop
NO_BUILD=1 scripts/dev-stack.sh   # skip cargo build
```

It writes everything under `.stack/` (gitignored):

- `.stack/env`: generated once (`BETTER_AUTH_SECRET`, `HUMANIP_CONTROL_TOKEN`,
  `STRIPE_WEBHOOK_SECRET`, `GATEWAY_ADMIN_TOKEN`), reused on every start.
- `.stack/data/auth.db`, `.stack/data/ledger.db`: the two databases.
- `.stack/gateway.yaml`: a dev gateway config with `accounts_db` on the shared
  ledger, `allow_direct: true` and one `kind: direct` provider, so proxied
  traffic exits from your own machine with no real provider.
- `.stack/logs/<service>.log` and `.stack/pids`.

Then open http://localhost:1420, sign up, create a key, top up through the
fake checkout, and point a client at `socks5://localhost:1080` with the key as
the password. The script refuses to start if any of its ports
(12111, 8787, 1080, 8080, 9090, 3000, 1420) is taken.

Delete `.stack/` for a clean slate.

## Production

**Stripe.** Drop fake-stripe. Set `STRIPE_SECRET_KEY` to your live (or test)
key and unset `STRIPE_API_BASE`. In the Stripe dashboard add a webhook
endpoint `https://<your-host>/api/v1/billing/webhook` for
`checkout.session.completed` and `checkout.session.async_payment_succeeded`,
and put its signing secret in `STRIPE_WEBHOOK_SECRET`. See
[billing.md](billing.md).

**One reverse proxy** in front of everything, on the console origin:

| Path | Upstream |
|---|---|
| `/` | the built console (`bun run build`, serve `frontend/dist`) |
| `/api/auth/*` | web-api `:3000` |
| `/app/*` | web-api `:3000` |
| `/api/v1/billing/webhook` | appctl `:8787` |
| `/api/v1/control/*` | **not exposed**: internal only, web-api reaches appctl directly |
| `/api/v1/billing/checkout` | **not exposed**: only web-api calls it |

A Caddy sketch:

```
humanip.example {
	handle /api/auth/* { reverse_proxy 127.0.0.1:3000 }
	handle /app/*      { reverse_proxy 127.0.0.1:3000 }
	handle /api/v1/billing/webhook { reverse_proxy 127.0.0.1:8787 }
	handle /api/v1/*   { respond 404 }
	handle {
		root * /srv/humanip/frontend/dist
		file_server
	}
}
```

Caddy sets `X-Forwarded-For` from the client connection, which the rate
limiter relies on. Bind web-api and appctl to `127.0.0.1` (or a private
network), set `PUBLIC_URL` and `HUMANIP_PUBLIC_URL` to `https://humanip.example`,
and use long random values for `BETTER_AUTH_SECRET` and
`HUMANIP_CONTROL_TOKEN`. The gateway's proxy ports are public; its admin port
stays private.

**Backups.** Back up both SQLite files. Both run in WAL mode, so copy them with
SQLite's online backup rather than `cp`:

```bash
sqlite3 /var/lib/humanip/auth.db   ".backup '/backup/auth-$(date +%F).db'"
sqlite3 /var/lib/humanip/ledger.db ".backup '/backup/ledger-$(date +%F).db'"
```

The ledger holds money (balances and credited sessions); back it up at least
daily and before upgrades. Restoring `auth.db` without the matching ledger
leaves users linked to account ids the ledger does not know, so restore the two
from the same point in time.
