//! The `humanip-gateway` binary: `--check`, env expansion, and a full run
//! with the admin listener ending in a graceful SIGTERM shutdown.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_humanip-gateway");

fn write_config(name: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("humanip-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

fn config(socks: &str, http: &str, admin: &str) -> String {
    format!(
        r#"
socks_listen: "{socks}"
http_listen: "{http}"
admin_listen: "{admin}"
admin_token: "${{TEST_ADMIN_TOKEN}}"
api_keys: [{{ name: alice, key: ka }}]
providers:
  - name: chain
    exit_type: isp
    sourcing: verified
    price_per_gb: 2.0
    fraud_score: 5
    max_sticky_minutes: 60
    latency_ms: 10
    upstream: {{ kind: socks5, addr: "127.0.0.1:9", username: "{{session}}", password: p }}
"#
    )
}

fn check(path: &PathBuf, token: Option<&str>) -> std::process::Output {
    let mut cmd = Command::new(BIN);
    cmd.arg("--config").arg(path).arg("--check");
    match token {
        Some(t) => cmd.env("TEST_ADMIN_TOKEN", t),
        None => cmd.env_remove("TEST_ADMIN_TOKEN"),
    };
    cmd.output().unwrap()
}

#[test]
fn check_accepts_a_good_config() {
    let p = write_config(
        "good.yaml",
        &config("127.0.0.1:0", "127.0.0.1:0", "127.0.0.1:0"),
    );
    let out = check(&p, Some("tok"));
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("config ok:"));
}

#[test]
fn check_accepts_the_shipped_examples() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for f in ["examples/gateway.yaml", "deploy/gateway.prod.example.yaml"] {
        let out = Command::new(BIN)
            .arg("--config")
            .arg(root.join(f))
            .arg("--check")
            .env("HUMANIP_ADMIN_TOKEN", "t")
            .env("HUMANIP_API_KEY_DEFAULT", "k")
            .env("NETNUT_PASSWORD", "p")
            .env("IPROXY_PASSWORD", "p")
            .output()
            .unwrap();
        assert!(out.status.success(), "{f}: {out:?}");
    }
}

#[test]
fn check_rejects_a_bad_config() {
    let p = write_config("bad.yaml", "api_keys: []\nproviders: []\n");
    let out = check(&p, None);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    // Unset secret reference is an error, never an empty token.
    let p = write_config(
        "nosecret.yaml",
        &config("127.0.0.1:0", "127.0.0.1:0", "127.0.0.1:0"),
    );
    let out = check(&p, None);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("TEST_ADMIN_TOKEN"));
    let out = check(&PathBuf::from("/nonexistent/gateway.yaml"), None);
    assert_eq!(out.status.code(), Some(1));
}

fn free_port() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().to_string()
}

fn http_get(addr: &str, path: &str) -> Option<String> {
    let mut s = TcpStream::connect(addr).ok()?;
    write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    Some(out)
}

#[cfg(unix)]
#[test]
fn serves_admin_and_stops_on_sigterm() {
    let admin = free_port();
    let p = write_config("run.yaml", &config(&free_port(), &free_port(), &admin));
    let mut child = Command::new(BIN)
        .arg("--config")
        .arg(&p)
        .env("TEST_ADMIN_TOKEN", "tok")
        .env("HUMANIP_LOG_FORMAT", "json")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let body = loop {
        if let Some(r) = http_get(&admin, "/healthz") {
            break r;
        }
        assert!(Instant::now() < deadline, "admin never came up");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(body.starts_with("HTTP/1.1 200 "), "{body}");
    assert!(http_get(&admin, "/metrics")
        .unwrap()
        .contains("humanip_connections_active"));

    let killed = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()
        .unwrap();
    assert!(killed.success());
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        assert!(Instant::now() < deadline, "did not exit on SIGTERM");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "{status:?}");
    let mut logs = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut logs)
        .unwrap();
    let first = logs.lines().next().expect("a log line");
    assert!(
        first.starts_with('{') && first.contains("\"level\""),
        "{first}"
    );
    assert!(logs.contains("shutdown signal received"), "{logs}");
}
