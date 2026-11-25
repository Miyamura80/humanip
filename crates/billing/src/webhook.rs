//! Stripe webhook signature verification.
//!
//! The `Stripe-Signature` header looks like `t=1700000000,v1=<hex>[,v1=<hex>]`
//! (plus `v0=` test signatures, which are ignored). The expected signature is
//! HMAC-SHA256 keyed by the endpoint secret over `"{t}.{payload}"`. Every `v1`
//! is checked in constant time; any match passes. The timestamp must be within
//! `tolerance_secs` of now to block replays.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use thiserror::Error;

/// Stripe's recommended replay window.
pub const DEFAULT_TOLERANCE_SECS: i64 = 300;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WebhookError {
    #[error("signature header has no valid timestamp")]
    MissingTimestamp,
    #[error("signature header has no v1 signatures")]
    NoSignatures,
    #[error("timestamp outside the tolerance window")]
    TimestampOutsideTolerance,
    #[error("no v1 signature matches the payload")]
    SignatureMismatch,
    #[error("webhook secret is empty")]
    EmptySecret,
}

/// Verify a Stripe webhook delivery. `now_unix` is injected for testability.
pub fn verify_webhook(
    payload: &[u8],
    sig_header: &str,
    secret: &str,
    now_unix: i64,
    tolerance_secs: i64,
) -> Result<(), WebhookError> {
    if secret.is_empty() {
        return Err(WebhookError::EmptySecret);
    }
    let mut timestamp: Option<i64> = None;
    let mut sigs: Vec<Vec<u8>> = Vec::new();
    let mut saw_v1 = false;
    for part in sig_header.split(',') {
        let Some((k, v)) = part.trim().split_once('=') else {
            continue;
        };
        match k {
            "t" => timestamp = v.parse().ok(),
            "v1" => {
                saw_v1 = true;
                // Undecodable values can never match; skip them.
                if let Ok(bytes) = hex::decode(v) {
                    sigs.push(bytes);
                }
            }
            _ => {}
        }
    }
    let t = timestamp.ok_or(WebhookError::MissingTimestamp)?;
    if !saw_v1 {
        return Err(WebhookError::NoSignatures);
    }
    if now_unix.abs_diff(t) > tolerance_secs.max(0) as u64 {
        return Err(WebhookError::TimestampOutsideTolerance);
    }
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).map_err(|_| WebhookError::EmptySecret)?;
    mac.update(t.to_string().as_bytes());
    mac.update(b".");
    mac.update(payload);
    // Check every candidate without short-circuiting so timing does not
    // reveal which position matched.
    let mut matched = false;
    for sig in &sigs {
        matched |= mac.clone().verify_slice(sig).is_ok();
    }
    if matched {
        Ok(())
    } else {
        Err(WebhookError::SignatureMismatch)
    }
}

