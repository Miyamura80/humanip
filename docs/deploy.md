# Deploying the humanip gateway

The gateway is one binary, `humanip-gateway`, with three listeners. This page
covers ports, TLS, configuration, secrets, scaling and monitoring. Deploy
files live in [`deploy/`](../deploy/).

```text
                 raw TCP (no TLS, no HTTP LB)
 proxy client ---------------------------------> :1080 SOCKS5      --+
 proxy client ---------------------------------> :8080 HTTP proxy  --+--> Gateway --> providers
                                                                     |
 operator ---- HTTPS ----> TLS proxy / edge ---> :9090 admin HTTP  --+
 Prometheus -- private network ----------------> :9090 /metrics
```

## Ports

| Port | Listener (config key) | Protocol | Put in front of it |
| ---- | --------------------- | -------- | ------------------ |
| 1080 | `socks_listen` | SOCKS5 over raw TCP | A TCP (layer 4) passthrough only |
| 8080 | `http_listen` | HTTP proxy protocol (CONNECT and absolute-form forwarding) over raw TCP | A TCP (layer 4) passthrough only |
| 9090 | `admin_listen` | Plain HTTP/1.1: `/healthz`, `/metrics`, `/admin/v1/*` | A TLS-terminating proxy, or nothing on a private network |

The proxy ports must not sit behind an HTTP load balancer or a TLS
terminator. Proxy clients speak plain SOCKS5 and plain HTTP proxy protocol.
An HTTP-aware balancer breaks `CONNECT`, rewrites or drops
`Proxy-Authorization`, and expects origin-form requests. Use a layer 4 service:
Fly `[[services]]` with no handlers, a Railway TCP proxy, an AWS NLB, or a
Kubernetes `Service` of type `LoadBalancer` without an HTTP ingress.

Proxy credentials (the API key is the proxy password) travel in clear text on
1080 and 8080, as with every commercial proxy network. Clients that need
confidentiality wrap the tunnel themselves: HTTPS through `CONNECT`, or SOCKS
inside a VPN or SSH tunnel.

## Admin API

| Route | Auth | Response |
| ----- | ---- | -------- |
| `GET /healthz` | none | `200 ok` |
| `GET /metrics` | none | Prometheus text, `text/plain; version=0.0.4` |
| `GET /admin/v1/status` | `Authorization: Bearer <admin_token>` | JSON: version, uptime, sessions, providers, routes, health |
| `GET /admin/v1/usage?since=UNIX[&account=ID]` | Bearer token | JSON array of usage rows; `503` with no account store, `400` without `since` |
| anything else | | `404` |

A missing or wrong token returns `401`. The token is compared in constant
time. The admin listener is off unless `admin_listen` is set, and then
`admin_token` is required.

### TLS for the admin API

The admin listener speaks plain HTTP. Terminate TLS in front of it:

- **Fly.io**: `deploy/fly.toml` maps public 443 (`tls` + `http` handlers) to
  9090, so Fly holds the certificate.
- **Railway**: give the service a public domain targeting port 9090; Railway
  serves it over HTTPS.
- **Compose or a VM**: the compose file binds 9090 to `127.0.0.1` only. Put
  Caddy, nginx or a cloud load balancer with a certificate in front, or reach
  it over SSH, WireGuard or Tailscale.

Never expose 9090 on a public interface without TLS: the bearer token would
cross the network in clear text.

### Keep `/metrics` private

`/metrics` needs no token, so Prometheus can scrape it without secrets. It
reveals provider names, traffic volume and failure rates, so do not publish
it. Scrape over a private network (Fly `.internal` addresses, a Docker
network, a VPC). When the admin port is public behind a TLS proxy, block
`/metrics` at that proxy, for example in Caddy:

```text
admin.example.com {
    @metrics path /metrics
    respond @metrics 404
    reverse_proxy 127.0.0.1:9090
}
```

## Configuration and environment variables

Start the binary with `--config <path>`, or set `HUMANIP_CONFIG`. Run
`humanip-gateway --config <path> --check` in CI or before a deploy: it
validates the file and exits `0` (valid) or `1` (invalid).

Every `${NAME}` in the YAML is replaced from the environment before parsing,
and `${NAME:-default}` falls back to `default` when `NAME` is unset or empty.
An unset `${NAME}` without a default stops startup, so a missing secret
never turns into an empty password. Lines that start with `#` are not
expanded. Template placeholders without `$`, such as `{session}` and
`{country}`, are left for the provider adapters.

