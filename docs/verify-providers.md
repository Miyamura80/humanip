# Verifying provider adapters with `humanip-verify`

The named adapters in `crates/gateway/src/providers/` (Bright Data, Oxylabs,
Decodo, IPRoyal, NetNut, Rayobyte, Proxy-Cheap) build each vendor's host, port
and username syntax for sticky sessions and country targeting. Those formats
were taken from search summaries because the vendor docs could not be read
directly, so the gateway prints an "unverified adapter" warning at startup.

`humanip-verify` checks an adapter against your own account before you send
real traffic through it. It uses the same `render(session, sticky, country)`
and the same dialer (`upstream::connect`) as the gateway, so a pass means the
gateway will produce working usernames for that account.

```text
gateway.yaml --> ProviderConfig.upstream.render(session, sticky, country)
                        |
                        v
          upstream::connect (SOCKS5 or HTTP CONNECT to the vendor)
                        |
          raw tunnel ---+--- GET /?format=json        -> api.ipify.org:80
                        +--- GET /json/<ip>?fields=... -> ip-api.com:80
```

## Running it

```bash
cargo build -p humanip-gateway --bin humanip-verify
target/debug/humanip-verify --config gateway.yaml --provider decodo-resi --country DE
```

Flags:

| Flag | Meaning |
| --- | --- |
| `--config FILE` | Gateway config (same file `humanip-gateway` reads). Required. |
| `--provider NAME` | Provider to check. Repeat it to check several. |
| `--all` | Check every provider in the config instead. |
| `--country XX` | Ask for this country on every connection and require every exit to geolocate there. |
| `--timeout-ms N` | Per tunnel open and per response. Default 15000. |
| `--json` | Print `{"ok": bool, "reports": [...]}` instead of the table. |
| `--ip-url URL` | IP-echo endpoint. Default `http://api.ipify.org/?format=json`. Must answer `{"ip": "..."}`. |
| `--geo-url TEMPLATE` | Geo endpoint with `{ip}` and optional `{fields}`. Default `http://ip-api.com/json/{ip}?fields={fields}`. |

Exit codes: `0` when every check passes or only warns, `1` when any check
fails, `2` for usage or config errors (unknown flag, unknown provider,
unreadable or invalid config, bad URL).

Passwords are never printed. Reports carry only the vendor endpoint and the
check details; the config types already redact secrets in `Debug`.

Sample output:

```text
provider decodo-resi (decodo adapter) via socks5 gate.decodo.com:7000
  CHECK            RESULT     TIME  DETAIL
  reachable        PASS      412ms  tunnel to api.ipify.org:80 open in 401ms
  exit_ip          PASS      655ms  exit 203.0.113.101
  sticky           PASS     1210ms  3 connections on one session exited via 203.0.113.101
  sessions_differ  PASS      610ms  second session exited via 198.51.100.44
  rotating         PASS     1802ms  3 rotating connections used 3 exits
  country          PASS     2400ms  5 of 5 exits geolocate to DE
  not_datacenter   PASS     2400ms  5 exits, none flagged hosting (0 mobile, 1 flagged proxy)
  result: PASS (0 warnings)
  the decodo adapter works with this account: the startup "unverified adapter" warning can be ignored for decodo-resi
```

## Per vendor

Put the provider in a gateway config with only its credentials; the adapter
supplies host, port and username syntax. A minimal file:

```yaml
api_keys: [{ name: me, key: local-only }]
providers:
  - name: decodo-resi
    exit_type: residential
    sourcing: verified
    price_per_gb: 3.5
    fraud_score: 10
    max_sticky_minutes: 30
    latency_ms: 400
    upstream: { kind: decodo, username: sp123, password: xyz, protocol: socks5 }
```