/// Compute the `v1` hex signature for `"{t}.{payload}"`. Exposed for tests
/// and local tooling that needs to sign fixtures.
pub fn sign_payload(payload: &[u8], secret: &str, t: i64) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac accepts any key");
    mac.update(t.to_string().as_bytes());
    mac.update(b".");
    mac.update(payload);
    hex::encode(mac.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "whsec_test_secret";
    const NOW: i64 = 1_795_608_000;
    const BODY: &[u8] = br#"{"id":"evt_1","type":"checkout.session.completed"}"#;

    fn header(t: i64, sigs: &[String]) -> String {
        let mut h = format!("t={t}");
        for s in sigs {
            h.push_str(&format!(",v1={s}"));
        }
        h
    }

    #[test]
    fn valid_signature_passes() {
        let h = header(NOW, &[sign_payload(BODY, SECRET, NOW)]);
        assert_eq!(verify_webhook(BODY, &h, SECRET, NOW, 300), Ok(()));
    }

    #[test]
    fn known_vector_matches_hmac_sha256() {
        // HMAC-SHA256(key="whsec_test_secret", "1795608000.{BODY}"), computed
        // independently with `openssl dgst -sha256 -hmac`.
        let sig = sign_payload(BODY, SECRET, NOW);
        assert_eq!(
            sig,
            "34c1ab9e329a6020092b40c34d8773e14b9f0751d4b9d178736c4536361d37e2"
        );
        let h = format!("t={NOW},v0=deadbeef,v1={sig}");
        assert!(verify_webhook(BODY, &h, SECRET, NOW, 300).is_ok());
    }

    #[test]
    fn tampered_payload_rejected() {
        let h = header(NOW, &[sign_payload(BODY, SECRET, NOW)]);
        let tampered = br#"{"id":"evt_1","type":"checkout.session.completed "}"#;
        assert_eq!(
            verify_webhook(tampered, &h, SECRET, NOW, 300),
            Err(WebhookError::SignatureMismatch)
        );
    }

    #[test]
    fn wrong_secret_rejected() {
        let h = header(NOW, &[sign_payload(BODY, "whsec_other", NOW)]);
        assert_eq!(
            verify_webhook(BODY, &h, SECRET, NOW, 300),
            Err(WebhookError::SignatureMismatch)
        );
    }

    #[test]
    fn stale_and_future_timestamps_rejected() {
        let old = NOW - 301;
        let h = header(old, &[sign_payload(BODY, SECRET, old)]);
        assert_eq!(
            verify_webhook(BODY, &h, SECRET, NOW, 300),
            Err(WebhookError::TimestampOutsideTolerance)
        );
        let future = NOW + 301;
        let h = header(future, &[sign_payload(BODY, SECRET, future)]);
        assert_eq!(
            verify_webhook(BODY, &h, SECRET, NOW, 300),
            Err(WebhookError::TimestampOutsideTolerance)
        );
        // Exactly at the edge is still accepted.
        let edge = NOW - 300;
        let h = header(edge, &[sign_payload(BODY, SECRET, edge)]);
        assert!(verify_webhook(BODY, &h, SECRET, NOW, 300).is_ok());
    }

    #[test]
    fn multiple_v1_all_wrong_rejected() {
        let h = header(
            NOW,
            &[
                sign_payload(BODY, "whsec_a", NOW),
                sign_payload(BODY, "whsec_b", NOW),
                "not-hex".into(),
            ],
        );
        assert_eq!(
            verify_webhook(BODY, &h, SECRET, NOW, 300),
            Err(WebhookError::SignatureMismatch)
        );
    }

    #[test]
    fn multiple_v1_one_valid_passes() {
        // Stripe sends several v1 values while a secret is being rolled.
        let h = header(
            NOW,
            &[
                sign_payload(BODY, "whsec_old", NOW),
                sign_payload(BODY, SECRET, NOW),
            ],
        );
        assert!(verify_webhook(BODY, &h, SECRET, NOW, 300).is_ok());
    }

    #[test]
    fn malformed_headers_rejected() {
        let sig = sign_payload(BODY, SECRET, NOW);
        assert_eq!(
            verify_webhook(BODY, &format!("v1={sig}"), SECRET, NOW, 300),
            Err(WebhookError::MissingTimestamp)
        );
        assert_eq!(
            verify_webhook(BODY, &format!("t=abc,v1={sig}"), SECRET, NOW, 300),
            Err(WebhookError::MissingTimestamp)
        );
        assert_eq!(
            verify_webhook(BODY, &format!("t={NOW}"), SECRET, NOW, 300),
            Err(WebhookError::NoSignatures)
        );
        assert_eq!(
            verify_webhook(BODY, "", SECRET, NOW, 300),
            Err(WebhookError::MissingTimestamp)
        );
        assert_eq!(
            verify_webhook(BODY, &format!("t={NOW},v1={sig}"), "", NOW, 300),
            Err(WebhookError::EmptySecret)
        );
    }
}
