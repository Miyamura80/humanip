//! `humanip-gateway --config gateway.yaml [--check]`
//!
//! ```text
//!            +-- socks_listen  (raw TCP, SOCKS5) ---------+
//! serve_all -+-- http_listen   (raw TCP, CONNECT/forward) +--> Gateway
//!            +-- admin_listen  (HTTP: healthz, metrics, admin API)
//!            +-- sticky pin sweeper (60 s)
//!            +-- SIGTERM / Ctrl-C: stop accepting, drain, exit 0
//! ```
//!
//! The config path comes from `--config` or `HUMANIP_CONFIG`. `${VAR}` and
//! `${VAR:-default}` in the YAML are replaced from the environment before
//! parsing, so secrets can stay out of the file. `--check` validates and
//! exits 0 (valid) or 1 (invalid).
//!
//! Logs: `HUMANIP_LOG_FORMAT=json` for JSON lines, human-readable otherwise;
//! level filter from `RUST_LOG` (default `info`).

use humanip_gateway::config::{self, GatewayConfig};
use humanip_gateway::server::Gateway;
use humanip_gateway::{admin, metrics};
use std::future::Future;
use std::io;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

const USAGE: &str = "usage: humanip-gateway --config <gateway.yaml> [--check]";

#[tokio::main]
async fn main() {
    init_tracing();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let path = match args.iter().position(|a| a == "--config") {
        Some(i) => args.get(i + 1).cloned(),
        None => std::env::var("HUMANIP_CONFIG").ok(),
    };
    let Some(path) = path else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    let cfg = match load(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("humanip-gateway: {path}: {e}");
            std::process::exit(if check { 1 } else { 2 });
        }
    };
    if check {
        println!(
            "config ok: {} api keys, {} providers, {} routes, admin {}",
            cfg.api_keys.len(),
            cfg.providers.len(),
            cfg.routes.len(),
            cfg.admin_listen.as_deref().unwrap_or("off"),
        );
        warn_unverified(&cfg);
        return;
    }
    if let Err(e) = serve_all(cfg, shutdown_signal()).await {
        tracing::error!(error = %e, "humanip-gateway failed");
        std::process::exit(1);
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr);
    let json = std::env::var("HUMANIP_LOG_FORMAT").is_ok_and(|v| v.eq_ignore_ascii_case("json"));
    if json {
        builder.json().flatten_event(true).init();
    } else {
        builder.pretty().init();
    }
}

/// Read, expand `${VAR}` references and validate a config file.
fn load(path: &str) -> Result<GatewayConfig, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("cannot read: {e}"))?;
    let yaml = expand_env(&raw, |k| std::env::var(k).ok())?;
    config::parse_yaml(&yaml).map_err(|e| e.to_string())
}

/// Replace `${NAME}` and `${NAME:-default}`. An unset `${NAME}` is an error so
/// a missing secret never becomes an empty password. `{session}`-style
/// templates (no `$`) are left alone.
/// Whole-line `#` comments are copied unchanged so documentation can mention
/// `${VAR}`; references in trailing comments are still expanded.
fn expand_env(src: &str, lookup: impl Fn(&str) -> Option<String>) -> Result<String, String> {
    let mut out = String::with_capacity(src.len());
    for line in src.split_inclusive('\n') {
        if line.trim_start().starts_with('#') {
            out.push_str(line);
        } else {
            expand_line(line, &lookup, &mut out)?;
        }
    }
    Ok(out)
}

fn expand_line(
    line: &str,
    lookup: &impl Fn(&str) -> Option<String>,
    out: &mut String,
) -> Result<(), String> {
    let mut rest = line;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| "unterminated ${ in config".to_string())?;
        let expr = &after[..end];
        let (name, default) = match expr.split_once(":-") {
            Some((n, d)) => (n, Some(d)),
            None => (expr, None),
        };
        let valid =
            !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if !valid {
            return Err(format!("invalid variable reference ${{{expr}}}"));
        }
        let value = match (lookup(name), default) {
            (Some(v), _) if !v.is_empty() => v,
            (_, Some(d)) => d.to_string(),
            (Some(v), None) => v,
            (None, None) => return Err(format!("environment variable {name} is not set")),
        };
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(())
}

