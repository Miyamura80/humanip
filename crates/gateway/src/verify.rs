//! Verify a provider adapter against a real account: `humanip-verify`.
//!
//! ```text
//! ProviderConfig.upstream.render(session, sticky, country)
//!        |
//!        v
//!   upstream::connect -> raw tunnel --GET--> IP echo  (api.ipify.org:80)
//!                                   --GET--> geo API  (ip-api.com:80)
//!
//! session A x3 sticky ---> reachable, exit_ip, sticky (same IP x3)
//! session B    sticky ---> sessions_differ (soft)
//! fresh x3   rotating ---> rotating (soft)
//! every exit IP -> geo ---> country (--country XX), not_datacenter (soft)
//! ```
//!
//! "Soft" checks warn instead of failing: they depend on pool size and luck.
//! Transport errors always fail, whichever check hits them.

pub mod fetch;
mod geo;
pub mod output;

use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::config::ProviderConfig;
use crate::providers::fnv64;
use crate::Upstream;
use fetch::{FetchError, Geo, HttpUrl};
use geo::{country_check, datacenter_check};

/// Default IP-echo endpoint (plain HTTP).
pub const DEFAULT_IP_URL: &str = "http://api.ipify.org/?format=json";
/// Default geo endpoint. `{ip}` and `{fields}` are filled in per lookup.
pub const DEFAULT_GEO_URL: &str = "http://ip-api.com/json/{ip}?fields={fields}";
/// Fields requested from the geo endpoint, in one call per exit IP.
pub const GEO_FIELDS: &str = "status,message,countryCode,hosting,proxy,mobile";

/// Check names, in report order.
pub const CHECKS: [&str; 7] = [
    "reachable",
    "exit_ip",
    "sticky",
    "sessions_differ",
    "rotating",
    "country",
    "not_datacenter",
];

/// Knobs for one verification run.
#[derive(Clone, Debug)]
pub struct Opts {
    /// Uppercase ISO alpha-2. When set, every connection asks for it and
    /// every exit IP must geolocate there.
    pub country: Option<String>,
    /// Per tunnel open and per response.
    pub timeout: Duration,
    pub ip_url: String,
    /// Template with `{ip}` and optionally `{fields}`.
    pub geo_url: String,
    /// Connections for the sticky and rotating checks (at least 2).
    pub samples: usize,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            country: None,
            timeout: Duration::from_millis(15_000),
            ip_url: DEFAULT_IP_URL.into(),
            geo_url: DEFAULT_GEO_URL.into(),
            samples: 3,
        }
    }
}

impl Opts {
    /// URLs parse, country is two letters, samples >= 2.
    pub fn validate(&self) -> Result<(), String> {
        HttpUrl::parse(&self.ip_url).map_err(|e| format!("--ip-url {e}"))?;
        if !self.geo_url.contains("{ip}") {
            return Err("--geo-url must contain {ip}".into());
        }
        geo_url(&self.geo_url, "192.0.2.1".parse().unwrap())
            .map_err(|e| format!("--geo-url {e}"))?;
        if let Some(c) = &self.country {
            if c.len() != 2 || !c.bytes().all(|b| b.is_ascii_alphabetic()) {
                return Err(format!("--country {c:?} must be 2 ASCII letters"));
            }
        }
        if self.samples < 2 {
            return Err("samples must be at least 2".into());
        }
        Ok(())
    }
}

fn geo_url(template: &str, ip: IpAddr) -> Result<HttpUrl, String> {
    HttpUrl::parse(
        &template
            .replace("{ip}", &ip.to_string())
            .replace("{fields}", GEO_FIELDS),
    )
}

/// Outcome of one check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pass,
    Warn,
    Fail,
    /// Not run: an earlier check failed, or the check was not requested.
    Skip,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Warn => "WARN",
            Status::Fail => "FAIL",
            Status::Skip => "SKIP",
        }
    }
}

/// One row of the report.
#[derive(Clone, Debug)]
pub struct Check {
    pub name: &'static str,
    pub status: Status,
    pub detail: String,
    /// Wall time for the whole check.
    pub elapsed_ms: u64,
    /// Per-request totals, in order.
    pub samples_ms: Vec<u64>,
}

/// Everything learned about one provider.
#[derive(Clone, Debug)]
pub struct Report {
    pub provider: String,
    /// Named adapter kind (`decodo`, ...) when the upstream is one.
    pub adapter: Option<&'static str>,
    /// `socks5`, `http` or `direct`.
    pub protocol: &'static str,
    /// Vendor gateway host:port (empty for direct).
    pub endpoint: String,
    pub checks: Vec<Check>,
}

