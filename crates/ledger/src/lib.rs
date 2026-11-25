//! SQLite implementation of `humanip_core::accounts::AccountStore`, plus the
//! admin API (accounts, key issue/rotate/revoke, caps) and the
//! `humanip-admin` CLI handler.
//!
//! All money is integer micro-USD. One connection sits behind a `Mutex`, so
//! every call blocks the calling thread for the duration of one SQLite
//! statement or transaction; async callers should use `spawn_blocking`.

mod admin;
pub mod cli;
mod schema;
mod secrets;
mod store;

pub use admin::AccountSummary;
pub use secrets::{hash_secret, SECRET_PREFIX};

use humanip_core::accounts::StoreError;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

/// SQLite-backed account store. `Send + Sync`; share it behind an `Arc`.
pub struct SqliteAccounts {
    conn: Mutex<Connection>,
    take_rate: f64,
}

pub(crate) fn storage(e: rusqlite::Error) -> StoreError {
    StoreError::Storage(e.to_string())
}

impl SqliteAccounts {
    /// Open (creating if needed) a database file, enable WAL and migrate.
    /// Uses `humanip_core::TAKE_RATE` for pricing.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::from_conn(Connection::open(path).map_err(storage)?)
    }

    /// A private in-memory database, for tests.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::from_conn(Connection::open_in_memory().map_err(storage)?)
    }

    fn from_conn(mut conn: Connection) -> Result<Self, StoreError> {
        conn.busy_timeout(Duration::from_secs(5)).map_err(storage)?;
        // In-memory databases answer "memory" here; that is fine.
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(storage)?;
        // FULL: a credited Stripe payment must survive power loss once acked.
        conn.pragma_update(None, "synchronous", "FULL")
            .map_err(storage)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(storage)?;
        schema::migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            take_rate: humanip_core::TAKE_RATE,
        })
    }

    /// The schema version this database is at (`PRAGMA user_version`).
    pub fn schema_version(&self) -> Result<i64, StoreError> {
        self.lock()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(storage)
    }

    /// A panic mid-transaction rolls the transaction back on drop, so a
    /// poisoned lock still guards a consistent connection.
    pub(crate) fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub(crate) fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn store_is_send_sync_and_migrated() {
        assert_send_sync::<SqliteAccounts>();
        let s = SqliteAccounts::open_in_memory().unwrap();
        assert_eq!(s.schema_version().unwrap(), schema::LATEST);
    }
}
