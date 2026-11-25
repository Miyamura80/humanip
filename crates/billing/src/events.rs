//! Turn a verified Stripe event into a balance credit.
//!
//! Only call [`handle_event`] after [`crate::verify_webhook`] has passed.

use humanip_core::accounts::{AccountStore, MicroUsd, StoreError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::MICROS_PER_CENT;

const COMPLETED: &str = "checkout.session.completed";
const ASYNC_SUCCEEDED: &str = "checkout.session.async_payment_succeeded";

/// What happened to an event.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// Credit applied for the first time.
    Applied {
        account_id: String,
        amount_micro: MicroUsd,
        session_id: String,
    },
    /// This session was already credited (Stripe redelivery, or both the
    /// completed and async-succeeded events arrived for one session).
    Duplicate {
        account_id: String,
        session_id: String,
    },
    /// Nothing to do: another event type, an unpaid session, or a non-USD one.
    Ignored { reason: String },
}

#[derive(Debug, Error)]
pub enum EventError {
    #[error("malformed event: {0}")]
    Malformed(String),
    #[error("store: {0}")]
    Store(#[from] StoreError),
}

#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    data: EventData,
}

#[derive(Deserialize)]
struct EventData {
    object: serde_json::Value,
}

#[derive(Deserialize)]
struct Session {
    id: String,
    payment_status: Option<String>,
    amount_total: Option<i64>,
    currency: Option<String>,
    client_reference_id: Option<String>,
    #[serde(default)]
    metadata: std::collections::HashMap<String, String>,
}

fn ignored(reason: impl Into<String>) -> Outcome {
    Outcome::Ignored {
        reason: reason.into(),
    }
}

