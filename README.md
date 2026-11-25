# humanip

**A proxy router that sends each client out through a non-datacenter exit
(residential, ISP or mobile/eSIM), with a per-route policy.**

- **Sticky** routes keep one IP for a session (browser and computer-use agents)
  or for months (mautrix bridges), and fail closed if that IP dies.
- **Rotating** routes take a fresh exit per request.
- Every route picks the cheapest exit that clears a **reliability floor**
  (95% by default). A 0-100 **risk dial** trades IP reputation, exit class,
  provider sourcing and sticky guarantees for cost.

Clients connect over plain **SOCKS5** or **HTTP CONNECT**, so no SDK is needed.

> Status: early bootstrap. The routing rules (`crates/humanip-core`) and the
> `quote` / route-config commands work today. The proxy data plane and
> provider adapters are next, see [Roadmap](#roadmap).

## How it works

```
  browser agent        mautrix bridge         scraper
  (sticky/session)     (sticky/forever)       (rotating)
        |                     |                    |
        +---------- SOCKS5 :1080 / HTTP :8080 -----+
                              |
               user-route_X-session_Y-risk_N:KEY      <- settings ride in
                              |                          the proxy username
   +--------------------------v---------------------------+
   | humanip gateway (Rust, open source)                  |
   |                                                      |
   |  route policy --> risk limits --> eligible offers     |
   |                                       |              |
   |               cost/reliability frontier (Pareto)     |
   |                                       |              |
   |   default pick = cheapest on frontier >= floor        |
   |   sticky sessions pick once, never switch mid-session |
   |                                                      |
   |  passive health (403/CAPTCHA/TLS reset) --+          |
   |  IP reputation (IPQS / Scamalytics) ------+--> live  |
   +-----------+-------------+-------------+-----+  DB    |
               |             |             |     (closed) |
          residential      ISP static   mobile/eSIM   generic
          providers        providers    providers     SOCKS/HTTP
```

### Why a frontier

Two numbers matter per target site: what a *successful* GB costs (price
including the 5% fee, divided by success rate) and how often it works. An exit
that is both pricier and less reliable than another is never picked. Of the
rest, humanip takes the cheapest one that works at least 95% of the time on
that site. If none clears the floor it takes the most reliable one and says so.

For the WhatsApp bridge in the sample data, Rayobyte is cheapest but works 93%
of the time, so NetNut (96%) is the default pick.

## Quick start

```bash
cargo test --workspace
cargo build -p appctl
```

Quote exits for a site:

```bash
target/debug/appctl call quote --args '{"url": "https://web.whatsapp.com", "exit_types": ["mobile", "isp"]}' --json
```

Parse a proxy username:

```bash
target/debug/appctl call parse_proxy_user --args '{"username": "user-route_agent-browser-session_r42-risk_40"}' --json
```

Validate a routes file:

```bash
target/debug/appctl call validate_routes --args "{\"yaml\": $(jq -Rs . < examples/routes.yaml)}" --json
```

Run the console (mock data):

```bash
bun install && make dev
```

## Routes config

Routes live in a YAML file. The full schema is exported by
`appctl call route_schema --json`. See [`examples/routes.yaml`](examples/routes.yaml).

```yaml
routes:
  - name: mautrix-whatsapp
    mode: sticky
    sticky: { ttl: forever, on_pin_loss: fail_closed }
    exit_types: [mobile, isp]
    geo: { country: DE, city: Berlin, asn: 3320, match_account: true }
    risk: 5
    reliability_floor: 0.95
    spend_cap_usd: 25
    alert_webhook: https://ops.example.com/hooks/humanip
```

| Field | Values | Default |
|---|---|---|
| `mode` | `sticky`, `rotating` | `sticky` |
| `sticky.ttl` | `session` (30 min idle), `{hours: N}`, `{days: N}`, `forever` | `session` |
| `sticky.on_pin_loss` | `fail_closed`, `same_asn`, `same_city`, `same_country`, `any` | `fail_closed` |
| `exit_types` | `mobile`, `isp`, `residential` (never datacenter) | `[residential]` |
| `risk` | 0-100 | 20 |
| `reliability_floor` | 0.5-1.0 | 0.95 |
| `overrides` | `max_fraud_score`, `allowed_exit_types`, `sourcing_tiers`, `min_sticky_minutes` | none |

### What the risk dial controls

| Risk | Max fraud score | Exit classes | Provider sourcing | Sticky guarantee |
|---|---|---|---|---|
| 0-29 | 15-32 | preferred only | verified | 1 day |
| 30-59 | 33-50 | + one step cheaper | verified (+ standard from 40) | 30 min |
| 60-100 | 51-75 | down to residential | + unverified from 75 | best effort from 70 |

A sticky route's own pin length always applies on top of this.

## Proxy username

Any SOCKS5/HTTP client can pass settings in the username. The password is your
API key.

```
user-route_agent-browser-session_r42-risk_40-country_us-exit_mobile-target_linkedin.com
```

| Key | Meaning |
|---|---|
| `route_` | inherit a saved route |
| `session_` | sticky key: same value, same exit. Omit for rotating |
| `risk_` | 0-100, overrides the route |
| `country_` `city_` `asn_` | geo constraints |
| `exit_` | `mobile`, `isp` or `residential` |
| `target_` | hint for the cheapest-exit lookup (otherwise read from CONNECT/SNI) |

Values may contain dashes: a token that does not start with a known key
continues the previous value.

## Repository layout

| Path | What |
|---|---|
| `crates/humanip-core` | Routing rules, no I/O: policy schema, risk resolution, Pareto selection, username parsing |
| `crates/engine` | Typed commands (`quote`, `validate_routes`, `route_schema`, `parse_proxy_user`, ...) shared by CLI and HTTP |
| `crates/cli` | `appctl` binary: CLI + HTTP API |
| `frontend/` | Console mockups (React/Vite) and design-system exploration at `#/systems` |
| `examples/routes.yaml` | Example routes |

## Roadmap

1. **Data plane**: SOCKS5 and HTTP CONNECT listeners, username auth, upstream
   dialing, session table for sticky pins.
2. **Provider adapters**: residential majors, ISP/static, mobile/eSIM, generic
   upstream list, plus a mock adapter for tests.
3. **Health**: passive scoring from block pages, CAPTCHAs, 403s and TLS resets;
   pin-loss detection and webhooks.
4. **Reputation**: IPQS / Scamalytics checks with caching.
5. **Billing**: prepaid credits (Stripe, USDC), per-key spend caps, domain
   blocklist with KYC tiers.

## Business model

The router is open source. Hosted humanip holds the provider accounts and bills
provider cost plus a 5% fee. The live price, success-rate and reputation
database that powers `quote` in production is a separate, closed service; this
repo ships an illustrative sample catalog.

## License

MIT
