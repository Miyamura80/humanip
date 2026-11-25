//! humanip-verify against loopback fakes: no internet.
//!
//! ```text
//! verify --decodo socks5--> fake provider --"EXIT <ip>\r\n" + bytes--> IP echo / geo
//! ```
//!
//! The fake provider picks an exit IP from the vendor username (sticky by
//! `-session-`, or a counter), records ip -> country for the geo server, and
//! prefixes the tunnel with a one-line `EXIT` preamble the local servers read.

use humanip_gateway::config::{parse_yaml, ProviderConfig};
use humanip_gateway::socks5::{self, Reply};
use humanip_gateway::verify::{self, Opts, Status};
use humanip_gateway::TargetAddr;
use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

type GeoMap = Arc<Mutex<HashMap<String, String>>>;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Same IP per session; rotating connections get a fresh IP.
    Sticky,
    /// Every connection gets a fresh IP, session or not.
    IgnoreSession,
    /// One IP for everything (tiny pool).
    Static,
}

const PASSWORD: &str = "pw-secret-123";

fn user_field<'a>(user: &'a str, key: &str) -> Option<&'a str> {
    let parts: Vec<&str> = user.split('-').collect();
    parts.windows(2).find(|w| w[0] == key).map(|w| w[1])
}

fn ip_for(n: u32) -> String {
    format!("203.0.113.{}", n % 250 + 1)
}

fn hash(s: &str) -> u32 {
    s.bytes().fold(2_166_136_261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16_777_619)
    })
}

async fn fake_provider(mode: Mode, force_country: Option<&'static str>, geo: GeoMap) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let counter = Arc::new(AtomicU32::new(100));
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            let (geo, counter) = (geo.clone(), counter.clone());
            tokio::spawn(async move {
                let Ok((user, pass)) = socks5::negotiate(&mut s).await else {
                    return;
                };
                let ok = pass == PASSWORD;
                socks5::auth_reply(&mut s, ok).await.unwrap();
                if !ok {
                    return;
                }
                let Ok(TargetAddr::Ip(target)) = socks5::read_request(&mut s).await else {
                    return;
                };
                let fresh = || ip_for(counter.fetch_add(1, Ordering::Relaxed));
                let ip = match (mode, user_field(&user, "session")) {
                    (Mode::Static, _) => "198.51.100.9".to_string(),
                    (Mode::Sticky, Some(sid)) => ip_for(hash(sid)),
                    _ => fresh(),
                };
                let cc = force_country
                    .or(user_field(&user, "country"))
                    .unwrap_or("us")
                    .to_ascii_uppercase();
                geo.lock().unwrap().insert(ip.clone(), cc);
                let mut up = TcpStream::connect(target).await.unwrap();
                socks5::reply(&mut s, Reply::Succeeded).await.unwrap();
                up.write_all(format!("EXIT {ip}\r\n").as_bytes())
                    .await
                    .unwrap();
                let _ = tokio::io::copy_bidirectional(&mut s, &mut up).await;
            });
        }
    });
    addr
}

/// Reads the EXIT preamble and the request head; returns (exit ip, path).
async fn read_preamble_and_head(s: &mut BufReader<TcpStream>) -> (String, String) {
    let mut line = String::new();
    s.read_line(&mut line).await.unwrap();
    let exit = line
        .trim()
        .strip_prefix("EXIT ")
        .unwrap_or_default()
        .to_string();
    let mut path = String::new();
    loop {
        let mut l = String::new();
        if s.read_line(&mut l).await.unwrap() == 0 || l == "\r\n" {
            break;
        }
        if path.is_empty() {
            path = l.split_whitespace().nth(1).unwrap_or_default().to_string();
        }
    }
    (exit, path)
}

async fn ip_echo() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (s, _) = l.accept().await.unwrap();
            tokio::spawn(async move {
                let mut s = BufReader::new(s);
                let (exit, _) = read_preamble_and_head(&mut s).await;
                let body = format!("{{\"ip\":\"{exit}\"}}");
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.get_mut().write_all(resp.as_bytes()).await;
            });
        }
    });
    addr
}

/// Geo server: `/json/<ip>?...` -> countryCode from the map, chunked body.
async fn geo_server(geo: GeoMap, hosting: bool) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (s, _) = l.accept().await.unwrap();
            let geo = geo.clone();
            tokio::spawn(async move {
                let mut s = BufReader::new(s);
                let (_, path) = read_preamble_and_head(&mut s).await;
                let ip = path
                    .strip_prefix("/json/")
                    .and_then(|p| p.split('?').next())
                    .unwrap_or_default();
                let cc = geo.lock().unwrap().get(ip).cloned();
                let body = match cc {
                    Some(cc) => format!(
                        "{{\"status\":\"success\",\"countryCode\":\"{cc}\",\
                         \"hosting\":{hosting},\"proxy\":false,\"mobile\":false}}"
                    ),
                    None => "{\"status\":\"fail\",\"message\":\"unknown ip\"}".into(),
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",
                    body.len()
                );
                let _ = s.get_mut().write_all(resp.as_bytes()).await;
            });
        }
    });
    addr
}

