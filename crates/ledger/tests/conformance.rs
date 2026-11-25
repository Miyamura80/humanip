//! One behavioural suite, run against `MemoryAccounts` (the reference) and
//! `SqliteAccounts`. Each scenario is a plain fn taking a fixture builder; the
//! macro at the bottom instantiates it once per implementation.

use humanip_core::accounts::{
    price_usage, AccountStore, Admission, Denial, KeyRecord, KycTier, MemoryAccounts, MemoryKey,
    StoreError, UsageEvent, UsageRow, MICROS_PER_USD,
};
use humanip_core::TAKE_RATE;
use humanip_ledger::SqliteAccounts;
use std::collections::HashMap;

const USD: i64 = MICROS_PER_USD;
/// 2026-11-25T12:00:00Z
const NOW: i64 = 1_795_608_000;
/// 2026-11-01T00:00:00Z
const NOV_1: i64 = 1_793_491_200;

#[derive(Clone, Copy)]
struct Spec {
    account: &'static str,
    cap: Option<i64>,
    balance: i64,
    revoked: bool,
}

fn spec(account: &'static str, cap: Option<i64>, balance: i64) -> Spec {
    Spec {
        account,
        cap,
        balance,
        revoked: false,
    }
}

struct Fixture {
    store: Box<dyn AccountStore>,
    /// One `(record, secret)` per spec, in spec order.
    keys: Vec<(KeyRecord, String)>,
}

impl Fixture {
    fn key(&self, i: usize) -> &str {
        &self.keys[i].0.key_id
    }
    fn acct(&self, i: usize) -> &str {
        &self.keys[i].0.account_id
    }
}

type Build = fn(&[Spec]) -> Fixture;

fn memory(specs: &[Spec]) -> Fixture {
    let m = MemoryAccounts::new(TAKE_RATE);
    let mut keys = Vec::new();
    for (i, s) in specs.iter().enumerate() {
        let record = KeyRecord {
            key_id: format!("key_{i:012x}"),
            account_id: format!("acct_{}", s.account),
            name: format!("k{i}"),
            kyc_tier: KycTier::Basic,
            monthly_cap_micro: s.cap,
            revoked: s.revoked,
        };
        let secret = format!("hip_live_memorysecret{i:011}");
        m.add_key(
            MemoryKey {
                record: record.clone(),
                secret: secret.clone(),
            },
            s.balance,
        );
        keys.push((record, secret));
    }
    Fixture {
        store: Box::new(m),
        keys,
    }
}

fn sqlite(specs: &[Spec]) -> Fixture {
    let db = SqliteAccounts::open_in_memory().unwrap();
    let mut accounts: HashMap<&str, String> = HashMap::new();
    let mut keys = Vec::new();
    for (i, s) in specs.iter().enumerate() {
        let acct = match accounts.get(s.account) {
            Some(a) => a.clone(),
            None => {
                // Like MemoryAccounts::add_key, the first key seeds the balance.
                let a = db.create_account(s.account).unwrap();
                if s.balance > 0 {
                    assert!(db.credit(&a, s.balance, &format!("seed:{a}")).unwrap());
                }
                accounts.insert(s.account, a.clone());
                a
            }
        };
        let (mut rec, secret) = db
            .issue_key(&acct, &format!("k{i}"), KycTier::Basic, s.cap)
            .unwrap();
        if s.revoked {
            db.revoke_key(&rec.key_id).unwrap();
            rec.revoked = true;
        }
        keys.push((rec, secret));
    }
    Fixture {
        store: Box::new(db),
        keys,
    }
}

fn ev(key: &str, provider: &str, domain: &str, up: u64, down: u64, at: i64) -> UsageEvent {
    UsageEvent {
        key_id: key.into(),
        provider: provider.into(),
        domain: domain.into(),
        bytes_up: up,
        bytes_down: down,
        provider_price_per_gb: 4.0,
        at_unix: at,
    }
}

