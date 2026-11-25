# Abuse controls

Upstream proxy vendors forbid resellers from sending unverified customers to
sensitive targets such as banks, government sites and payment processors. The
gateway enforces this with a list of domain rules, each tied to a minimum KYC
tier. A request to a matching host from an account below that tier is refused
before it reaches any vendor.

## What is blocked by default

The gateway ships with no rules. [`examples/blocklist.yaml`](../examples/blocklist.yaml)
is the recommended starting point; paste its `blocked_domains:` list into your
gateway config.

| Category          | Examples                                                 | Required tier |
| ----------------- | -------------------------------------------------------- | ------------- |
| `government`      | `.gov`, `.mil`, `gov.uk`, `.gouv.fr`, `bund.de`, `europa.eu` | `verified` |
| `banking`         | `chase.com`, `bankofamerica.com`, `barclays.co.uk`, `ing.com` | `verified` |
| `payments`        | `paypal.com`, `stripe.com`, `wise.com`, `revolut.com`    | `verified`    |
| `crypto_exchange` | `coinbase.com`, `binance.com`, `kraken.com`              | `basic`       |
| `healthcare`      | `mychart.com`, `nhs.uk`, `kp.org`                        | `verified`    |
| `elections`       | `.vote`, `electoralcommission.org.uk`, `elections.ca`    | `verified`    |

Crypto exchanges sit at `basic` because their public market data is a common,
legitimate target and the exchanges run their own KYC before funds move. Raise
them to `verified` if a vendor you route to asks for it.

The list is a default, not legal advice. Review it against the acceptable-use
policy of every vendor you route to.

## KYC tiers

Tiers are ordered `none < basic < verified`. Each API key carries one.

- `none`: a new account.
- `basic`: typically email and payment method verified.
- `verified`: typically identity documents or business checked.

A rule's `min_kyc` is the lowest tier allowed through. A `none` rule never
blocks anything; `verified` is the highest tier, so use it for "always review".

## How rules match

```yaml
blocked_domains:
  - { pattern: "chase.com", category: banking, min_kyc: verified }
  - { pattern: ".gov", category: government, min_kyc: verified }
  - { pattern: "10.0.0.0/8", category: internal, min_kyc: verified }
```

- `chase.com` matches `chase.com` and every subdomain (`secure.chase.com`), on
  a label boundary: `notchase.com` does not match.
- `.gov` matches hosts ending in `.gov` (`irs.gov`) but not the bare `gov`. Use
  the plain form when the suffix is itself a site (`gov.uk`).
- An IP address or CIDR block matches IP-literal targets only. Domain patterns
  never match IP literals. Legacy IPv4 spellings (`10.1`, `0xa.0.0.1`) and
  IPv4-mapped IPv6 are normalised first.
- When several rules match, the strictest `min_kyc` wins and is reported. Ties
  go to the rule listed first.

Target hosts are lowercased, one trailing dot and any `:port` are removed, and
fullwidth characters and ideographic full stops are folded to ASCII. There is
no punycode converter, so write internationalised names in `xn--` form. A host
that still contains non-ASCII or unexpected characters, or an empty label,
fails closed: it needs the strictest tier in the list.

`humanip_core::abuse::validate_rules`, run during config validation, rejects empty patterns, schemes, paths, ports, wildcards (`*`),
uppercase, non-ASCII, trailing dots, CIDR blocks with host bits set, and
duplicates. `x.com` already covers subdomains, so wildcards are never needed.

## Overriding per deployment

The rule list is plain config, so each deployment owns its copy:

- Remove a rule to allow a category for everyone.
- Change `min_kyc` to loosen or tighten one pattern.
- Add a narrower rule with a stricter tier to tighten one subdomain. A
  narrower rule cannot loosen a broader one, because the strictest match wins.
- Add your own categories (any string) for vendor-specific lists.

## What the client sees

| Transport | Response                                          |
| --------- | ------------------------------------------------- |
| HTTP      | `403 Forbidden` with body `Blocked: <category>`   |
| SOCKS5    | reply code `0x02` (connection not allowed by ruleset) |

The category is the `category` of the strictest matching rule, or
`unrecognized_host` for a host that failed closed. The matched pattern and
required tier are available to the gateway (`Blocked { category, pattern,
required }`) for logging, but are not sent to the client.
