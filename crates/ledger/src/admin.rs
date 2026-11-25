//! Admin API on `SqliteAccounts`, beyond the `AccountStore` trait.

use crate::secrets::{hash_secret, new_account_id, new_key_id, new_secret};
use crate::store::{account_exists, key_from_row, kyc_to_str, KEY_COLUMNS};
use crate::{now_unix, storage, SqliteAccounts};
use humanip_core::accounts::{KeyRecord, KycTier, MicroUsd, StoreError};
use rusqlite::{params, ErrorCode, OptionalExtension};

/// One row of `list_accounts`.
#[derive(Clone, Debug, PartialEq)]
pub struct AccountSummary {
    pub account_id: String,
    pub name: String,
    pub balance_micro: MicroUsd,
    pub created_at: i64,
}

/// Random ids are 48 bits; on the off chance of a collision, try again.
const ID_ATTEMPTS: usize = 4;

fn is_constraint(e: &rusqlite::Error) -> bool {
    e.sqlite_error_code() == Some(ErrorCode::ConstraintViolation)
}

impl SqliteAccounts {
    /// Create an account with a zero balance. Returns `acct_` + 12 hex.
    pub fn create_account(&self, name: &str) -> Result<String, StoreError> {
        let conn = self.lock();
        for _ in 0..ID_ATTEMPTS {
            let id = new_account_id()?;
            match conn.execute(
                "INSERT INTO accounts (account_id, name, created_at) VALUES (?1, ?2, ?3)",
                params![id, name, now_unix()],
            ) {
                Ok(_) => return Ok(id),
                Err(e) if is_constraint(&e) => continue,
                Err(e) => return Err(storage(e)),
            }
        }
        Err(StoreError::Storage(
            "could not allocate an account id".into(),
        ))
    }

    /// Issue a key. The returned secret is shown once; only its hash is stored.
    pub fn issue_key(
        &self,
        account_id: &str,
        name: &str,
        kyc: KycTier,
        monthly_cap_micro: Option<MicroUsd>,
    ) -> Result<(KeyRecord, String), StoreError> {
        let conn = self.lock();
        if !account_exists(&conn, account_id)? {
            return Err(StoreError::NotFound(account_id.into()));
        }
        for _ in 0..ID_ATTEMPTS {
            let key_id = new_key_id()?;
            let secret = new_secret()?;
            let res = conn.execute(
                "INSERT INTO keys (key_id, account_id, name, secret_sha256, kyc_tier,
                     monthly_cap_micro, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    key_id,
                    account_id,
                    name,
                    &hash_secret(&secret)[..],
                    kyc_to_str(kyc),
                    monthly_cap_micro,
                    now_unix()
                ],
            );
            match res {
                Ok(_) => {
                    let rec = KeyRecord {
                        key_id,
                        account_id: account_id.into(),
                        name: name.into(),
                        kyc_tier: kyc,
                        monthly_cap_micro,
                        revoked: false,
                    };
                    return Ok((rec, secret));
                }
                Err(e) if is_constraint(&e) => continue,
                Err(e) => return Err(storage(e)),
            }
        }
        Err(StoreError::Storage("could not allocate a key id".into()))
    }

    /// Replace a key's secret. Same `key_id`; the old secret stops working at
    /// once. A revoked key stays revoked.
    pub fn rotate_key(&self, key_id: &str) -> Result<String, StoreError> {
        let secret = new_secret()?;
        let n = self
            .lock()
            .execute(
                "UPDATE keys SET secret_sha256 = ?1 WHERE key_id = ?2",
                params![&hash_secret(&secret)[..], key_id],
            )
            .map_err(storage)?;
        if n == 0 {
            return Err(StoreError::NotFound(key_id.into()));
        }
        Ok(secret)
    }

    /// Revoke a key. Idempotent: the first revocation time is kept.
    pub fn revoke_key(&self, key_id: &str) -> Result<(), StoreError> {
        self.update_key(
            key_id,
            "UPDATE keys SET revoked_at = COALESCE(revoked_at, ?1) WHERE key_id = ?2",
            params![now_unix(), key_id],
        )
    }

    /// Set or clear (`None`) a key's monthly spend cap.
    pub fn set_cap(&self, key_id: &str, cap_micro: Option<MicroUsd>) -> Result<(), StoreError> {
        self.update_key(
            key_id,
            "UPDATE keys SET monthly_cap_micro = ?1 WHERE key_id = ?2",
            params![cap_micro, key_id],
        )
    }

    pub fn set_kyc(&self, key_id: &str, kyc: KycTier) -> Result<(), StoreError> {
        self.update_key(
            key_id,
            "UPDATE keys SET kyc_tier = ?1 WHERE key_id = ?2",
            params![kyc_to_str(kyc), key_id],
        )
    }

    fn update_key(
        &self,
        key_id: &str,
        sql: &str,
        p: impl rusqlite::Params,
    ) -> Result<(), StoreError> {
        match self.lock().execute(sql, p).map_err(storage)? {
            0 => Err(StoreError::NotFound(key_id.into())),
            _ => Ok(()),
        }
    }

    /// One key by id, or `NotFound`.
    pub fn get_key(&self, key_id: &str) -> Result<KeyRecord, StoreError> {
        let sql = format!("SELECT {KEY_COLUMNS} FROM keys WHERE key_id = ?1");
        self.lock()
            .query_row(&sql, [key_id], key_from_row)
            .optional()
            .map_err(storage)?
            .ok_or_else(|| StoreError::NotFound(key_id.into()))
    }

    /// Keys of one account, oldest first. `NotFound` for an unknown account.
    pub fn list_keys(&self, account_id: &str) -> Result<Vec<KeyRecord>, StoreError> {
        let conn = self.lock();
        if !account_exists(&conn, account_id)? {
            return Err(StoreError::NotFound(account_id.into()));
        }
        let sql = format!(
            "SELECT {KEY_COLUMNS} FROM keys WHERE account_id = ?1 ORDER BY created_at, key_id"
        );
        let mut stmt = conn.prepare(&sql).map_err(storage)?;
        let rows = stmt
            .query_map([account_id], key_from_row)
            .map_err(storage)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(storage)
    }

    /// Balance recomputed from the journal: credits minus usage. Should always
    /// equal `balance()`; exposed for audits and tests.
    pub fn journal_balance(&self, account_id: &str) -> Result<MicroUsd, StoreError> {
        self.lock()
            .query_row(
                "SELECT (SELECT COALESCE(SUM(amount_micro), 0) FROM credits WHERE account_id = ?1)
                      - (SELECT COALESCE(SUM(total_micro), 0) FROM usage WHERE account_id = ?1)",
                [account_id],
                |r| r.get(0),
            )
            .map_err(storage)
    }

    /// All accounts, oldest first.
    pub fn list_accounts(&self) -> Result<Vec<AccountSummary>, StoreError> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(
                "SELECT account_id, name, balance_micro, created_at FROM accounts
                 ORDER BY created_at, account_id",
            )
            .map_err(storage)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(AccountSummary {
                    account_id: r.get(0)?,
                    name: r.get(1)?,
                    balance_micro: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })
            .map_err(storage)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(storage)
    }
}
