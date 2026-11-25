//! `AccountStore` for `SqliteAccounts`.

use crate::secrets::{ct_eq, hash_secret};
use crate::{now_unix, storage, SqliteAccounts};
use humanip_core::accounts::{
    month_start_unix, price_usage, AccountStore, Admission, Charge, Denial, KeyRecord, KycTier,
    MicroUsd, StoreError, UsageEvent, UsageRow,
};
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};

pub(crate) const KEY_COLUMNS: &str =
    "key_id, account_id, name, kyc_tier, monthly_cap_micro, revoked_at IS NOT NULL";

pub(crate) fn kyc_to_str(k: KycTier) -> &'static str {
    match k {
        KycTier::None => "none",
        KycTier::Basic => "basic",
        KycTier::Verified => "verified",
    }
}

pub(crate) fn kyc_from_str(s: &str) -> Option<KycTier> {
    match s {
        "none" => Some(KycTier::None),
        "basic" => Some(KycTier::Basic),
        "verified" => Some(KycTier::Verified),
        _ => None,
    }
}

/// Maps a row selected with [`KEY_COLUMNS`].
pub(crate) fn key_from_row(r: &Row<'_>) -> rusqlite::Result<KeyRecord> {
    let tier: String = r.get(3)?;
    Ok(KeyRecord {
        key_id: r.get(0)?,
        account_id: r.get(1)?,
        name: r.get(2)?,
        // The CHECK constraint makes anything else impossible.
        kyc_tier: kyc_from_str(&tier).unwrap_or_default(),
        monthly_cap_micro: r.get(4)?,
        revoked: r.get(5)?,
    })
}

pub(crate) fn account_exists(conn: &Connection, account_id: &str) -> Result<bool, StoreError> {
    conn.query_row(
        "SELECT 1 FROM accounts WHERE account_id = ?1",
        [account_id],
        |_| Ok(()),
    )
    .optional()
    .map(|o| o.is_some())
    .map_err(storage)
}

fn to_i64(v: u64, what: &str) -> Result<i64, StoreError> {
    i64::try_from(v).map_err(|_| StoreError::Storage(format!("{what} out of range: {v}")))
}

impl AccountStore for SqliteAccounts {
    fn authenticate(&self, presented_secret: &str) -> Result<Option<KeyRecord>, StoreError> {
        let want = hash_secret(presented_secret);
        let conn = self.lock();
        // The index lookup on the hash is not constant time, but it leaks only
        // facts about SHA-256 outputs, which say nothing about the secret. The
        // final comparison of the stored digest is constant time.
        let sql = format!("SELECT {KEY_COLUMNS}, secret_sha256 FROM keys WHERE secret_sha256 = ?1");
        let row = conn
            .query_row(&sql, [&want[..]], |r| {
                Ok((key_from_row(r)?, r.get::<_, Vec<u8>>(6)?))
            })
            .optional()
            .map_err(storage)?;
        Ok(row.and_then(|(rec, stored)| ct_eq(&stored, &want).then_some(rec)))
    }

