//! Hand-rolled argument handling for `humanip-admin`. `run` does all the
//! work and writes to `out`, so tests can drive it without a subprocess.

use crate::SqliteAccounts;
use humanip_core::accounts::{AccountStore, KycTier, MicroUsd, MICROS_PER_USD};
use std::io::Write;

pub const USAGE: &str = "usage: humanip-admin --db PATH <command>

commands:
  account create NAME
  account list
  key issue ACCOUNT NAME [--kyc none|basic|verified] [--cap-usd USD]
  key rotate KEY_ID
  key revoke KEY_ID
  key list ACCOUNT
  key cap KEY_ID USD|none
  key kyc KEY_ID none|basic|verified
  credit ACCOUNT USD --ref REF
  balance ACCOUNT
  usage [ACCOUNT] [--since-days 30]";

#[derive(Debug, PartialEq)]
pub enum CliError {
    /// Bad arguments; the binary prints `USAGE` and exits 2.
    Usage(String),
    /// The command ran and failed; exit 1.
    Failed(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Usage(m) | CliError::Failed(m) => f.write_str(m),
        }
    }
}

fn usage(m: impl Into<String>) -> CliError {
    CliError::Usage(m.into())
}

fn failed(e: impl std::fmt::Display) -> CliError {
    CliError::Failed(e.to_string())
}

/// Parse a decimal USD amount ("50", "0.25", "12.345678") into micro-USD
/// exactly, without going through floating point.
pub fn parse_usd(s: &str) -> Result<MicroUsd, CliError> {
    let bad = || usage(format!("not a USD amount: {s:?}"));
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    let digits = |d: &str| d.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty() || !digits(whole) || !digits(frac) || frac.len() > 6 {
        return Err(bad());
    }
    let whole: i64 = whole.parse().map_err(|_| bad())?;
    let frac_micro: i64 = format!("{frac:0<6}").parse().map_err(|_| bad())?;
    whole
        .checked_mul(MICROS_PER_USD)
        .and_then(|w| w.checked_add(frac_micro))
        .ok_or_else(bad)
}

/// "$12.345678", with a leading '-' for negative balances.
pub fn format_usd(micro: MicroUsd) -> String {
    let sign = if micro < 0 { "-" } else { "" };
    let m = micro.unsigned_abs();
    let per = MICROS_PER_USD as u64;
    format!("{sign}${}.{:06}", m / per, m % per)
}

fn parse_kyc(s: &str) -> Result<KycTier, CliError> {
    match s {
        "none" => Ok(KycTier::None),
        "basic" => Ok(KycTier::Basic),
        "verified" => Ok(KycTier::Verified),
        _ => Err(usage(format!("unknown kyc tier: {s:?}"))),
    }
}

/// Splits `--flag value` pairs out of `args`, leaving positionals in order.
struct Args {
    positional: Vec<String>,
    flags: Vec<(String, String)>,
}

impl Args {
    fn parse(args: &[String]) -> Result<Self, CliError> {
        let mut positional = Vec::new();
        let mut flags = Vec::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if let Some(name) = a.strip_prefix("--") {
                let v = it
                    .next()
                    .ok_or_else(|| usage(format!("--{name} needs a value")))?;
                flags.push((name.to_string(), v.clone()));
            } else {
                positional.push(a.clone());
            }
        }
        Ok(Self { positional, flags })
    }

    fn take(&mut self, name: &str) -> Option<String> {
        let i = self.flags.iter().position(|(k, _)| k == name)?;
        Some(self.flags.remove(i).1)
    }

    /// Every flag must have been taken by the command. Positional arity is
    /// enforced by the exact slice patterns in `run`.
    fn finish(&self) -> Result<(), CliError> {
        match self.flags.first() {
            Some((k, _)) => Err(usage(format!("unknown flag --{k}"))),
            None => Ok(()),
        }
    }
}