struct Env {
    yaml: String,
    opts: Opts,
}

impl Env {
    fn provider(&self) -> ProviderConfig {
        parse_yaml(&self.yaml).unwrap().providers.remove(0)
    }
}

fn provider_yaml(name: &str, endpoint: SocketAddr) -> String {
    format!(
        r#"
  - name: {name}
    exit_type: residential
    sourcing: verified
    price_per_gb: 1.0
    fraud_score: 5
    max_sticky_minutes: 60
    latency_ms: 10
    upstream: {{ kind: decodo, username: cust, password: {PASSWORD}, protocol: socks5, endpoint: "{endpoint}" }}
"#
    )
}

async fn env(mode: Mode, force_country: Option<&'static str>, hosting: bool) -> Env {
    let geo: GeoMap = Arc::default();
    let fake = fake_provider(mode, force_country, geo.clone()).await;
    let echo = ip_echo().await;
    let geo_addr = geo_server(geo, hosting).await;
    let yaml = format!(
        "api_keys: [{{ name: a, key: k }}]\nproviders:{}",
        provider_yaml("fake", fake)
    );
    let opts = Opts {
        country: Some("DE".into()),
        timeout: Duration::from_secs(5),
        ip_url: format!("http://{echo}/?format=json"),
        geo_url: format!("http://{geo_addr}/json/{{ip}}?fields={{fields}}"),
        samples: 3,
    };
    Env { yaml, opts }
}

fn status(r: &verify::Report, name: &str) -> Status {
    r.check(name)
        .unwrap_or_else(|| panic!("no check {name}"))
        .status
}

#[tokio::test]
async fn well_behaved_fake_passes_everything() {
    let e = env(Mode::Sticky, None, false).await;
    let r = verify::run(&e.provider(), &e.opts).await;
    let names: Vec<&str> = r.checks.iter().map(|c| c.name).collect();
    assert_eq!(names, verify::CHECKS);
    for c in &r.checks {
        assert_eq!(c.status, Status::Pass, "{}: {}", c.name, c.detail);
    }
    assert_eq!(r.adapter, Some("decodo"));
    assert!(!r.failed());
}

#[tokio::test]
async fn sticky_fails_when_the_provider_ignores_the_session() {
    let e = env(Mode::IgnoreSession, None, false).await;
    let r = verify::run(&e.provider(), &e.opts).await;
    assert_eq!(status(&r, "sticky"), Status::Fail);
    assert!(r.check("sticky").unwrap().detail.contains("ignored"));
    assert!(r.failed());
}

#[tokio::test]
async fn country_mismatch_fails() {
    let e = env(Mode::Sticky, Some("FR"), false).await;
    let r = verify::run(&e.provider(), &e.opts).await;
    assert_eq!(status(&r, "country"), Status::Fail);
    assert!(r.check("country").unwrap().detail.contains("is FR"));
    assert_eq!(status(&r, "sticky"), Status::Pass);
}

#[tokio::test]
async fn hosting_flag_warns_but_does_not_fail() {
    let e = env(Mode::Sticky, None, true).await;
    let r = verify::run(&e.provider(), &e.opts).await;
    assert_eq!(status(&r, "not_datacenter"), Status::Warn);
    assert!(r
        .check("not_datacenter")
        .unwrap()
        .detail
        .contains("hosting"));
    assert!(!r.failed());
}

#[tokio::test]
async fn tiny_pool_only_warns_on_soft_checks() {
    let mut e = env(Mode::Static, None, false).await;
    e.opts.country = None;
    let r = verify::run(&e.provider(), &e.opts).await;
    assert_eq!(status(&r, "sticky"), Status::Pass);
    assert_eq!(status(&r, "sessions_differ"), Status::Warn);
    assert_eq!(status(&r, "rotating"), Status::Warn);
    assert_eq!(status(&r, "country"), Status::Skip);
    assert!(!r.failed());
}

#[tokio::test]
async fn unreachable_provider_fails_and_skips_the_rest() {
    let dead = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap();
    let e = env(Mode::Sticky, None, false).await;
    let yaml = format!(
        "api_keys: [{{ name: a, key: k }}]\nproviders:{}",
        provider_yaml("dead", dead)
    );
    let p = parse_yaml(&yaml).unwrap().providers.remove(0);
    let r = verify::run(&p, &e.opts).await;
    assert_eq!(status(&r, "reachable"), Status::Fail);
    assert!(r.checks[1..].iter().all(|c| c.status == Status::Skip));
    assert_eq!(r.checks.len(), verify::CHECKS.len());
}