| Variable | Default | Meaning |
| -------- | ------- | ------- |
| `HUMANIP_CONFIG` | none | Config path when `--config` is not given. The image sets `/etc/humanip/gateway.yaml`. |
| `HUMANIP_LOG_FORMAT` | human-readable | `json` for one JSON object per line (use in production). |
| `RUST_LOG` | `info` | Log filter, for example `info,humanip_gateway=debug`. |
| `HUMANIP_SHUTDOWN_GRACE_SECS` | `10` | How long to wait for open proxy connections after SIGTERM. |
| `HUMANIP_ADMIN_TOKEN` | none | Referenced by `deploy/gateway.prod.example.yaml` as `admin_token`. |
| `HUMANIP_API_KEY_DEFAULT` | none | The example's client API key. |
| `NETNUT_PASSWORD`, `IPROXY_PASSWORD` | none | The example's provider credentials. |
| `HUMANIP_SOCKS_LISTEN`, `HUMANIP_HTTP_LISTEN`, `HUMANIP_ADMIN_LISTEN` | `0.0.0.0:1080`, `:8080`, `:9090` | Listener overrides in the example. |

Only the first four are read by the binary itself. The rest are names the
example config chooses; rename them freely.

## Secrets

- Keep secrets out of the YAML and the image. Reference them as `${NAME}` and
  inject them with the platform's secret store: `fly secrets set`, Railway
  variables, Docker or Kubernetes secrets exposed as environment variables.
- Secrets in scope: `admin_token`, every client API key, every provider
  password.
- Generate the admin token with at least 32 random bytes, for example
  `openssl rand -hex 32`. Rotate it by changing the secret and restarting.
- The gateway redacts API keys and provider passwords in its `Debug` output.
- The image runs as the unprivileged user `humanip` (uid 10001) and writes
  only to `/data`.

## Scaling

The sticky pin table, which maps a session to its chosen exit, lives in
process memory. Two instances behind a round-robin balancer would pin the
same session to different exits, and a sticky session would change IP
whenever its connections land on another instance. Passive health data is
per process too.

Until the pin table is shared:

- Run one instance. The deploy files set one replica (`numReplicas: 1`,
  `fly scale count 1`).
- To run more, use a balancer with client-IP affinity on the TCP ports, and
  accept that a pin is lost when its instance restarts or a client changes
  IP.
- Scale vertically first. The data plane is a byte splice with one task per
  tunnel; raise the file descriptor limit (`ulimit -n`) above the expected
  number of concurrent tunnels times two.

A restart also drops all pins. Routes with `on_pin_loss: fail_closed` then
refuse connections until the client starts a new session.

## Shutdown

On SIGTERM or Ctrl-C the gateway stops accepting connections, waits up to
`HUMANIP_SHUTDOWN_GRACE_SECS` for open proxy connections to finish, and
exits `0`. Give the platform a slightly longer stop timeout: the deploy files
use 15 seconds.

## Prometheus

Scrape `http://<host>:9090/metrics`:

```yaml
scrape_configs:
  - job_name: humanip-gateway
    scrape_interval: 15s
    static_configs:
      - targets: ["humanip-gateway.internal:9090"]
```

| Metric | Type | Labels |
| ------ | ---- | ------ |
| `humanip_connections_total` | counter | `proto` (`socks5`, `http_connect`, `http_forward`), `result` (`ok`, `auth_failed`, `denied`, `blocked`, `dial_failed`, `protocol_error`) |
| `humanip_connections_active` | gauge | `proto` |
| `humanip_bytes_total` | counter | `provider`, `direction` (`up` is client to target, `down` is target to client) |
| `humanip_dial_failures_total` | counter | `provider` |

Useful queries:

```text
sum by (result) (rate(humanip_connections_total[5m]))
sum by (provider) (rate(humanip_bytes_total[5m])) * 8   # bits per second
rate(humanip_dial_failures_total[5m])
```

On Fly, the `[metrics]` section in `deploy/fly.toml` makes Fly's managed
Prometheus scrape the same endpoint.

## Platforms

### Docker Compose

```bash
export HUMANIP_ADMIN_TOKEN=$(openssl rand -hex 32) HUMANIP_API_KEY_DEFAULT=... \
       NETNUT_PASSWORD=... IPROXY_PASSWORD=...
docker compose -f deploy/docker-compose.yml up --build
```

Edit `deploy/gateway.prod.example.yaml` (mounted read-only) or mount your own
file. The `gateway-data` volume at `/data` is reserved for `accounts_db`.

### Fly.io

See the header of `deploy/fly.toml` for the full command sequence. Allocate a
dedicated IPv4 address: shared IPv4 on Fly only routes HTTP and TLS by
hostname, so raw TCP ports need their own address.

### Railway

`deploy/railway.json` builds the image and health-checks `/healthz`. In the
dashboard:

1. Set the secrets as service variables, and `PORT=9090` so Railway's health
   check and public domain target the admin listener.
2. Generate a public domain for the admin API (HTTPS).
3. Add TCP proxies for ports 1080 and 8080. Clients connect to the
   `*.proxy.rlwy.net:<port>` address Railway assigns.
4. Attach a volume at `/data` when `accounts_db` is in use.