/// Run one admin command. `args` excludes the program name.
pub fn run(args: &[String], out: &mut dyn Write) -> Result<(), CliError> {
    let mut a = Args::parse(args)?;
    let db = a.take("db").ok_or_else(|| usage("missing --db PATH"))?;
    let positional = std::mem::take(&mut a.positional);
    let words: Vec<&str> = positional.iter().map(String::as_str).collect();
    if words.is_empty() {
        return Err(usage("missing command"));
    }
    let store = SqliteAccounts::open(&db).map_err(failed)?;
    let w = |out: &mut dyn Write, s: String| writeln!(out, "{s}").map_err(failed);
    match words.as_slice() {
        ["account", "create", name] => {
            a.finish()?;
            let id = store.create_account(name).map_err(failed)?;
            w(out, id)
        }
        ["account", "list"] => {
            a.finish()?;
            for acct in store.list_accounts().map_err(failed)? {
                let bal = format_usd(acct.balance_micro);
                w(out, format!("{}\t{}\t{bal}", acct.account_id, acct.name))?;
            }
            Ok(())
        }
        ["key", "issue", account, name] => {
            let kyc = a.take("kyc").map(|s| parse_kyc(&s)).transpose()?;
            let cap = a.take("cap-usd").map(|s| parse_usd(&s)).transpose()?;
            a.finish()?;
            let (rec, secret) = store
                .issue_key(account, name, kyc.unwrap_or_default(), cap)
                .map_err(failed)?;
            w(out, format!("key_id: {}", rec.key_id))?;
            w(out, format!("secret: {secret}"))?;
            w(
                out,
                "WARNING: save this secret now. It is shown once and cannot be recovered.".into(),
            )
        }
        ["key", "rotate", key_id] => {
            a.finish()?;
            let secret = store.rotate_key(key_id).map_err(failed)?;
            w(out, format!("key_id: {key_id}"))?;
            w(out, format!("secret: {secret}"))?;
            w(
                out,
                "WARNING: save this secret now. The old secret no longer works.".into(),
            )
        }
        ["key", "revoke", key_id] => {
            a.finish()?;
            store.revoke_key(key_id).map_err(failed)?;
            w(out, format!("revoked {key_id}"))
        }
        ["key", "list", account] => {
            a.finish()?;
            for k in store.list_keys(account).map_err(failed)? {
                let cap = k.monthly_cap_micro.map_or("no cap".into(), format_usd);
                let state = if k.revoked { "revoked" } else { "active" };
                let kyc = format!("{:?}", k.kyc_tier).to_lowercase();
                w(
                    out,
                    format!("{}\t{}\t{kyc}\t{cap}\t{state}", k.key_id, k.name),
                )?;
            }
            Ok(())
        }
        ["key", "cap", key_id, cap] => {
            a.finish()?;
            let cap = if *cap == "none" {
                None
            } else {
                Some(parse_usd(cap)?)
            };
            store.set_cap(key_id, cap).map_err(failed)?;
            w(
                out,
                format!(
                    "cap for {key_id}: {}",
                    cap.map_or("none".into(), format_usd)
                ),
            )
        }
        ["key", "kyc", key_id, tier] => {
            a.finish()?;
            store.set_kyc(key_id, parse_kyc(tier)?).map_err(failed)?;
            w(out, format!("kyc for {key_id}: {tier}"))
        }
        ["credit", account, amount] => {
            let reference = a
                .take("ref")
                .ok_or_else(|| usage("credit needs --ref REF"))?;
            a.finish()?;
            let micro = parse_usd(amount)?;
            let applied = store.credit(account, micro, &reference).map_err(failed)?;
            let bal = format_usd(store.balance(account).map_err(failed)?);
            if applied {
                w(
                    out,
                    format!("credited {} to {account}; balance {bal}", format_usd(micro)),
                )
            } else {
                w(
                    out,
                    format!("ref {reference:?} already applied; balance {bal}"),
                )
            }
        }
        ["balance", account] => {
            a.finish()?;
            w(out, format_usd(store.balance(account).map_err(failed)?))
        }
        ["usage", rest @ ..] if rest.len() <= 1 => {
            let days: i64 = match a.take("since-days") {
                Some(d) => d
                    .parse()
                    .map_err(|_| usage(format!("bad --since-days {d:?}")))?,
                None => 30,
            };
            a.finish()?;
            let since = crate::now_unix() - days * 86_400;
            let rows = store
                .usage_since(rest.first().copied(), since)
                .map_err(failed)?;
            w(out, "key_id\tprovider\tdomain\tbytes\tcharged".into())?;
            for r in rows {
                let charged = format_usd(r.total_micro);
                w(
                    out,
                    format!(
                        "{}\t{}\t{}\t{}\t{charged}",
                        r.key_id, r.provider, r.domain, r.bytes
                    ),
                )?;
            }
            Ok(())
        }
        [] => Err(usage("missing command")),
        _ => Err(usage(format!("unknown command: {}", words.join(" ")))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usd_round_trips_exactly() {
        assert_eq!(parse_usd("50").unwrap(), 50_000_000);
        assert_eq!(parse_usd("0.25").unwrap(), 250_000);
        assert_eq!(parse_usd("12.345678").unwrap(), 12_345_678);
        assert!(parse_usd("1.2345678").is_err());
        assert!(parse_usd("-1").is_err());
        assert!(parse_usd(".5").is_err());
        assert!(parse_usd("1e3").is_err());
        assert_eq!(format_usd(-2_100_000), "-$2.100000");
        assert_eq!(format_usd(7), "$0.000007");
    }
}
