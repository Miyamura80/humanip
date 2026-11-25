//! `humanip-verify`: check a provider adapter against a real account.
//!
//! Exit codes: 0 all checks pass or only warn, 1 any check failed, 2 usage
//! or config error.

use std::process::ExitCode;
use std::time::Duration;

use humanip_gateway::config::{self, ProviderConfig};
use humanip_gateway::verify::{self, output, Opts};

const USAGE: &str = "usage: humanip-verify --config <gateway.yaml> (--provider <NAME> | --all)
                      [--country XX] [--timeout-ms 15000] [--json]
                      [--ip-url http://api.ipify.org/?format=json]
                      [--geo-url 'http://ip-api.com/json/{ip}?fields={fields}']

Checks: reachable, exit_ip, sticky, sessions_differ, rotating, country,
not_datacenter. Exit 0 on pass or warnings only, 1 on any failure, 2 on usage
errors. Every check sends real traffic through the provider.";

struct Args {
    config: String,
    providers: Vec<String>,
    all: bool,
    json: bool,
    opts: Opts,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut a = Args {
        config: String::new(),
        providers: Vec::new(),
        all: false,
        json: false,
        opts: Opts::default(),
    };
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--config" => a.config = value()?,
            "--provider" => a.providers.push(value()?),
            "--all" => a.all = true,
            "--json" => a.json = true,
            "--country" => a.opts.country = Some(value()?.to_ascii_uppercase()),
            "--timeout-ms" => {
                let v = value()?;
                let ms: u64 = v
                    .parse()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| format!("--timeout-ms {v:?} must be a positive integer"))?;
                a.opts.timeout = Duration::from_millis(ms);
            }
            "--ip-url" => a.opts.ip_url = value()?,
            "--geo-url" => a.opts.geo_url = value()?,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if a.config.is_empty() {
        return Err("--config is required".into());
    }
    if a.all == !a.providers.is_empty() {
        return Err("pass exactly one of --provider NAME or --all".into());
    }
    a.opts.validate()?;
    Ok(a)
}

fn usage_error(msg: &str) -> ExitCode {
    if !msg.is_empty() {
        eprintln!("humanip-verify: {msg}");
    }
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

#[tokio::main]
async fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => return usage_error(&e),
    };
    let yaml = match std::fs::read_to_string(&args.config) {
        Ok(y) => y,
        Err(e) => return usage_error(&format!("cannot read {}: {e}", args.config)),
    };
    let cfg = match config::parse_yaml(&yaml) {
        Ok(c) => c,
        Err(e) => return usage_error(&format!("invalid config {}: {e}", args.config)),
    };
    let selected: Vec<&ProviderConfig> = if args.all {
        cfg.providers.iter().collect()
    } else {
        let mut v = Vec::new();
        for name in &args.providers {
            match cfg.provider(name) {
                Some(p) => v.push(p),
                None => {
                    let known: Vec<&str> = cfg.providers.iter().map(|p| p.name.as_str()).collect();
                    return usage_error(&format!(
                        "no provider {name:?} in {} (have: {})",
                        args.config,
                        known.join(", ")
                    ));
                }
            }
        }
        v
    };

    let mut reports = Vec::new();
    for p in selected {
        if !args.json {
            eprintln!("verifying {} ...", p.name);
        }
        let r = verify::run(p, &args.opts).await;
        if !args.json {
            println!("{}", output::table(&r));
        }
        reports.push(r);
    }
    if args.json {
        println!("{}", output::json(&reports));
    }
    if reports.iter().any(verify::Report::failed) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