    fn admit(&self, key_id: &str, now_unix: i64) -> Result<Admission, StoreError> {
        let conn = self.lock();
        let (revoked, cap, balance): (bool, Option<MicroUsd>, MicroUsd) = conn
            .query_row(
                "SELECT k.revoked_at IS NOT NULL, k.monthly_cap_micro, a.balance_micro
                 FROM keys k JOIN accounts a ON a.account_id = k.account_id
                 WHERE k.key_id = ?1",
                [key_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(storage)?
            .ok_or_else(|| StoreError::NotFound(key_id.into()))?;
        if revoked {
            return Ok(Admission::Denied(Denial::Revoked));
        }
        if balance <= 0 {
            return Ok(Admission::Denied(Denial::InsufficientBalance {
                balance_micro: balance,
            }));
        }
        if let Some(cap) = cap {
            let spent: MicroUsd = conn
                .query_row(
                    "SELECT COALESCE(SUM(total_micro), 0) FROM usage
                     WHERE key_id = ?1 AND at_unix >= ?2",
                    params![key_id, month_start_unix(now_unix)],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            if spent >= cap {
                return Ok(Admission::Denied(Denial::SpendCapReached {
                    cap_micro: cap,
                    spent_micro: spent,
                }));
            }
        }
        Ok(Admission::Allowed)
    }

    fn record_usage(&self, ev: &UsageEvent) -> Result<Charge, StoreError> {
        let up = to_i64(ev.bytes_up, "bytes_up")?;
        let down = to_i64(ev.bytes_down, "bytes_down")?;
        let total_bytes = ev
            .bytes_up
            .checked_add(ev.bytes_down)
            .ok_or_else(|| StoreError::Storage("byte count overflow".into()))?;
        let charge = price_usage(total_bytes, ev.provider_price_per_gb, self.take_rate);
        let mut conn = self.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let account: String = tx
            .query_row(
                "SELECT account_id FROM keys WHERE key_id = ?1",
                [&ev.key_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?
            .ok_or_else(|| StoreError::NotFound(ev.key_id.clone()))?;
        tx.execute(
            "INSERT INTO usage (key_id, account_id, provider, domain, bytes_up, bytes_down,
                 price_per_gb, provider_cost_micro, fee_micro, total_micro, at_unix)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                ev.key_id,
                account,
                ev.provider,
                ev.domain,
                up,
                down,
                ev.provider_price_per_gb,
                charge.provider_cost_micro,
                charge.fee_micro,
                charge.total_micro,
                ev.at_unix
            ],
        )
        .map_err(storage)?;
        // May go negative for in-flight traffic; `admit` stops the next connection.
        tx.execute(
            "UPDATE accounts SET balance_micro = balance_micro - ?1 WHERE account_id = ?2",
            params![charge.total_micro, account],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(charge)
    }

    fn credit(
        &self,
        account_id: &str,
        amount_micro: MicroUsd,
        external_ref: &str,
    ) -> Result<bool, StoreError> {
        if amount_micro <= 0 {
            return Err(StoreError::Storage(format!(
                "credit amount must be positive, got {amount_micro}"
            )));
        }
        let mut conn = self.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let seen = tx
            .query_row(
                "SELECT 1 FROM credits WHERE external_ref = ?1",
                [external_ref],
                |_| Ok(()),
            )
            .optional()
            .map_err(storage)?;
        if seen.is_some() {
            return Ok(false);
        }
        if !account_exists(&tx, account_id)? {
            return Err(StoreError::NotFound(account_id.into()));
        }
        // The UNIQUE constraint on external_ref is the real guard; the check
        // above only turns the common duplicate into a clean `false`.
        tx.execute(
            "INSERT INTO credits (account_id, amount_micro, external_ref, at_unix)
             VALUES (?1, ?2, ?3, ?4)",
            params![account_id, amount_micro, external_ref, now_unix()],
        )
        .map_err(storage)?;
        tx.execute(
            "UPDATE accounts SET balance_micro = balance_micro + ?1 WHERE account_id = ?2",
            params![amount_micro, account_id],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(true)
    }

    fn balance(&self, account_id: &str) -> Result<MicroUsd, StoreError> {
        self.lock()
            .query_row(
                "SELECT balance_micro FROM accounts WHERE account_id = ?1",
                [account_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?
            .ok_or_else(|| StoreError::NotFound(account_id.into()))
    }

    fn usage_since(
        &self,
        account_id: Option<&str>,
        since_unix: i64,
    ) -> Result<Vec<UsageRow>, StoreError> {
        let conn = self.lock();
        // BINARY collation orders like Rust's `String` Ord, matching MemoryAccounts.
        let mut stmt = conn
            .prepare_cached(
                "SELECT key_id, provider, domain, SUM(bytes_up + bytes_down), SUM(total_micro)
                 FROM usage
                 WHERE at_unix >= ?1 AND (?2 IS NULL OR account_id = ?2)
                 GROUP BY key_id, provider, domain
                 ORDER BY key_id, provider, domain",
            )
            .map_err(storage)?;
        let rows = stmt
            .query_map(params![since_unix, account_id], |r| {
                Ok(UsageRow {
                    key_id: r.get(0)?,
                    provider: r.get(1)?,
                    domain: r.get(2)?,
                    bytes: r.get::<_, i64>(3)? as u64,
                    total_micro: r.get(4)?,
                })
            })
            .map_err(storage)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(storage)
    }
}