// ---- CLI -------------------------------------------------------------------

#[derive(Deserialize)]
struct JsonOut {
    ok: bool,
    reports: Vec<JsonReport>,
}

#[derive(Deserialize)]
struct JsonReport {
    provider: String,
    adapter: Option<String>,
    ok: bool,
    checks: Vec<JsonCheck>,
}

#[derive(Deserialize)]
struct JsonCheck {
    name: String,
    status: String,
    #[allow(dead_code)]
    detail: String,
    #[allow(dead_code)]
    elapsed_ms: u64,
    samples_ms: Vec<u64>,
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn write_config(yaml: &str) -> std::path::PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("humanip-verify-{}-{n}.yaml", std::process::id()));
    std::fs::write(&p, yaml).unwrap();
    p
}

async fn cli(args: Vec<String>) -> Run {
    tokio::task::spawn_blocking(move || {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_humanip-verify"))
            .args(&args)
            .output()
            .unwrap();
        Run {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    })
    .await
    .unwrap()
}

fn cli_args(e: &Env, cfg: &std::path::Path, extra: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = vec![
        "--config".into(),
        cfg.display().to_string(),
        "--ip-url".into(),
        e.opts.ip_url.clone(),
        "--geo-url".into(),
        e.opts.geo_url.clone(),
        "--timeout-ms".into(),
        "5000".into(),
    ];
    v.extend(extra.iter().map(|s| s.to_string()));
    v
}

#[tokio::test]
async fn cli_passes_with_json_output() {
    let e = env(Mode::Sticky, None, false).await;
    let cfg = write_config(&e.yaml);
    let run = cli(cli_args(
        &e,
        &cfg,
        &["--provider", "fake", "--country", "de", "--json"],
    ))
    .await;
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let out: JsonOut = serde_saphyr::from_str(run.stdout.trim()).expect("json parses");
    assert!(out.ok);
    let r = &out.reports[0];
    assert_eq!(
        (r.provider.as_str(), r.adapter.as_deref(), r.ok),
        ("fake", Some("decodo"), true)
    );
    assert_eq!(r.checks.len(), 7);
    assert!(
        r.checks.iter().all(|c| c.status == "pass"),
        "{}",
        run.stdout
    );
    assert_eq!(
        r.checks
            .iter()
            .find(|c| c.name == "sticky")
            .unwrap()
            .samples_ms
            .len(),
        3
    );
    assert!(!run.stdout.contains(PASSWORD) && !run.stderr.contains(PASSWORD));
}

#[tokio::test]
async fn cli_exits_1_on_failure_and_prints_a_table() {
    let e = env(Mode::IgnoreSession, None, false).await;
    let cfg = write_config(&e.yaml);
    let run = cli(cli_args(&e, &cfg, &["--provider", "fake"])).await;
    assert_eq!(run.code, 1, "{}{}", run.stdout, run.stderr);
    assert!(run.stdout.contains("CHECK") && run.stdout.contains("FAIL"));
    assert!(run.stdout.contains("result: FAIL"));
    assert!(!run.stdout.contains(PASSWORD));
}

#[tokio::test]
async fn cli_all_with_warnings_exits_0() {
    let e = env(Mode::Static, None, true).await;
    let cfg = write_config(&e.yaml);
    let run = cli(cli_args(&e, &cfg, &["--all", "--json"])).await;
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let out: JsonOut = serde_saphyr::from_str(run.stdout.trim()).unwrap();
    assert_eq!(out.reports.len(), 1);
    assert!(out.reports[0].checks.iter().any(|c| c.status == "warn"));
}

#[tokio::test]
async fn cli_usage_errors_exit_2() {
    let e = env(Mode::Sticky, None, false).await;
    let cfg = write_config(&e.yaml);
    let cases: Vec<Vec<String>> = vec![
        vec![],
        vec!["--provider".into(), "fake".into()],
        cli_args(&e, &cfg, &[]),
        cli_args(&e, &cfg, &["--provider", "nope"]),
        cli_args(&e, &cfg, &["--provider", "fake", "--all"]),
        cli_args(&e, &cfg, &["--provider", "fake", "--country", "DEU"]),
        cli_args(&e, &cfg, &["--provider", "fake", "--timeout-ms", "0"]),
        cli_args(&e, &cfg, &["--provider", "fake", "--ip-url", "https://x/"]),
        cli_args(&e, &cfg, &["--provider", "fake", "--bogus"]),
        vec![
            "--config".into(),
            "/nonexistent/x.yaml".into(),
            "--all".into(),
        ],
    ];
    for args in cases {
        let run = cli(args.clone()).await;
        assert_eq!(run.code, 2, "{args:?}: {}", run.stderr);
        assert!(run.stderr.contains("usage:"), "{args:?}");
    }
}
