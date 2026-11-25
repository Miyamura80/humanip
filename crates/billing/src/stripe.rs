//! Minimal Stripe API client: just enough to open a Checkout Session.
//!
//! `api_base` is configurable so tests point it at a loopback mock server.
//! Nothing in this crate's tests talks to the real Stripe API.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Production Stripe API base. Override with `STRIPE_API_BASE` in tests.
pub const DEFAULT_API_BASE: &str = "https://api.stripe.com";
/// Smallest top-up, in whole USD.
pub const MIN_TOPUP_USD: f64 = 5.0;
/// Largest top-up, in whole USD.
pub const MAX_TOPUP_USD: f64 = 5_000.0;
/// Product name shown on the Checkout page.
pub const PRODUCT_NAME: &str = "humanip credit";

/// The parts of a Checkout Session the caller needs: redirect the customer to `url`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CheckoutSession {
    pub id: String,
    pub url: String,
}

#[derive(Debug, Error)]
pub enum StripeError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("stripe request failed: {0}")]
    Transport(String),
    #[error("stripe returned HTTP {status}: {message}")]
    Api { status: u16, message: String },
    #[error("unexpected stripe response: {0}")]
    BadResponse(String),
}

/// Convert a USD amount to cents, enforcing the $5 to $5,000 bound.
/// Amounts are rounded to the nearest cent.
pub fn cents_from_usd(amount_usd: f64) -> Result<i64, StripeError> {
    if !amount_usd.is_finite() || !(MIN_TOPUP_USD..=MAX_TOPUP_USD).contains(&amount_usd) {
        return Err(StripeError::InvalidInput(format!(
            "amount_usd must be between {MIN_TOPUP_USD} and {MAX_TOPUP_USD}"
        )));
    }
    Ok((amount_usd * 100.0).round() as i64)
}

fn validate_account_id(account_id: &str) -> Result<(), StripeError> {
    let ok = !account_id.is_empty()
        && account_id.len() <= 128
        && account_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(StripeError::InvalidInput(
            "account_id must be 1-128 chars of [A-Za-z0-9_-]".into(),
        ))
    }
}

pub struct StripeClient {
    pub secret_key: String,
    pub api_base: String,
    http: reqwest::Client,
}

impl std::fmt::Debug for StripeClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the secret key.
        f.debug_struct("StripeClient")
            .field("api_base", &self.api_base)
            .finish_non_exhaustive()
    }
}

impl StripeClient {
    /// Build a client. Requires a rustls crypto provider to be installed
    /// (the `appctl` binary installs ring at startup).
    pub fn new(
        secret_key: impl Into<String>,
        api_base: impl Into<String>,
    ) -> Result<Self, StripeError> {
        let api_base = api_base.into();
        let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(20));
        // A loopback mock never needs the system proxy.
        if ["http://127.0.0.1", "http://localhost", "http://[::1]"]
            .iter()
            .any(|p| api_base.starts_with(p))
        {
            builder = builder.no_proxy();
        }
        let http = builder
            .build()
            .map_err(|e| StripeError::Transport(format!("building HTTP client: {e}")))?;
        Ok(Self {
            secret_key: secret_key.into(),
            api_base: api_base.trim_end_matches('/').to_string(),
            http,
        })
    }

    /// The exact form fields sent to `POST /v1/checkout/sessions`, in order.
    pub fn checkout_form(
        account_id: &str,
        amount_usd: f64,
        success_url: &str,
        cancel_url: &str,
    ) -> Result<Vec<(String, String)>, StripeError> {
        validate_account_id(account_id)?;
        let cents = cents_from_usd(amount_usd)?;
        let pairs: [(&str, String); 11] = [
            ("mode", "payment".into()),
            ("line_items[0][quantity]", "1".into()),
            ("line_items[0][price_data][currency]", "usd".into()),
            (
                "line_items[0][price_data][product_data][name]",
                PRODUCT_NAME.into(),
            ),
            ("line_items[0][price_data][unit_amount]", cents.to_string()),
            ("client_reference_id", account_id.into()),
            ("metadata[account_id]", account_id.into()),
            ("payment_method_types[]", "card".into()),
            // Stablecoin (USDC) payments. The Stripe account must have the
            // "Stablecoins and crypto" payment method enabled.
            ("payment_method_types[]", "crypto".into()),
            ("success_url", success_url.into()),
            ("cancel_url", cancel_url.into()),
        ];
        Ok(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    /// Open a one-off payment Checkout Session crediting `account_id`.
    pub async fn create_checkout(
        &self,
        account_id: &str,
        amount_usd: f64,
        success_url: &str,
        cancel_url: &str,
    ) -> Result<CheckoutSession, StripeError> {
        let form = Self::checkout_form(account_id, amount_usd, success_url, cancel_url)?;
        let body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form.iter())
            .finish();
        let resp = self
            .http
            .post(format!("{}/v1/checkout/sessions", self.api_base))
            .bearer_auth(&self.secret_key)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .map_err(|e| StripeError::Transport(e.without_url().to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| StripeError::Transport(e.to_string()))?;
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| StripeError::BadResponse(format!("non-JSON body: {e}")))?;
        if !status.is_success() {
            let message = json["error"]["message"]
                .as_str()
                .unwrap_or("unknown error")
                .to_string();
            return Err(StripeError::Api {
                status: status.as_u16(),
                message,
            });
        }
        match (json["id"].as_str(), json["url"].as_str()) {
            (Some(id), Some(url)) => Ok(CheckoutSession {
                id: id.into(),
                url: url.into(),
            }),
            _ => Err(StripeError::BadResponse("missing id or url".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amount_bounds() {
        assert_eq!(cents_from_usd(5.0).unwrap(), 500);
        assert_eq!(cents_from_usd(5_000.0).unwrap(), 500_000);
        assert_eq!(cents_from_usd(12.345).unwrap(), 1235);
        for bad in [4.99, 5_000.01, 0.0, -10.0, f64::NAN, f64::INFINITY] {
            assert!(cents_from_usd(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn account_id_is_validated() {
        assert!(StripeClient::checkout_form("acct_1", 10.0, "s", "c").is_ok());
        for bad in ["", "a b", "x&metadata[evil]=1", &"a".repeat(129)] {
            assert!(StripeClient::checkout_form(bad, 10.0, "s", "c").is_err());
        }
    }

    #[test]
    fn debug_hides_secret() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let c = StripeClient::new("sk_test_hidden", "http://127.0.0.1:1/").unwrap();
        assert_eq!(c.api_base, "http://127.0.0.1:1");
        assert!(!format!("{c:?}").contains("sk_test_hidden"));
    }
}