| Vendor | `upstream:` | Notes |
| --- | --- | --- |
| Bright Data | `{ kind: brightdata, customer: hl_abc123, zone: residential1, password: xyz }` | Zone must be a residential or ISP zone with targeting enabled. |
| Oxylabs | `{ kind: oxylabs, username: myuser, password: xyz }` | Default `pr.oxylabs.io:7777`. |
| Decodo | `{ kind: decodo, username: sp123, password: xyz, protocol: socks5 }` | Default `gate.decodo.com:7000`. Try both `protocol: socks5` and `http`. |
| IPRoyal | `{ kind: iproyal, username: myuser, password: xyz }` | |
| NetNut | `{ kind: netnut, username: myuser, password: xyz, endpoint: "gw-am.netnut.net:5959" }` | Use the regional endpoint from your dashboard. |
| Rayobyte | `{ kind: rayobyte, username: myuser, password: xyz }` | |
| Proxy-Cheap | `{ kind: proxy_cheap, username: myuser, password: xyz, endpoint: "host.from.dashboard:12345" }` | The endpoint is per order; copy it from the dashboard. |

Run once without `--country` and once with a country your plan covers, for
example `--country US` and `--country DE`. If a vendor supports both SOCKS5
and HTTP, verify each `protocol` you plan to use: the username syntax is the
same but the vendor gateways are separate code paths.

## What each check proves

| Check | Kind | Proves |
| --- | --- | --- |
| `reachable` | hard | The vendor gateway accepted the rendered username and password and opened a tunnel within the timeout. A fail here usually means wrong host, port, protocol or credentials, or a username the vendor does not parse. |
| `exit_ip` | hard | Bytes flow through the tunnel: a plain HTTP/1.1 GET to the IP-echo service returned `{"ip": ...}`. |
| `sticky` | hard | Three connections with the same session id and `sticky=true` exit via the same IP. A fail means the session parameter is ignored or mis-spelled, which would break every sticky route. |
| `sessions_differ` | soft | A second session id gets a different exit. A warning means a small pool, or that the vendor ignores the session value and pins on something else. |
| `rotating` | soft | Three connections with `sticky=false` (no session parameter) use more than one exit. A warning means the pool is small or the account is configured for static IPs. |
| `country` | hard, with `--country` | Every exit IP seen in the run geolocates to the requested country. The lookup is sent through the provider too, so it also leaves via the exit. Skipped without `--country`. |
| `not_datacenter` | soft | No exit is flagged `hosting=true` by ip-api. A warning means at least one exit looks like a datacenter range, which most anti-bot systems treat like a VPS. |

Transport errors (refused auth, timeouts, non-2xx replies) fail whichever
check hits them, soft or not. When `reachable` or `exit_ip` fails, the later
checks are reported as `SKIP`.

A run opens about 8 tunnels for the IP echo plus one geo lookup per distinct
exit IP (typically 4 to 8), so expect a few kilobytes of billed traffic.

## Limits

- **Soft checks depend on pool size.** A provider with a small pool, a static
  ISP plan, or a narrow country filter can legitimately return the same IP to
  two sessions or to three rotating connections. Re-run, or widen the
  country, before concluding the adapter is wrong.
- **Sticky is sampled, not proven.** Three connections over a few seconds
  cannot show that a session holds for its full TTL.
- **ip-api.com is a free service.** It allows roughly 45 requests per minute
  per source IP, which here is the exit IP, and serves plain HTTP only. Its
  geolocation and `hosting`/`proxy`/`mobile` flags are a single database's
  opinion. Use `--geo-url` to point at another service that returns the same
  field names (`countryCode`, `hosting`, `proxy`, `mobile`, and `status`/
  `message` on failure).
- **Plain HTTP only.** The checks speak HTTP over the raw tunnel to avoid a TLS
  stack, so `--ip-url` and `--geo-url` must be `http://`. A vendor that blocks
  port 80 will fail `exit_ip`.
- **Results are per account and per plan.** A pass on your account does not
  prove another customer's zone or plan uses the same syntax.

## After a pass

A passing run (warnings allowed) shows the adapter renders a working host,
port and username for your account, so the startup "unverified adapter"
warning no longer applies to that provider. Today the warning still prints;
note the run (for example by keeping the `--json` output) and ignore it for
that provider.

Follow-up, not yet implemented: a per-provider config flag such as
`verified: true` in `gateway.yaml` that silences the startup warning for
providers you have verified. It belongs in `config.rs` and the server's
startup check.