/// The scenario from `MemoryAccounts`' own unit test.
fn reference_scenario(build: Build) {
    let f = build(&[spec("1", Some(2 * USD), 10 * USD)]);
    let (m, k, a) = (&f.store, f.key(0), f.acct(0));
    assert!(m.authenticate("nope").unwrap().is_none());
    assert_eq!(m.authenticate(&f.keys[0].1).unwrap().unwrap(), f.keys[0].0);
    assert_eq!(m.admit(k, NOW).unwrap(), Admission::Allowed);
    // 0.5 GB at $4 + 5% = $2.10, which reaches the $2 cap.
    let c = m
        .record_usage(&ev(k, "p", "d.com", 0, 500_000_000, NOW))
        .unwrap();
    assert_eq!(c.total_micro, 2_100_000);
    assert_eq!(m.balance(a).unwrap(), 7_900_000);
    assert_eq!(
        m.admit(k, NOW).unwrap(),
        Admission::Denied(Denial::SpendCapReached {
            cap_micro: 2 * USD,
            spent_micro: 2_100_000
        })
    );
    assert!(m.credit(a, USD, "cs_1").unwrap());
    assert!(!m.credit(a, USD, "cs_1").unwrap());
    assert_eq!(m.balance(a).unwrap(), 8_900_000);
    let rows = m.usage_since(Some(a), 0).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].bytes, 500_000_000);
}

fn unknown_ids(build: Build) {
    let f = build(&[spec("1", None, USD)]);
    let m = &f.store;
    assert!(m.authenticate("").unwrap().is_none());
    assert!(m.authenticate("hip_live_doesnotexist").unwrap().is_none());
    assert!(matches!(
        m.admit("key_missing", NOW),
        Err(StoreError::NotFound(_))
    ));
    let bad = ev("key_missing", "p", "d", 1, 1, NOW);
    assert!(matches!(m.record_usage(&bad), Err(StoreError::NotFound(_))));
    assert!(matches!(
        m.balance("acct_missing"),
        Err(StoreError::NotFound(_))
    ));
}

fn revoked_key_authenticates_but_is_denied(build: Build) {
    let specs = [Spec {
        revoked: true,
        ..spec("1", None, 10 * USD)
    }];
    let f = build(&specs);
    let rec = f.store.authenticate(&f.keys[0].1).unwrap().unwrap();
    assert!(rec.revoked);
    // Revocation wins over a healthy balance.
    let got = f.store.admit(f.key(0), NOW).unwrap();
    assert_eq!(got, Admission::Denied(Denial::Revoked));
}

fn balance_gate_and_negative_balance(build: Build) {
    let f = build(&[spec("1", None, 0)]);
    let (m, k, a) = (&f.store, f.key(0), f.acct(0));
    let got = m.admit(k, NOW).unwrap();
    assert_eq!(
        got,
        Admission::Denied(Denial::InsufficientBalance { balance_micro: 0 })
    );
    assert!(m.credit(a, 1, "tiny").unwrap());
    assert_eq!(m.admit(k, NOW).unwrap(), Admission::Allowed);
    // In-flight traffic may overdraw; the next admit says by how much.
    m.record_usage(&ev(k, "p", "d", 0, 1_000_000_000, NOW))
        .unwrap();
    assert_eq!(m.balance(a).unwrap(), 1 - 4_200_000);
    assert_eq!(
        m.admit(k, NOW).unwrap(),
        Admission::Denied(Denial::InsufficientBalance {
            balance_micro: 1 - 4_200_000
        })
    );
    assert!(m.credit(a, 5 * USD, "topup").unwrap());
    assert_eq!(m.admit(k, NOW).unwrap(), Admission::Allowed);
}

