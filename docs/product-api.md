# Product API contracts

How a signed-in user gets an account, keys, credit and usage. Three services, all
self-hostable and runnable locally:

```
browser --same origin--> web-api (:3000, Bun + Hono + better-auth)
                             | Bearer HUMANIP_CONTROL_TOKEN
                             v
                         appctl (:8787) /api/v1/control/*, /api/v1/billing/*
                             | SQLite ledger (HUMANIP_ACCOUNTS_DB)
                             v
                         humanip-gateway (proxy :1080/:8080, admin :9090)
Stripe (or fake-stripe :12111) --signed webhook--> appctl /api/v1/billing/webhook
```

Money crosses these APIs as USD numbers (`balance_usd`, `total_usd`) for the
browser, and as integer micro-USD (`*_micro`) between services.

## 1. Control API (appctl, service to service)

Base `/api/v1/control`. Every request needs `Authorization: Bearer
<HUMANIP_CONTROL_TOKEN>` (constant-time compare). If the env var is unset or
empty, every control route returns 503 `{"error":"control API not configured"}`.
Missing/wrong token: 401. Errors are `{"error": "..."}`. Store: the SQLite ledger
at `HUMANIP_ACCOUNTS_DB` (503 if unset).

| Method | Path | Body | 200 response |
|---|---|---|---|
| POST | `/accounts` | `{"name": str}` | `{"account_id": str}` |
| GET | `/accounts/{account_id}` | | `{"account_id","name","balance_micro"}` |
| GET | `/accounts/{account_id}/keys` | | `[{"key_id","name","kyc_tier","monthly_cap_micro":int\|null,"revoked":bool}]` |
| POST | `/accounts/{account_id}/keys` | `{"name": str, "monthly_cap_usd": num\|null}` | `{"key": KeyRecord, "secret": str}` (secret shown once) |
| POST | `/accounts/{account_id}/keys/{key_id}/rotate` | | `{"secret": str}` |
| POST | `/accounts/{account_id}/keys/{key_id}/revoke` | | `{"ok": true}` |
| PUT | `/accounts/{account_id}/keys/{key_id}/cap` | `{"monthly_cap_usd": num\|null}` | `{"ok": true}` |
| GET | `/accounts/{account_id}/usage?since=UNIX` | | `[UsageRow]` (`key_id, provider, domain, bytes, total_micro`) |

Rules: unknown account or a key that belongs to another account is 404 (never
leak another account's keys). Key names 1-64 chars. Caps 0 < usd <= 100000.

Billing (existing routes, now also token-guarded for checkout):

| Method | Path | Auth | Body | Response |
|---|---|---|---|---|
| POST | `/api/v1/billing/checkout` | control token | `{"account_id","amount_usd"}` | `{"id","url"}` |
| POST | `/api/v1/billing/webhook` | Stripe signature | raw event | `{"received":true,...}` |

## 2. Web API (web-api, browser facing)

better-auth mounted at `/api/auth/*` (email + password; sessions in an HTTP-only
cookie). App routes under `/app/api`, all require a session (401 otherwise) and
act only on the signed-in user's `account_id`, which web-api stored when the user
signed up (it calls `POST /control/accounts` in a user-create hook).

| Method | Path | Body | 200 response |
|---|---|---|---|
| GET | `/app/api/me` | | `{"user":{"id","email","name"},"account":{"account_id","balance_usd"}}` |
| GET | `/app/api/keys` | | `[{"key_id","name","revoked","monthly_cap_usd":num\|null}]` |
| POST | `/app/api/keys` | `{"name","monthly_cap_usd":num\|null}` | `{"key":{...as above},"secret"}` |
| POST | `/app/api/keys/{key_id}/rotate` | | `{"secret"}` |
| POST | `/app/api/keys/{key_id}/revoke` | | `{"ok":true}` |
| GET | `/app/api/usage?days=30` | | `{"rows":[{"key_id","provider","domain","bytes","total_usd"}],"total_usd"}` |
| POST | `/app/api/checkout` | `{"amount_usd"}` | `{"url"}` (redirect the browser there) |
| GET | `/app/api/config` | | `{"gateway_host","socks_port","http_port"}` (public, for connect snippets) |

Checkout success returns to `{PUBLIC_URL}/#/account?topup=success`.

## 3. Local ports

| Service | Port |
|---|---|
| Vite console | 1420 (proxies `/api/auth`, `/app` to 3000; `/api/v1` to 8787; `/admin` to 9090) |
| web-api | 3000 |
| appctl serve | 8787 |
| gateway SOCKS5 / HTTP / admin | 1080 / 8080 / 9090 |
| fake-stripe | 12111 |
