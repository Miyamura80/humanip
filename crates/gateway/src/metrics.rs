//! Process metrics and their Prometheus text exposition.
//!
//! Connection counters are fixed arrays of `std` atomics (no lock on the hot
//! path). Per-provider counters live in a `Mutex<HashMap>` because provider
//! names are only known at runtime; the lock is held for a map lookup only.
//!
//! The free functions ([`connection_opened`], [`bytes`], ...) are the hooks
//! the server calls; they update one process-wide [`Metrics`]. Tests build
//! their own [`Metrics`] so they never depend on global state or test order.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

/// Front-end protocol of a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proto {
    Socks5,
    HttpConnect,
    HttpForward,
}

impl Proto {
    pub const ALL: [Proto; 3] = [Proto::Socks5, Proto::HttpConnect, Proto::HttpForward];

    /// Label value used in the exposition.
    pub fn label(self) -> &'static str {
        match self {
            Proto::Socks5 => "socks5",
            Proto::HttpConnect => "http_connect",
            Proto::HttpForward => "http_forward",
        }
    }

    fn idx(self) -> usize {
        self as usize
    }
}

/// How a connection ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnResult {
    Ok,
    AuthFailed,
    Denied,
    Blocked,
    DialFailed,
    ProtocolError,
}

impl ConnResult {
    pub const ALL: [ConnResult; 6] = [
        ConnResult::Ok,
        ConnResult::AuthFailed,
        ConnResult::Denied,
        ConnResult::Blocked,
        ConnResult::DialFailed,
        ConnResult::ProtocolError,
    ];

    /// Label value used in the exposition.
    pub fn label(self) -> &'static str {
        match self {
            ConnResult::Ok => "ok",
            ConnResult::AuthFailed => "auth_failed",
            ConnResult::Denied => "denied",
            ConnResult::Blocked => "blocked",
            ConnResult::DialFailed => "dial_failed",
            ConnResult::ProtocolError => "protocol_error",
        }
    }

    fn idx(self) -> usize {
        self as usize
    }
}

#[derive(Default, Clone, Copy)]
struct ProviderCounters {
    up: u64,
    down: u64,
    dial_failures: u64,
}

/// One set of gateway metrics. The process uses a single global instance.
#[derive(Default)]
pub struct Metrics {
    total: [[AtomicU64; 6]; 3],
    active: [AtomicI64; 3],
    providers: Mutex<HashMap<String, ProviderCounters>>,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn connection_opened(&self, proto: Proto) {
        self.active[proto.idx()].fetch_add(1, Ordering::Relaxed);
    }

    pub fn connection_closed(&self, proto: Proto, result: ConnResult) {
        self.active[proto.idx()].fetch_sub(1, Ordering::Relaxed);
        self.total[proto.idx()][result.idx()].fetch_add(1, Ordering::Relaxed);
    }

    pub fn bytes(&self, provider: &str, up: u64, down: u64) {
        self.with_provider(provider, |c| {
            c.up = c.up.saturating_add(up);
            c.down = c.down.saturating_add(down);
        });
    }

    pub fn dial_failure(&self, provider: &str) {
        self.with_provider(provider, |c| c.dial_failures += 1);
    }

    /// Connections currently open across all protocols.
    pub fn active_total(&self) -> i64 {
        self.active.iter().map(|a| a.load(Ordering::Relaxed)).sum()
    }

    fn with_provider(&self, provider: &str, f: impl FnOnce(&mut ProviderCounters)) {
        let mut map = self.providers.lock().unwrap_or_else(|e| e.into_inner());
        match map.get_mut(provider) {
            Some(c) => f(c),
            None => f(map.entry(provider.to_string()).or_default()),
        }
    }

    /// Prometheus text exposition format 0.0.4.
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(2048);
        header(
            &mut out,
            "humanip_connections_total",
            "counter",
            "Client connections closed, by front-end protocol and outcome.",
        );
        for p in Proto::ALL {
            for r in ConnResult::ALL {
                let v = self.total[p.idx()][r.idx()].load(Ordering::Relaxed);
                let _ = writeln!(
                    out,
                    "humanip_connections_total{{proto=\"{}\",result=\"{}\"}} {v}",
                    p.label(),
                    r.label()
                );
            }
        }
        header(
            &mut out,
            "humanip_connections_active",
            "gauge",
            "Client connections currently open, by front-end protocol.",
        );
        for p in Proto::ALL {
            let v = self.active[p.idx()].load(Ordering::Relaxed);
            let _ = writeln!(
                out,
                "humanip_connections_active{{proto=\"{}\"}} {v}",
                p.label()
            );
        }
        let mut rows: Vec<(String, ProviderCounters)> = {
            let map = self.providers.lock().unwrap_or_else(|e| e.into_inner());
            map.iter().map(|(k, v)| (k.clone(), *v)).collect()
        };
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        header(
            &mut out,
            "humanip_bytes_total",
            "counter",
            "Bytes relayed through each provider; up is client to target.",
        );
        for (name, c) in &rows {
            let name = escape_label(name);
            for (dir, v) in [("up", c.up), ("down", c.down)] {
                let _ = writeln!(
                    out,
                    "humanip_bytes_total{{provider=\"{name}\",direction=\"{dir}\"}} {v}"
                );
            }
        }
        header(
            &mut out,
            "humanip_dial_failures_total",
            "counter",
            "Failed upstream dials, by provider.",
        );
        for (name, c) in &rows {
            let _ = writeln!(
                out,
                "humanip_dial_failures_total{{provider=\"{}\"}} {}",
                escape_label(name),
                c.dial_failures
            );
        }
        out
    }
}