fn cap_resets_at_month_boundary(build: Build) {
    let f = build(&[spec("1", Some(2 * USD), 100 * USD)]);
    let (m, k) = (&f.store, f.key(0));
    let oct_end = NOV_1 - 1;
    m.record_usage(&ev(k, "p", "d", 0, 500_000_000, oct_end))
        .unwrap();
    assert!(matches!(
        m.admit(k, oct_end).unwrap(),
        Admission::Denied(Denial::SpendCapReached { .. })
    ));
    // October spend does not count against November.
    assert_eq!(m.admit(k, NOV_1).unwrap(), Admission::Allowed);
    // Spend strictly below the cap is allowed; reaching it denies.
    m.record_usage(&ev(k, "p", "d", 0, 400_000_000, NOV_1))
        .unwrap(); // $1.68
    assert_eq!(m.admit(k, NOW).unwrap(), Admission::Allowed);
    m.record_usage(&ev(k, "p", "d", 0, 100_000_000, NOW))
        .unwrap(); // +$0.42
    assert_eq!(
        m.admit(k, NOW).unwrap(),
        Admission::Denied(Denial::SpendCapReached {
            cap_micro: 2 * USD,
            spent_micro: 2_100_000
        })
    );
}

fn cap_is_per_key_and_optional(build: Build) {
    let f = build(&[spec("1", Some(USD), 100 * USD), spec("1", None, 0)]);
    let m = &f.store;
    m.record_usage(&ev(f.key(1), "p", "d", 0, 10_000_000_000, NOW))
        .unwrap();
    assert_eq!(m.admit(f.key(0), NOW).unwrap(), Admission::Allowed);
    assert_eq!(m.admit(f.key(1), NOW).unwrap(), Admission::Allowed);
    // Both keys draw on the one account balance.
    assert_eq!(m.balance(f.acct(0)).unwrap(), 100 * USD - 42 * USD);
}

fn charge_matches_price_usage(build: Build) {
    let f = build(&[spec("1", None, 10 * USD)]);
    let mut e = ev(f.key(0), "p", "d", 123_456_789, 987_654_321, NOW);
    e.provider_price_per_gb = 3.3;
    let want = price_usage(123_456_789 + 987_654_321, 3.3, TAKE_RATE);
    assert_eq!(f.store.record_usage(&e).unwrap(), want);
    assert_eq!(
        f.store.balance(f.acct(0)).unwrap(),
        10 * USD - want.total_micro
    );
}

fn credit_is_idempotent_across_accounts(build: Build) {
    let f = build(&[spec("1", None, 0), spec("2", None, 0)]);
    let m = &f.store;
    assert!(m.credit(f.acct(0), 3 * USD, "cs_x").unwrap());
    assert!(!m.credit(f.acct(0), 3 * USD, "cs_x").unwrap());
    assert!(!m.credit(f.acct(1), 3 * USD, "cs_x").unwrap());
    assert!(m.credit(f.acct(1), 2 * USD, "cs_y").unwrap());
    assert_eq!(m.balance(f.acct(0)).unwrap(), 3 * USD);
    assert_eq!(m.balance(f.acct(1)).unwrap(), 2 * USD);
}

fn row(f: &Fixture, i: usize, p: &str, d: &str, bytes: u64) -> UsageRow {
    let charge = price_usage(bytes, 4.0, TAKE_RATE);
    UsageRow {
        key_id: f.key(i).into(),
        provider: p.into(),
        domain: d.into(),
        bytes,
        total_micro: charge.total_micro,
    }
}

