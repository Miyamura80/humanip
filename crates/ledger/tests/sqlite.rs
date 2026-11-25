//! Behaviour specific to `SqliteAccounts`: admin API, persistence, concurrency.

use humanip_core::accounts::{
    AccountStore, Admission, Denial, KycTier, StoreError, UsageEvent, MICROS_PER_USD,
};
use humanip_ledger::{SqliteAccounts, SECRET_PREFIX};
use std::sync::Arc;

const USD: i64 = MICROS_PER_USD;
const NOW: i64 = 1_795_608_000;

fn ev(key: &str, bytes: u64) -> UsageEvent {
    UsageEvent {
        key_id: key.into(),
        provider: "p".into(),
        domain: "d.com".into(),
        bytes_up: 0,
        bytes_down: bytes,
        provider_price_per_gb: 4.0,
        at_unix: NOW,
    }
}

fn funded(db: &SqliteAccounts, usd: i64) -> (String, String, String) {
    let a = db.create_account("acme").unwrap();
    db.credit(&a, usd * USD, &format!("seed:{a}")).unwrap();
    let (rec, secret) = db.issue_key(&a, "main", KycTier::Basic, None).unwrap();
    (a, rec.key_id, secret)
}

#[test]
fn rotation_invalidates_old_secret_and_keeps_key_id() {
    let db = SqliteAccounts::open_in_memory().unwrap();
    let (_, key, old) = funded(&db, 1);
    let new = db.rotate_key(&key).unwrap();
    assert_ne!(old, new);
    assert!(new.starts_with(SECRET_PREFIX));
    assert!(db.authenticate(&old).unwrap().is_none());
    assert_eq!(db.authenticate(&new).unwrap().unwrap().key_id, key);
    assert!(matches!(
        db.rotate_key("key_nope"),
        Err(StoreError::NotFound(_))
    ));
}

#[test]
fn revoke_denies_admit_but_still_authenticates() {
    let db = SqliteAccounts::open_in_memory().unwrap();
    let (_, key, secret) = funded(&db, 5);
    assert_eq!(db.admit(&key, NOW).unwrap(), Admission::Allowed);
    db.revoke_key(&key).unwrap();
    db.revoke_key(&key).unwrap(); // idempotent
    assert!(db.authenticate(&secret).unwrap().unwrap().revoked);
    assert_eq!(
        db.admit(&key, NOW).unwrap(),
        Admission::Denied(Denial::Revoked)
    );
    // In-flight traffic on a revoked key is still billed.
    assert!(db.record_usage(&ev(&key, 1_000)).is_ok());
    assert!(matches!(
        db.revoke_key("key_nope"),
        Err(StoreError::NotFound(_))
    ));
}

#[test]
fn set_cap_and_set_kyc_take_effect() {
    let db = SqliteAccounts::open_in_memory().unwrap();
    let (a, key, secret) = funded(&db, 50);
    db.record_usage(&ev(&key, 500_000_000)).unwrap(); // $2.10
    db.set_cap(&key, Some(2 * USD)).unwrap();
    assert!(matches!(
        db.admit(&key, NOW).unwrap(),
        Admission::Denied(Denial::SpendCapReached { .. })
    ));
    db.set_cap(&key, None).unwrap();
    assert_eq!(db.admit(&key, NOW).unwrap(), Admission::Allowed);
    db.set_kyc(&key, KycTier::Verified).unwrap();
    let rec = db.authenticate(&secret).unwrap().unwrap();
    assert_eq!(
        (rec.kyc_tier, rec.monthly_cap_micro),
        (KycTier::Verified, None)
    );
    assert_eq!(db.list_keys(&a).unwrap(), vec![rec.clone()]);
    assert_eq!(db.get_key(&key).unwrap(), rec);
    assert!(matches!(
        db.set_kyc("key_nope", KycTier::None),
        Err(StoreError::NotFound(_))
    ));
}