impl Report {
    /// Any check failed.
    pub fn failed(&self) -> bool {
        self.checks.iter().any(|c| c.status == Status::Fail)
    }

    /// Check by name.
    pub fn check(&self, name: &str) -> Option<&Check> {
        self.checks.iter().find(|c| c.name == name)
    }
}

/// A fresh `hip` + 16 hex id, unique per call and per run.
fn new_session() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let n = N.fetch_add(1, Ordering::Relaxed);
    let seed = format!("{nanos}-{}-{n}", std::process::id());
    format!("hip{:016x}", fnv64(&seed))
}

fn describe(up: &Upstream) -> (&'static str, String) {
    match up {
        Upstream::Direct => ("direct", String::new()),
        Upstream::Socks5 { addr, .. } => ("socks5", addr.clone()),
        Upstream::Http { addr, .. } => ("http", addr.clone()),
    }
}

/// One IP-echo sample.
struct Sample {
    ip: Result<IpAddr, FetchError>,
    connect_ms: u64,
    total_ms: u64,
}

struct Runner<'a> {
    provider: &'a ProviderConfig,
    opts: &'a Opts,
    ip_url: HttpUrl,
}

impl Runner<'_> {
    fn render(&self, session: &str, sticky: bool) -> Upstream {
        self.provider
            .upstream
            .render(session, sticky, self.opts.country.as_deref())
    }

    async fn sample(&self, up: &Upstream) -> Sample {
        let start = Instant::now();
        match fetch::get(up, &self.ip_url, self.opts.timeout).await {
            Ok(f) => Sample {
                ip: fetch::parse_ip(&f.body).map_err(FetchError::Http),
                connect_ms: f.connect_ms,
                total_ms: f.total_ms,
            },
            Err(e) => Sample {
                ip: Err(e),
                connect_ms: 0,
                total_ms: fetch::ms(start),
            },
        }
    }

    async fn geo(&self, up: &Upstream, ip: IpAddr) -> (Result<Geo, String>, u64) {
        let start = Instant::now();
        let r = match geo_url(&self.opts.geo_url, ip) {
            Ok(url) => match fetch::get(up, &url, self.opts.timeout).await {
                Ok(f) => fetch::parse_geo(&f.body),
                Err(e) => Err(e.to_string()),
            },
            Err(e) => Err(e),
        };
        (r, fetch::ms(start))
    }
}

fn check(name: &'static str, status: Status, detail: impl Into<String>) -> Check {
    Check {
        name,
        status,
        detail: detail.into(),
        elapsed_ms: 0,
        samples_ms: Vec::new(),
    }
}

fn timed(mut c: Check, start: Instant, samples_ms: Vec<u64>) -> Check {
    c.elapsed_ms = fetch::ms(start);
    c.samples_ms = samples_ms;
    c
}