/// Credit the account named in a paid Checkout Session. Idempotent on the
/// session id via [`AccountStore::credit`].
pub fn handle_event(store: &dyn AccountStore, payload: &[u8]) -> Result<Outcome, EventError> {
    let ev: Event =
        serde_json::from_slice(payload).map_err(|e| EventError::Malformed(e.to_string()))?;
    if ev.kind != COMPLETED && ev.kind != ASYNC_SUCCEEDED {
        return Ok(ignored(format!("event type {}", ev.kind)));
    }
    let s: Session = serde_json::from_value(ev.data.object)
        .map_err(|e| EventError::Malformed(format!("checkout session: {e}")))?;
    // Crypto and other delayed methods complete with payment_status "unpaid";
    // the later async_payment_succeeded event carries "paid".
    if s.payment_status.as_deref() != Some("paid") {
        return Ok(ignored(format!(
            "payment_status {}",
            s.payment_status.as_deref().unwrap_or("missing")
        )));
    }
    if !s
        .currency
        .as_deref()
        .is_some_and(|c| c.eq_ignore_ascii_case("usd"))
    {
        return Ok(ignored("currency is not usd"));
    }
    let cents = s
        .amount_total
        .ok_or_else(|| EventError::Malformed("missing amount_total".into()))?;
    if cents <= 0 {
        return Ok(ignored("non-positive amount_total"));
    }
    // Sessions without an account id were not created by humanip (the Stripe
    // account may sell other things); acknowledge them so Stripe stops retrying.
    let Some(account_id) = s
        .metadata
        .get("account_id")
        .cloned()
        .or(s.client_reference_id)
        .filter(|a| !a.is_empty())
    else {
        return Ok(ignored("no account_id on session"));
    };
    let amount_micro = cents
        .checked_mul(MICROS_PER_CENT)
        .ok_or_else(|| EventError::Malformed("amount_total overflows".into()))?;
    if store.credit(&account_id, amount_micro, &s.id)? {
        Ok(Outcome::Applied {
            account_id,
            amount_micro,
            session_id: s.id,
        })
    } else {
        Ok(Outcome::Duplicate {
            account_id,
            session_id: s.id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use humanip_core::accounts::MemoryAccounts;
    use serde_json::json;

    fn event(kind: &str, status: &str, cents: i64, session: &str) -> Vec<u8> {
        json!({
            "id": "evt_1",
            "object": "event",
            "type": kind,
            "data": { "object": {
                "id": session,
                "object": "checkout.session",
                "mode": "payment",
                "payment_status": status,
                "amount_total": cents,
                "currency": "usd",
                "client_reference_id": "acct_1",
                "metadata": { "account_id": "acct_1" }
            }}
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn paid_session_credits() {
        let m = MemoryAccounts::new(0.05);
        let out = handle_event(&m, &event(COMPLETED, "paid", 2500, "cs_1")).unwrap();
        assert_eq!(
            out,
            Outcome::Applied {
                account_id: "acct_1".into(),
                amount_micro: 25_000_000,
                session_id: "cs_1".into()
            }
        );
        assert_eq!(m.balance("acct_1").unwrap(), 25_000_000);
    }

    #[test]
    fn duplicate_is_not_double_credited() {
        let m = MemoryAccounts::new(0.05);
        let body = event(COMPLETED, "paid", 1000, "cs_dup");
        handle_event(&m, &body).unwrap();
        let again = handle_event(&m, &body).unwrap();
        assert!(matches!(again, Outcome::Duplicate { .. }));
        // The async event for the same session is also a duplicate.
        let async_ev = event(ASYNC_SUCCEEDED, "paid", 1000, "cs_dup");
        assert!(matches!(
            handle_event(&m, &async_ev).unwrap(),
            Outcome::Duplicate { .. }
        ));
        assert_eq!(m.balance("acct_1").unwrap(), 10_000_000);
    }

    #[test]
    fn unpaid_is_ignored() {
        let m = MemoryAccounts::new(0.05);
        let out = handle_event(&m, &event(COMPLETED, "unpaid", 1000, "cs_2")).unwrap();
        assert!(matches!(out, Outcome::Ignored { .. }));
        assert!(m.balance("acct_1").is_err(), "no balance row created");
    }

    #[test]
    fn async_succeeded_credits() {
        let m = MemoryAccounts::new(0.05);
        // Crypto: completed arrives unpaid, then async_payment_succeeded paid.
        handle_event(&m, &event(COMPLETED, "unpaid", 5000, "cs_3")).unwrap();
        let out = handle_event(&m, &event(ASYNC_SUCCEEDED, "paid", 5000, "cs_3")).unwrap();
        assert!(matches!(
            out,
            Outcome::Applied {
                amount_micro: 50_000_000,
                ..
            }
        ));
        assert_eq!(m.balance("acct_1").unwrap(), 50_000_000);
    }

    #[test]
    fn other_event_types_ignored() {
        let m = MemoryAccounts::new(0.05);
        for kind in [
            "payment_intent.succeeded",
            "checkout.session.expired",
            "checkout.session.async_payment_failed",
        ] {
            let out = handle_event(&m, &event(kind, "paid", 1000, "cs_4")).unwrap();
            assert!(matches!(out, Outcome::Ignored { .. }), "{kind}");
        }
        assert!(m.balance("acct_1").is_err());
    }

    #[test]
    fn malformed_json_errors_and_foreign_sessions_ignored() {
        let m = MemoryAccounts::new(0.05);
        assert!(matches!(
            handle_event(&m, b"{not json"),
            Err(EventError::Malformed(_))
        ));
        let no_session = br#"{"type":"checkout.session.completed","data":{"object":{}}}"#;
        assert!(matches!(
            handle_event(&m, no_session),
            Err(EventError::Malformed(_))
        ));
        let no_account = json!({"type": COMPLETED, "data": {"object": {
            "id": "cs_5", "payment_status": "paid", "amount_total": 100, "currency": "usd"
        }}});
        assert!(matches!(
            handle_event(&m, no_account.to_string().as_bytes()),
            Ok(Outcome::Ignored { .. })
        ));
    }

    #[test]
    fn falls_back_to_client_reference_id_and_ignores_non_usd() {
        let m = MemoryAccounts::new(0.05);
        let ev = json!({"type": COMPLETED, "data": {"object": {
            "id": "cs_6", "payment_status": "paid", "amount_total": 700,
            "currency": "usd", "client_reference_id": "acct_9"
        }}});
        assert!(matches!(
            handle_event(&m, ev.to_string().as_bytes()).unwrap(),
            Outcome::Applied { .. }
        ));
        assert_eq!(m.balance("acct_9").unwrap(), 7_000_000);
        let eur = json!({"type": COMPLETED, "data": {"object": {
            "id": "cs_7", "payment_status": "paid", "amount_total": 700,
            "currency": "eur", "client_reference_id": "acct_9"
        }}});
        assert!(matches!(
            handle_event(&m, eur.to_string().as_bytes()).unwrap(),
            Outcome::Ignored { .. }
        ));
    }
}
