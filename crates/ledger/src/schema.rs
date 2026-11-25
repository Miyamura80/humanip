//! Schema and migrations, versioned with `PRAGMA user_version`.

use crate::storage;
use humanip_core::accounts::StoreError;
use rusqlite::{Connection, TransactionBehavior};

/// Ordered migrations; entry `i` moves the database from version `i` to `i + 1`.
const MIGRATIONS: &[&str] = &[V1];

pub const LATEST: i64 = MIGRATIONS.len() as i64;

/// v1: accounts, keys, credits, usage. Money columns are integer micro-USD.
/// `accounts.balance_micro` is a running total kept in the same transaction
/// as every credit and usage insert, so it always equals
/// `SUM(credits.amount_micro) - SUM(usage.total_micro)` for the account.
const V1: &str = "
CREATE TABLE accounts (
    account_id    TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    balance_micro INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL
) STRICT;

CREATE TABLE keys (
    key_id            TEXT PRIMARY KEY,
    account_id        TEXT NOT NULL REFERENCES accounts(account_id),
    name              TEXT NOT NULL,
    secret_sha256     BLOB NOT NULL UNIQUE,
    kyc_tier          TEXT NOT NULL CHECK (kyc_tier IN ('none', 'basic', 'verified')),
    monthly_cap_micro INTEGER,
    revoked_at        INTEGER,
    created_at        INTEGER NOT NULL
) STRICT;
CREATE INDEX keys_by_account ON keys(account_id);

CREATE TABLE credits (
    credit_id    INTEGER PRIMARY KEY,
    account_id   TEXT NOT NULL REFERENCES accounts(account_id),
    amount_micro INTEGER NOT NULL,
    external_ref TEXT NOT NULL UNIQUE,
    at_unix      INTEGER NOT NULL
) STRICT;
CREATE INDEX credits_by_account ON credits(account_id);

CREATE TABLE usage (
    usage_id            INTEGER PRIMARY KEY,
    key_id              TEXT NOT NULL REFERENCES keys(key_id),
    account_id          TEXT NOT NULL REFERENCES accounts(account_id),
    provider            TEXT NOT NULL,
    domain              TEXT NOT NULL,
    bytes_up            INTEGER NOT NULL,
    bytes_down          INTEGER NOT NULL,
    price_per_gb        REAL NOT NULL,
    provider_cost_micro INTEGER NOT NULL,
    fee_micro           INTEGER NOT NULL,
    total_micro         INTEGER NOT NULL,
    at_unix             INTEGER NOT NULL
) STRICT;
CREATE INDEX usage_by_key_time ON usage(key_id, at_unix);
CREATE INDEX usage_by_account_time ON usage(account_id, at_unix);
";

pub fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage)?;
    let current: i64 = tx
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(storage)?;
    if current > LATEST {
        return Err(StoreError::Storage(format!(
            "database schema v{current} is newer than this build (v{LATEST})"
        )));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        tx.execute_batch(sql).map_err(storage)?;
        tx.pragma_update(None, "user_version", i as i64 + 1)
            .map_err(storage)?;
    }
    tx.commit().map_err(storage)
}