fn warn_unverified(cfg: &GatewayConfig) {
    for p in &cfg.providers {
        if let Some(kind) = p.upstream.unverified_adapter() {
            tracing::warn!(
                provider = %p.name,
                adapter = kind,
                "adapter host, port and username format are unverified; check them against \
                 the vendor dashboard or use a generic socks5/http upstream"
            );
        }
    }
}

/// Bind every listener, run them over one [`Gateway`] and return `Ok` after
/// `shutdown` resolves and in-flight connections drain (or the grace period,
/// `HUMANIP_SHUTDOWN_GRACE_SECS`, default 10, runs out).
async fn serve_all(cfg: GatewayConfig, shutdown: impl Future<Output = ()>) -> io::Result<()> {
    let socks = TcpListener::bind(&cfg.socks_listen).await?;
    let http = TcpListener::bind(&cfg.http_listen).await?;
    let admin_listener = match &cfg.admin_listen {
        Some(a) => Some(TcpListener::bind(a).await?),
        None => None,
    };
    tracing::info!(
        socks = %socks.local_addr()?,
        http = %http.local_addr()?,
        admin = %admin_listener
            .as_ref()
            .and_then(|l| l.local_addr().ok())
            .map_or("off".to_string(), |a| a.to_string()),
        "humanip-gateway listening"
    );
    warn_unverified(&cfg);
    let token = cfg.admin_token.clone().unwrap_or_default();
    // One store for auth, admission, metering and the admin usage API: the
    // SQLite ledger when accounts_db is set, else a dev store from api_keys.
    let accounts = humanip_gateway::server::accounts_for(&cfg)?;
    let gw = Gateway::new(cfg, accounts.clone());

    let sweeper = gw.clone();
    let sweep = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            let n = sweeper.selector().sessions().purge_expired(Instant::now());
            if n > 0 {
                tracing::debug!(purged = n, "expired sticky pins");
            }
        }
    });

    let admin_task = async {
        match admin_listener {
            Some(l) => admin::serve_admin(l, gw.clone(), token, Some(accounts.clone())).await,
            None => std::future::pending().await,
        }
    };
    let result = tokio::select! {
        r = async {
            tokio::try_join!(gw.clone().serve_socks(socks), gw.clone().serve_http(http), admin_task)
        } => r.map(|_| ()),
        _ = shutdown => {
            tracing::info!("shutdown signal received; no longer accepting connections");
            drain(grace_period()).await;
            Ok(())
        }
    };
    sweep.abort();
    result
}

fn grace_period() -> Duration {
    let secs = std::env::var("HUMANIP_SHUTDOWN_GRACE_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    Duration::from_secs(secs)
}

/// Wait for open client connections (per the metrics gauge) to finish.
async fn drain(grace: Duration) {
    let deadline = Instant::now() + grace;
    loop {
        let open = metrics::global().active_total();
        if open <= 0 {
            return;
        }
        if Instant::now() >= deadline {
            tracing::warn!(open, "grace period over; dropping open connections");
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Resolves on Ctrl-C or, on Unix, SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
}

#[cfg(test)]
mod tests {
    use super::expand_env;

    fn env(k: &str) -> Option<String> {
        match k {
            "TOKEN" => Some("s3cret".into()),
            "EMPTY" => Some(String::new()),
            _ => None,
        }
    }

    #[test]
    fn expands_variables() {
        let out = expand_env("a: ${TOKEN}\nb: \"{session}\"\n", env).unwrap();
        assert_eq!(out, "a: s3cret\nb: \"{session}\"\n");
        assert_eq!(
            expand_env("${MISSING:-0.0.0.0:1080}", env).unwrap(),
            "0.0.0.0:1080"
        );
        assert_eq!(expand_env("${EMPTY:-d}", env).unwrap(), "d");
        assert_eq!(expand_env("${EMPTY}", env).unwrap(), "");
        assert_eq!(expand_env("${TOKEN:-d}", env).unwrap(), "s3cret");
        assert_eq!(
            expand_env("  # see ${MISSING}\nx: 1", env).unwrap(),
            "  # see ${MISSING}\nx: 1"
        );
    }

    #[test]
    fn rejects_bad_references() {
        assert!(expand_env("${MISSING}", env)
            .unwrap_err()
            .contains("MISSING"));
        assert!(expand_env("${TOKEN", env).is_err());
        assert!(expand_env("${bad-name}", env).is_err());
        assert!(expand_env("${}", env).is_err());
    }
}
