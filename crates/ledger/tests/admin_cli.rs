//! Smoke tests for `humanip-admin`: the arg handler in process, and the real
//! binary as a subprocess.

use humanip_ledger::cli::{run, CliError};
use std::path::Path;
use std::process::Command;

fn call(db: &Path, args: &[&str]) -> Result<String, CliError> {
    let mut argv = vec!["--db".to_string(), db.display().to_string()];
    argv.extend(args.iter().map(|s| s.to_string()));
    let mut out = Vec::new();
    run(&argv, &mut out).map(|()| String::from_utf8(out).unwrap())
}

fn field(out: &str, name: &str) -> String {
    out.lines()
        .find_map(|l| l.strip_prefix(&format!("{name}: ")))
        .unwrap()
        .to_string()
}

#[test]
fn issue_credit_balance_and_list_via_handler() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("l.db");
    let acct = call(&db, &["account", "create", "acme"])
        .unwrap()
        .trim()
        .to_string();
    assert!(acct.starts_with("acct_"));
    let issued = call(
        &db,
        &[
            "key",
            "issue",
            &acct,
            "ci",
            "--kyc",
            "basic",
            "--cap-usd",
            "50",
        ],
    )
    .unwrap();
    let key = field(&issued, "key_id");
    assert!(field(&issued, "secret").starts_with("hip_live_"));
    assert!(issued.contains("WARNING"));
    assert_eq!(call(&db, &["balance", &acct]).unwrap(), "$0.000000\n");
    let c = call(&db, &["credit", &acct, "12.5", "--ref", "cs_1"]).unwrap();
    assert!(c.contains("credited $12.500000"));
    let dup = call(&db, &["credit", &acct, "12.5", "--ref", "cs_1"]).unwrap();
    assert!(dup.contains("already applied"));
    assert_eq!(call(&db, &["balance", &acct]).unwrap(), "$12.500000\n");
    let listed = call(&db, &["key", "list", &acct]).unwrap();
    assert_eq!(listed, format!("{key}\tci\tbasic\t$50.000000\tactive\n"));
    call(&db, &["key", "cap", &key, "none"]).unwrap();
    call(&db, &["key", "kyc", &key, "verified"]).unwrap();
    call(&db, &["key", "revoke", &key]).unwrap();
    let listed = call(&db, &["key", "list", &acct]).unwrap();
    assert_eq!(listed, format!("{key}\tci\tverified\tno cap\trevoked\n"));
    let rotated = call(&db, &["key", "rotate", &key]).unwrap();
    assert_ne!(field(&rotated, "secret"), field(&issued, "secret"));
    let usage = call(&db, &["usage", &acct, "--since-days", "7"]).unwrap();
    assert_eq!(usage.lines().count(), 1); // header only
    assert!(call(&db, &["usage"]).is_ok());
    assert!(call(&db, &["account", "list"])
        .unwrap()
        .contains("acme\t$12.500000"));
}

#[test]
fn handler_reports_usage_and_runtime_errors() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("l.db");
    assert!(matches!(
        run(&["balance".into(), "x".into()], &mut Vec::new()),
        Err(CliError::Usage(_))
    ));
    assert!(matches!(call(&db, &[]), Err(CliError::Usage(_))));
    assert!(matches!(
        call(&db, &["frobnicate"]),
        Err(CliError::Usage(_))
    ));
    assert!(matches!(
        call(&db, &["credit", "acct_x", "5"]),
        Err(CliError::Usage(_))
    ));
    assert!(matches!(
        call(&db, &["credit", "acct_x", "abc", "--ref", "r"]),
        Err(CliError::Usage(_))
    ));
    assert!(matches!(
        call(&db, &["key", "issue", "a", "b", "--bogus", "1"]),
        Err(CliError::Usage(_))
    ));
    assert!(matches!(
        call(&db, &["key", "issue", "a", "b", "--kyc", "gold"]),
        Err(CliError::Usage(_))
    ));
    assert!(matches!(
        call(&db, &["balance", "acct_missing"]),
        Err(CliError::Failed(_))
    ));
    assert!(matches!(
        call(&db, &["key", "issue", "acct_missing", "n"]),
        Err(CliError::Failed(_))
    ));
}

#[test]
fn binary_issue_then_balance() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("bin.db");
    let bin = env!("CARGO_BIN_EXE_humanip-admin");
    let run_bin = |args: &[&str]| {
        let out = Command::new(bin)
            .arg("--db")
            .arg(&db)
            .args(args)
            .output()
            .unwrap();
        (out.status.code(), String::from_utf8(out.stdout).unwrap())
    };
    let (code, acct) = run_bin(&["account", "create", "acme"]);
    assert_eq!(code, Some(0));
    let acct = acct.trim().to_string();
    let (code, issued) = run_bin(&["key", "issue", &acct, "ci", "--cap-usd", "50"]);
    assert_eq!(code, Some(0));
    assert!(field(&issued, "secret").starts_with("hip_live_"));
    assert_eq!(run_bin(&["credit", &acct, "3", "--ref", "r1"]).0, Some(0));
    assert_eq!(
        run_bin(&["balance", &acct]),
        (Some(0), "$3.000000\n".into())
    );
    assert_eq!(run_bin(&["nonsense"]).0, Some(2));
    assert_eq!(run_bin(&["balance", "acct_missing"]).0, Some(1));
}