fn header(out: &mut String, name: &str, kind: &str, help: &str) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {kind}");
}

/// Escape a label value: backslash, double quote and newline.
pub fn escape_label(v: &str) -> String {
    let mut s = String::with_capacity(v.len());
    for ch in v.chars() {
        match ch {
            '\\' => s.push_str("\\\\"),
            '"' => s.push_str("\\\""),
            '\n' => s.push_str("\\n"),
            c => s.push(c),
        }
    }
    s
}

static GLOBAL: LazyLock<Metrics> = LazyLock::new(Metrics::new);

/// The process-wide metrics the hooks below update.
pub fn global() -> &'static Metrics {
    &GLOBAL
}

pub fn connection_opened(proto: Proto) {
    GLOBAL.connection_opened(proto);
}

pub fn connection_closed(proto: Proto, result: ConnResult) {
    GLOBAL.connection_closed(proto, result);
}

pub fn bytes(provider: &str, up: u64, down: u64) {
    GLOBAL.bytes(provider, up, down);
}

pub fn dial_failure(provider: &str) {
    GLOBAL.dial_failure(provider);
}

/// Prometheus text exposition of all metrics.
pub fn render() -> String {
    GLOBAL.render()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every non-comment line is `name{labels} value` with a numeric value,
    /// and every metric name has HELP and TYPE before its samples.
    fn assert_well_formed(text: &str) {
        let mut typed = std::collections::HashSet::new();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("# TYPE ") {
                typed.insert(rest.split(' ').next().unwrap().to_string());
                continue;
            }
            if line.starts_with("# HELP ") {
                continue;
            }
            let (series, value) = line.rsplit_once(' ').expect("sample line");
            value.parse::<f64>().expect("numeric value");
            let name = series.split('{').next().unwrap();
            assert!(typed.contains(name), "{name} sampled before TYPE");
            assert!(series.ends_with('}'), "labels closed: {line}");
        }
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn empty_render_has_all_families() {
        let m = Metrics::new();
        let t = m.render();
        assert_well_formed(&t);
        for name in [
            "humanip_connections_total",
            "humanip_connections_active",
            "humanip_bytes_total",
            "humanip_dial_failures_total",
        ] {
            assert!(t.contains(&format!("# HELP {name} ")), "{name}");
            assert!(t.contains(&format!("# TYPE {name} ")), "{name}");
        }
        assert!(t.contains("humanip_connections_total{proto=\"socks5\",result=\"ok\"} 0\n"));
        assert!(t.contains("# TYPE humanip_connections_active gauge\n"));
    }

    #[test]
    fn counters_and_gauge() {
        let m = Metrics::new();
        m.connection_opened(Proto::Socks5);
        m.connection_opened(Proto::Socks5);
        m.connection_opened(Proto::HttpForward);
        m.connection_closed(Proto::Socks5, ConnResult::DialFailed);
        assert_eq!(m.active_total(), 2);
        m.bytes("netnut", 10, 200);
        m.bytes("netnut", 5, 0);
        m.dial_failure("netnut");
        m.dial_failure("iproxy");
        let t = m.render();
        assert_well_formed(&t);
        assert!(
            t.contains("humanip_connections_total{proto=\"socks5\",result=\"dial_failed\"} 1\n")
        );
        assert!(t.contains("humanip_connections_active{proto=\"socks5\"} 1\n"));
        assert!(t.contains("humanip_connections_active{proto=\"http_forward\"} 1\n"));
        assert!(t.contains("humanip_bytes_total{provider=\"netnut\",direction=\"up\"} 15\n"));
        assert!(t.contains("humanip_bytes_total{provider=\"netnut\",direction=\"down\"} 200\n"));
        assert!(t.contains("humanip_bytes_total{provider=\"iproxy\",direction=\"up\"} 0\n"));
        assert!(t.contains("humanip_dial_failures_total{provider=\"iproxy\"} 1\n"));
        // Providers render sorted, so output is stable.
        let i = t.find("provider=\"iproxy\"").unwrap();
        assert!(i < t.find("provider=\"netnut\"").unwrap());
    }

    #[test]
    fn label_values_are_escaped() {
        assert_eq!(escape_label(r#"a"b\c"#), r#"a\"b\\c"#);
        assert_eq!(escape_label("x\ny"), "x\\ny");
        let m = Metrics::new();
        m.dial_failure("we\"ird\\name\n");
        let t = m.render();
        assert_well_formed(&t);
        assert!(t.contains(r#"humanip_dial_failures_total{provider="we\"ird\\name\n"} 1"#));
    }

    #[test]
    fn global_hooks_accumulate() {
        // Unique provider name: other tests may touch the global instance.
        bytes("metrics-test-global-provider", 3, 4);
        dial_failure("metrics-test-global-provider");
        let t = render();
        assert!(t.contains(
            "humanip_bytes_total{provider=\"metrics-test-global-provider\",direction=\"down\"} 4\n"
        ));
        assert!(std::ptr::eq(global(), global()));
    }
}