fn usage_grouping_ordering_and_filters(build: Build) {
    let f = build(&[
        spec("1", None, USD),
        spec("1", None, 0),
        spec("2", None, USD),
    ]);
    let m = &f.store;
    // Deliberately out of order; sizes chosen so per-row pricing is exact.
    let script = [
        (2, "zeta", "b.com", 10_000_000, NOW),
        (0, "beta", "a.com", 5_000_000, NOW - 100),
        (1, "alpha", "a.com", 1_000_000, NOW),
        (0, "alpha", "b.com", 2_000_000, NOW),
        (0, "alpha", "a.com", 3_000_000, NOW - 50),
        (0, "alpha", "a.com", 4_000_000, NOW),
        (0, "alpha", "a.com", 9_000_000, NOW - 10_000),
    ];
    for (k, p, d, bytes, at) in script {
        m.record_usage(&ev(f.key(k), p, d, bytes / 2, bytes / 2, at))
            .unwrap();
    }
    let sorted = |mut v: Vec<UsageRow>| {
        v.sort_by(|a, b| {
            (&a.key_id, &a.provider, &a.domain).cmp(&(&b.key_id, &b.provider, &b.domain))
        });
        v
    };
    let acct1 = sorted(vec![
        row(&f, 0, "alpha", "a.com", 7_000_000),
        row(&f, 0, "alpha", "b.com", 2_000_000),
        row(&f, 0, "beta", "a.com", 5_000_000),
        row(&f, 1, "alpha", "a.com", 1_000_000),
    ]);
    assert_eq!(m.usage_since(Some(f.acct(0)), NOW - 1_000).unwrap(), acct1);
    let mut all = acct1.clone();
    all.push(row(&f, 2, "zeta", "b.com", 10_000_000));
    assert_eq!(m.usage_since(None, NOW - 1_000).unwrap(), sorted(all));
    let only2 = vec![row(&f, 2, "zeta", "b.com", 10_000_000)];
    assert_eq!(m.usage_since(Some(f.acct(2)), 0).unwrap(), only2);
    assert!(m.usage_since(Some(f.acct(0)), NOW + 1).unwrap().is_empty());
    let with_old = m.usage_since(Some(f.acct(0)), 0).unwrap();
    let old_row = with_old.iter().find(|r| r.key_id == f.key(0)).unwrap();
    assert_eq!(
        (old_row.provider.as_str(), old_row.bytes),
        ("alpha", 16_000_000)
    );
}

/// Replays one script on both stores and compares every observable output,
/// with key and account ids mapped back to their spec index.
#[test]
fn memory_and_sqlite_agree_on_a_mixed_script() {
    let specs = [
        spec("1", Some(3 * USD), 5 * USD),
        spec("1", None, 0),
        spec("2", Some(USD), 2 * USD),
    ];
    let outputs: Vec<Vec<String>> = [memory as Build, sqlite as Build]
        .iter()
        .map(|build| {
            let f = build(&specs);
            let m = &f.store;
            let norm = |s: String| {
                (0..3).fold(s, |s, i| {
                    s.replace(f.key(i), &format!("K{i}"))
                        .replace(f.acct(i), &format!("A{i}"))
                })
            };
            let mut log = Vec::new();
            for step in 0..40u64 {
                let k = (step % 3) as usize;
                let at = NOV_1 - 5 + step as i64 * 7;
                log.push(format!("{:?}", m.admit(f.key(k), at)));
                let e = ev(
                    f.key(k),
                    ["p1", "p2"][step as usize % 2],
                    "d",
                    step * 3_333_333,
                    777,
                    at,
                );
                log.push(format!("{:?}", m.record_usage(&e)));
                if step % 9 == 0 {
                    log.push(format!(
                        "{:?}",
                        m.credit(f.acct(k), USD, &format!("r{}", step % 18))
                    ));
                }
            }
            for i in 0..3 {
                log.push(format!("{:?}", m.balance(f.acct(i))));
            }
            let mut log: Vec<String> = log.into_iter().map(norm).collect();
            // Rows are ordered by real key ids, which are random; compare them
            // after renaming to K0..K2 and sorting so the check is deterministic.
            let mut rows: Vec<String> = m
                .usage_since(None, NOV_1)
                .expect("usage_since")
                .into_iter()
                .map(|r| norm(format!("{r:?}")))
                .collect();
            rows.sort();
            log.extend(rows);
            log
        })
        .collect();
    assert_eq!(outputs[0], outputs[1]);
}

macro_rules! conformance {
    ($($name:ident),* $(,)?) => {
        mod memory_impl {
            $( #[test] fn $name() { super::$name(super::memory) } )*
        }
        mod sqlite_impl {
            $( #[test] fn $name() { super::$name(super::sqlite) } )*
        }
    };
}

conformance!(
    reference_scenario,
    unknown_ids,
    revoked_key_authenticates_but_is_denied,
    balance_gate_and_negative_balance,
    cap_resets_at_month_boundary,
    cap_is_per_key_and_optional,
    charge_matches_price_usage,
    credit_is_idempotent_across_accounts,
    usage_grouping_ordering_and_filters,
);