#[test]
fn admin_listing_and_validation() {
    let db = SqliteAccounts::open_in_memory().unwrap();
    let a = db.create_account("one").unwrap();
    let b = db.create_account("two").unwrap();
    assert!(a.starts_with("acct_") && a.len() == 17 && a != b);
    let (k1, _) = db.issue_key(&a, "x", KycTier::None, Some(USD)).unwrap();
    let (k2, _) = db.issue_key(&a, "y", KycTier::Basic, None).unwrap();
    assert!(k1.key_id.starts_with("key_") && k1.key_id.len() == 16);
    let mut keys = db.list_keys(&a).unwrap();
    keys.sort_by(|x, y| x.name.cmp(&y.name));
    assert_eq!(keys, vec![k1, k2]);
    assert!(db.list_keys(&b).unwrap().is_empty());
    let names: Vec<String> = db
        .list_accounts()
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"one".into()) && names.contains(&"two".into()));
    assert!(matches!(
        db.list_keys("acct_nope"),
        Err(StoreError::NotFound(_))
    ));
    assert!(matches!(
        db.issue_key("acct_nope", "z", KycTier::None, None),
        Err(StoreError::NotFound(_))
    ));
    assert!(matches!(
        db.credit("acct_nope", USD, "r"),
        Err(StoreError::NotFound(_))
    ));
    assert!(matches!(
        db.credit(&a, 0, "r0"),
        Err(StoreError::Storage(_))
    ));
    assert!(matches!(
        db.credit(&a, -5, "r-"),
        Err(StoreError::Storage(_))
    ));
    // A rejected credit must not burn its ref.
    assert!(db.credit(&a, USD, "r").unwrap());
}

#[test]
fn concurrent_record_usage_sums_exactly() {
    let db = Arc::new(SqliteAccounts::open_in_memory().unwrap());
    let (a, key, _) = funded(&db, 1_000);
    let per_event = 4_200; // 1 MB at $4/GB + 5%
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (db, key) = (Arc::clone(&db), key.clone());
            std::thread::spawn(move || {
                (0..250)
                    .map(|_| db.record_usage(&ev(&key, 1_000_000)).unwrap().total_micro)
                    .sum::<i64>()
            })
        })
        .collect();
    let charged: i64 = handles.into_iter().map(|h| h.join().unwrap()).sum();
    assert_eq!(charged, 8 * 250 * per_event);
    assert_eq!(db.balance(&a).unwrap(), 1_000 * USD - charged);
    assert_eq!(db.journal_balance(&a).unwrap(), db.balance(&a).unwrap());
    let rows = db.usage_since(Some(&a), 0).unwrap();
    assert_eq!(
        (rows.len(), rows[0].bytes, rows[0].total_micro),
        (1, 2_000_000_000, charged)
    );
}

#[test]
fn file_db_survives_close_and_reopen_and_stores_no_secret() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.db");
    let (a, key, secret) = {
        let db = SqliteAccounts::open(&path).unwrap();
        let ids = funded(&db, 10);
        db.record_usage(&ev(&ids.1, 500_000_000)).unwrap();
        assert!(db.credit(&ids.0, USD, "cs_1").unwrap());
        ids
    };
    let db = SqliteAccounts::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), 1);
    assert_eq!(db.authenticate(&secret).unwrap().unwrap().key_id, key);
    assert_eq!(db.balance(&a).unwrap(), 10 * USD - 2_100_000 + USD);
    assert!(!db.credit(&a, USD, "cs_1").unwrap());
    assert_eq!(db.usage_since(None, 0).unwrap().len(), 1);
    drop(db);
    let mode: String = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(!bytes.windows(secret.len()).any(|w| w == secret.as_bytes()));
    }
}

#[test]
fn newer_schema_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.db");
    drop(SqliteAccounts::open(&path).unwrap());
    rusqlite::Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", 99)
        .unwrap();
    assert!(matches!(
        SqliteAccounts::open(&path),
        Err(StoreError::Storage(_))
    ));
}

#[test]
fn oversized_byte_counts_are_rejected_without_charging() {
    let db = SqliteAccounts::open_in_memory().unwrap();
    let (a, key, _) = funded(&db, 1);
    assert!(db.record_usage(&ev(&key, u64::MAX)).is_err());
    assert_eq!(db.balance(&a).unwrap(), USD);
}

#[test]
fn account_lookup_reports_name_and_balance() {
    let db = SqliteAccounts::open_in_memory().unwrap();
    let (a, _, _) = funded(&db, 3);
    let acct = db.account(&a).unwrap();
    assert_eq!(acct.account_id, a);
    assert_eq!(acct.name, "acme");
    assert_eq!(acct.balance_micro, 3 * USD);
    assert_eq!(
        db.account("acct_missing"),
        Err(StoreError::NotFound("acct_missing".into()))
    );
}