fn join_ips(ips: &[IpAddr]) -> String {
    ips.iter()
        .map(IpAddr::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn distinct(ips: impl IntoIterator<Item = IpAddr>) -> Vec<IpAddr> {
    let mut out: Vec<IpAddr> = Vec::new();
    for ip in ips {
        if !out.contains(&ip) {
            out.push(ip);
        }
    }
    out
}

/// Run every check against `provider`. Never panics on network trouble:
/// failures become [`Status::Fail`] rows.
pub async fn run(provider: &ProviderConfig, opts: &Opts) -> Report {
    let probe = provider.upstream.render("hip0000000000000000", true, None);
    let (protocol, endpoint) = describe(&probe);
    let mut report = Report {
        provider: provider.name.clone(),
        adapter: provider.upstream.unverified_adapter(),
        protocol,
        endpoint,
        checks: Vec::new(),
    };
    let ip_url = match opts.validate().and_then(|()| HttpUrl::parse(&opts.ip_url)) {
        Ok(u) => u,
        Err(e) => {
            report.checks.push(check("reachable", Status::Fail, e));
            return report;
        }
    };
    let r = Runner {
        provider,
        opts,
        ip_url,
    };
    report.checks = checks(&r).await;
    report
}

async fn checks(r: &Runner<'_>) -> Vec<Check> {
    let mut out = Vec::new();
    let target = r.ip_url.target();
    let sid_a = new_session();
    let up_a = r.render(&sid_a, true);

    // reachable + exit_ip: the first sticky sample.
    let start = Instant::now();
    let first = r.sample(&up_a).await;
    let ip_a = match &first.ip {
        Err(FetchError::Tunnel(e)) => {
            out.push(timed(
                check(
                    "reachable",
                    Status::Fail,
                    format!("no tunnel to {target}: {e}"),
                ),
                start,
                vec![first.total_ms],
            ));
            skip_rest(&mut out, "needs a working tunnel");
            return out;
        }
        Err(FetchError::Http(e)) => {
            out.push(timed(
                check(
                    "reachable",
                    Status::Pass,
                    format!("tunnel to {target} open"),
                ),
                start,
                vec![first.connect_ms],
            ));
            out.push(timed(
                check("exit_ip", Status::Fail, format!("IP echo: {e}")),
                start,
                vec![first.total_ms],
            ));
            skip_rest(&mut out, "needs an exit IP");
            return out;
        }
        Ok(ip) => *ip,
    };
    out.push(timed(
        check(
            "reachable",
            Status::Pass,
            format!("tunnel to {target} open in {}ms", first.connect_ms),
        ),
        start,
        vec![first.connect_ms],
    ));
    out.push(timed(
        check("exit_ip", Status::Pass, format!("exit {ip_a}")),
        start,
        vec![first.total_ms],
    ));

    // sticky: the same session keeps its exit.
    let start = Instant::now();
    let mut sticky_ips = vec![ip_a];
    let mut times = vec![first.total_ms];
    let mut err = None;
    for i in 1..r.opts.samples {
        let s = r.sample(&up_a).await;
        times.push(s.total_ms);
        match s.ip {
            Ok(ip) => sticky_ips.push(ip),
            Err(e) => {
                err.get_or_insert(format!("connection {}: {e}", i + 1));
            }
        }
    }
    let seen = distinct(sticky_ips.iter().copied());
    let c = match (err, seen.len()) {
        (Some(e), _) => check("sticky", Status::Fail, e),
        (None, 1) => check(
            "sticky",
            Status::Pass,
            format!(
                "{} connections on one session exited via {ip_a}",
                sticky_ips.len()
            ),
        ),
        (None, _) => check(
            "sticky",
            Status::Fail,
            format!(
                "one session gave {} exits ({}): session parameter ignored",
                seen.len(),
                join_ips(&seen)
            ),
        ),
    };
    out.push(timed(c, start, times));

    // sessions_differ: a second session gets its own exit (soft).
    let start = Instant::now();
    let s = r.sample(&r.render(&new_session(), true)).await;
    let mut all_ips = sticky_ips.clone();
    let c = match s.ip {
        Err(e) => check("sessions_differ", Status::Fail, e.to_string()),
        Ok(ip) if ip == ip_a => check(
            "sessions_differ",
            Status::Warn,
            format!("second session also exited via {ip}: small pool or session ignored"),
        ),
        Ok(ip) => {
            all_ips.push(ip);
            check(
                "sessions_differ",
                Status::Pass,
                format!("second session exited via {ip}"),
            )
        }
    };
    out.push(timed(c, start, vec![s.total_ms]));

    // rotating: no session parameter, the vendor should vary the exit (soft).
    let start = Instant::now();
    let (mut rot, mut times, mut err) = (Vec::new(), Vec::new(), None);
    for i in 0..r.opts.samples {
        let s = r.sample(&r.render(&new_session(), false)).await;
        times.push(s.total_ms);
        match s.ip {
            Ok(ip) => rot.push(ip),
            Err(e) => {
                err.get_or_insert(format!("connection {}: {e}", i + 1));
            }
        }
    }
    all_ips.extend(rot.iter().copied());
    let seen = distinct(rot.iter().copied());
    let c = match (err, seen.len()) {
        (Some(e), _) => check("rotating", Status::Fail, e),
        (None, 1) => check(
            "rotating",
            Status::Warn,
            format!(
                "{} rotating connections all exited via {}",
                rot.len(),
                seen[0]
            ),
        ),
        (None, n) => check(
            "rotating",
            Status::Pass,
            format!("{} rotating connections used {n} exits", rot.len()),
        ),
    };
    out.push(timed(c, start, times));

    // country + not_datacenter: one geo lookup per distinct exit IP, sent
    // through session A so the lookup also leaves via the provider.
    let start = Instant::now();
    let mut geos = Vec::new();
    let mut times = Vec::new();
    for ip in distinct(all_ips) {
        let (g, t) = r.geo(&up_a, ip).await;
        times.push(t);
        geos.push((ip, g));
    }
    let country = country_check(r.opts.country.as_deref(), &geos);
    if country.status == Status::Skip {
        out.push(country);
    } else {
        out.push(timed(country, start, times.clone()));
    }
    out.push(timed(datacenter_check(&geos), start, times));
    out
}

fn skip_rest(out: &mut Vec<Check>, why: &str) {
    for name in CHECKS.iter().skip(out.len()) {
        out.push(check(name, Status::Skip, why));
    }
}
