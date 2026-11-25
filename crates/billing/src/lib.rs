//! Stripe top-ups and webhook crediting for humanip.
//!
//! Flow: a customer asks for a top-up, [`StripeClient::create_checkout`]
//! opens a Stripe Checkout Session (card or USDC), Stripe calls our webhook,
//! [`verify_webhook`] checks the `Stripe-Signature` header, and
//! [`handle_event`] credits the prepaid balance through
//! [`AccountStore::credit`](humanip_core::accounts::AccountStore::credit),
//! idempotent on the Checkout Session id.
//!
//! No transport code lives here: `appctl serve` mounts the HTTP routes.

mod events;
mod stripe;
mod webhook;

pub use events::{handle_event, EventError, Outcome};
pub use stripe::{
    cents_from_usd, CheckoutSession, StripeClient, StripeError, DEFAULT_API_BASE, MAX_TOPUP_USD,
    MIN_TOPUP_USD, PRODUCT_NAME,
};
pub use webhook::{sign_payload, verify_webhook, WebhookError, DEFAULT_TOLERANCE_SECS};

/// Micro-USD per Stripe cent (1 cent = 10,000 micro-USD).
pub const MICROS_PER_CENT: i64 = 10_000;
