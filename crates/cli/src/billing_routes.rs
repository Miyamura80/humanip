//! Billing routes for `appctl serve`, nested under `/api/v1/billing`.
//!
//! - `POST /checkout` `{account_id, amount_usd}` -> `{id, url}` (Stripe Checkout).
//!   Needs the control token (`Authorization: Bearer <HUMANIP_CONTROL_TOKEN>`),
//!   so only the web service can open checkouts; 404 for an unknown account.
//! - `POST /webhook` raw Stripe event + `Stripe-Signature` -> 200 / 400
//!
//! Config comes from env (see `docs/billing.md`). Missing secrets give 503
//! "billing not configured" instead of failing at startup.

use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use humanip_billing::{
    handle_event, verify_webhook, EventError, StripeClient, StripeError, DEFAULT_API_BASE,
    DEFAULT_TOLERANCE_SECS,
};
use humanip_core::accounts::{AccountStore, MemoryAccounts, StoreError};
use humanip_ledger::SqliteAccounts;
use serde::Deserialize;
use serde_json::json;

use super::control_routes::{AuthError, ControlToken};

/// Gateway take rate on provider cost (5%).
const TAKE_RATE: f64 = 0.05;

/// The store that webhook credits land in: the shared SQLite ledger opened
/// once at startup from `HUMANIP_ACCOUNTS_DB` (the same file as the gateway's
/// `accounts_db`; WAL mode makes two processes safe). Without it, an in-memory
/// store is used and credits are lost on restart, which is only acceptable in
/// development.
pub fn billing_store(ledger: Option<Arc<SqliteAccounts>>) -> Arc<dyn AccountStore> {
    match ledger {
        Some(store) => store,
        None => {
            tracing::warn!(
                "no accounts ledger (HUMANIP_ACCOUNTS_DB); billing credits are kept in memory only"
            );
            Arc::new(MemoryAccounts::new(TAKE_RATE))
        }
    }
}

/// Billing settings, read from env by [`BillingConfig::from_env`].
#[derive(Clone, Default)]
pub struct BillingConfig {
    pub stripe_secret_key: Option<String>,
    pub webhook_secret: Option<String>,
    /// Base for success and cancel URLs, e.g. `https://humanip.example`.
    pub public_url: Option<String>,
    /// Stripe API base; defaults to `https://api.stripe.com`.
    pub api_base: String,
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

impl BillingConfig {
    pub fn from_env() -> Self {
        Self {
            stripe_secret_key: env_nonempty("STRIPE_SECRET_KEY"),
            webhook_secret: env_nonempty("STRIPE_WEBHOOK_SECRET"),
            public_url: env_nonempty("HUMANIP_PUBLIC_URL"),
            api_base: env_nonempty("STRIPE_API_BASE").unwrap_or_else(|| DEFAULT_API_BASE.into()),
        }
    }
}

#[derive(Clone)]
pub struct BillingState {
    config: Arc<BillingConfig>,
    store: Arc<dyn AccountStore>,
    /// Guards checkout; the webhook is authenticated by its Stripe signature.
    token: ControlToken,
    /// Built on first checkout so constructing the router never needs TLS.
    stripe: Arc<OnceLock<Result<StripeClient, String>>>,
}

impl BillingState {
    pub fn new(config: BillingConfig, store: Arc<dyn AccountStore>, token: ControlToken) -> Self {
        Self {
            config: Arc::new(config),
            store,
            token,
            stripe: Arc::new(OnceLock::new()),
        }
    }
}

/// The billing sub-router, ready to `nest` into any app.
pub fn router<S>(state: BillingState) -> Router<S> {
    Router::new()
        .route("/checkout", post(checkout))
        .route("/webhook", post(webhook))
        .with_state(state)
}

fn error(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    let body = json!({ "error": { "code": code, "message": message.into() } });
    (status, Json(body)).into_response()
}

fn not_configured() -> Response {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "not_configured",
        "billing not configured",
    )
}

#[derive(Deserialize)]
struct CheckoutReq {
    account_id: String,
    amount_usd: f64,
    /// Honoured only on `HUMANIP_PUBLIC_URL`'s origin; see [`return_url`].
    #[serde(default)]
    success_url: Option<String>,
    #[serde(default)]
    cancel_url: Option<String>,
}

/// `candidate` if it is under `public`'s origin, else `default` (no open redirects).
fn return_url(public: &str, candidate: Option<&str>, default: String) -> String {
    match candidate {
        Some(c) if c == public || c.starts_with(&format!("{public}/")) => c.to_string(),
        _ => default,
    }
}

async fn checkout(State(st): State<BillingState>, headers: HeaderMap, body: Bytes) -> Response {
    match st.token.check(&headers) {
        Ok(()) => {}
        Err(AuthError::NotConfigured) => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "not_configured",
                "control API not configured",
            )
        }
        Err(AuthError::Unauthorized) => {
            return error(StatusCode::UNAUTHORIZED, "unauthorized", "unauthorized")
        }
    }
    let cfg = &st.config;
    let (Some(key), Some(public)) = (&cfg.stripe_secret_key, &cfg.public_url) else {
        return not_configured();
    };
    let req: CheckoutReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_input",
                format!("invalid body: {e}"),
            )
        }
    };
    let store = st.store.clone();
    let account = req.account_id.clone();
    match tokio::task::spawn_blocking(move || store.balance(&account)).await {
        Ok(Ok(_)) => {}
        Ok(Err(StoreError::NotFound(_))) => {
            return error(StatusCode::NOT_FOUND, "not_found", "unknown account")
        }
        Ok(Err(e)) => {
            eprintln!("billing: store error: {e}");
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "store error",
            );
        }
        Err(e) => {
            eprintln!("billing: store task failed: {e}");
            return error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "internal error",
            );
        }
    }
    let client = st.stripe.get_or_init(|| {
        StripeClient::new(key.clone(), cfg.api_base.clone()).map_err(|e| e.to_string())
    });
    let client = match client {
        Ok(c) => c,
        Err(e) => {
            eprintln!("billing: cannot build Stripe client: {e}");
            return not_configured();
        }
    };
    let public = public.trim_end_matches('/');
    let success = return_url(
        public,
        req.success_url.as_deref(),
        format!("{public}/billing/success?session_id={{CHECKOUT_SESSION_ID}}"),
    );
    let cancel = return_url(
        public,
        req.cancel_url.as_deref(),
        format!("{public}/billing/cancel"),
    );
    match client
        .create_checkout(&req.account_id, req.amount_usd, &success, &cancel)
        .await
    {
        Ok(s) => Json(s).into_response(),
        Err(StripeError::InvalidInput(m)) => error(StatusCode::BAD_REQUEST, "invalid_input", m),
        Err(e) => {
            eprintln!("billing: checkout failed: {e}");
            error(
                StatusCode::BAD_GATEWAY,
                "network_error",
                "payment provider error",
            )
        }
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn webhook(State(st): State<BillingState>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(secret) = &st.config.webhook_secret else {
        return not_configured();
    };
    let sig = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if let Err(e) = verify_webhook(&body, sig, secret, now_unix(), DEFAULT_TOLERANCE_SECS) {
        return error(StatusCode::BAD_REQUEST, "bad_signature", e.to_string());
    }
    let store = st.store.clone();
    let result = tokio::task::spawn_blocking(move || handle_event(store.as_ref(), &body)).await;
    match result {
        Ok(Ok(outcome)) => Json(json!({ "received": true, "result": outcome })).into_response(),
        Ok(Err(EventError::Malformed(m))) => error(StatusCode::BAD_REQUEST, "invalid_input", m),
        // 500 makes Stripe retry; the credit is idempotent so retries are safe.
        Ok(Err(EventError::Store(e))) => {
            eprintln!("billing: store error: {e}");
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "store error",
            )
        }
        Err(e) => {
            eprintln!("billing: handler panicked: {e}");
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "internal error",
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use humanip_billing::sign_payload;
    use serde_json::Value;
    use tower::ServiceExt;

    const WHSEC: &str = "whsec_route_test";
    const TOKEN: &str = "ctl_route_test";

    fn app(config: BillingConfig, store: Arc<dyn AccountStore>) -> Router {
        let state = BillingState::new(config, store, ControlToken::new(Some(TOKEN)));
        Router::new().nest("/api/v1/billing", router(state))
    }

    fn memory() -> Arc<dyn AccountStore> {
        billing_store(None)
    }

    /// A ledger holding one account; returns its id.
    fn ledger_with_account() -> (Arc<dyn AccountStore>, String) {
        let db = SqliteAccounts::open_in_memory().unwrap();
        let id = db.create_account("acme").unwrap();
        (billing_store(Some(Arc::new(db))), id)
    }

    fn webhook_config() -> BillingConfig {
        BillingConfig {
            webhook_secret: Some(WHSEC.into()),
            api_base: "http://127.0.0.1:9".into(),
            ..Default::default()
        }
    }

    /// Checkout config whose Stripe base is a closed port: any call is a 502.
    fn dead_stripe_config() -> BillingConfig {
        BillingConfig {
            stripe_secret_key: Some("sk_test_unused".into()),
            public_url: Some("https://humanip.example".into()),
            api_base: "http://127.0.0.1:9".into(),
            ..Default::default()
        }
    }

    fn post(uri: &str, body: Vec<u8>, header: Option<(&str, &str)>) -> Request<Body> {
        let mut b = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        if let Some((k, v)) = header {
            b = b.header(k, v);
        }
        b.body(Body::from(body)).unwrap()
    }

    fn checkout_req(account: &str, amount: &str, token: Option<&str>) -> Request<Body> {
        let body = format!(r#"{{"account_id":"{account}","amount_usd":{amount}}}"#);
        let auth = token.map(|t| format!("Bearer {t}"));
        post(
            "/api/v1/billing/checkout",
            body.into_bytes(),
            auth.as_deref().map(|a| ("Authorization", a)),
        )
    }

    async fn json_body(resp: Response) -> Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn paid_event(session: &str) -> Vec<u8> {
        json!({
            "id": "evt_1",
            "type": "checkout.session.completed",
            "data": { "object": {
                "id": session,
                "payment_status": "paid",
                "amount_total": 2000,
                "currency": "usd",
                "client_reference_id": "acct_7",
                "metadata": { "account_id": "acct_7" }
            }}
        })
        .to_string()
        .into_bytes()
    }

    #[tokio::test]
    async fn checkout_503_when_unconfigured() {
        let resp = app(BillingConfig::default(), memory())
            .oneshot(checkout_req("acct_1", "10", Some(TOKEN)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            json_body(resp).await["error"]["message"],
            "billing not configured"
        );
    }

    #[tokio::test]
    async fn checkout_needs_the_control_token() {
        for token in [None, Some("wrong")] {
            let resp = app(dead_stripe_config(), memory())
                .oneshot(checkout_req("acct_1", "10", token))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{token:?}");
        }
        let unset = BillingState::new(dead_stripe_config(), memory(), ControlToken::new(None));
        let resp = Router::new()
            .nest("/api/v1/billing", router(unset))
            .oneshot(checkout_req("acct_1", "10", Some(TOKEN)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn checkout_404_for_unknown_account() {
        let (store, _) = ledger_with_account();
        let resp = app(dead_stripe_config(), store)
            .oneshot(checkout_req("acct_nobody", "10", Some(TOKEN)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn webhook_503_when_unconfigured() {
        let resp = app(BillingConfig::default(), memory())
            .oneshot(post("/api/v1/billing/webhook", paid_event("cs_x"), None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn checkout_rejects_out_of_range_amount_before_any_call() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (store, id) = ledger_with_account();
        let resp = app(dead_stripe_config(), store)
            .oneshot(checkout_req(&id, "4.5", Some(TOKEN)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn webhook_400_on_bad_signature() {
        let store = memory();
        let body = paid_event("cs_bad");
        let t = now_unix();
        let sig = format!("t={t},v1={}", sign_payload(&body, "whsec_wrong", t));
        let resp = app(webhook_config(), store.clone())
            .oneshot(post(
                "/api/v1/billing/webhook",
                body,
                Some(("Stripe-Signature", &sig)),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(store.balance("acct_7").is_err(), "nothing credited");

        let resp = app(webhook_config(), store)
            .oneshot(post("/api/v1/billing/webhook", paid_event("cs_bad"), None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn webhook_200_credits_once() {
        let store = memory();
        let body = paid_event("cs_ok");
        let t = now_unix();
        let sig = format!("t={t},v1={}", sign_payload(&body, WHSEC, t));
        for expected in ["applied", "duplicate"] {
            let resp = app(webhook_config(), store.clone())
                .oneshot(post(
                    "/api/v1/billing/webhook",
                    body.clone(),
                    Some(("Stripe-Signature", &sig)),
                ))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            assert_eq!(json_body(resp).await["result"]["outcome"], expected);
        }
        assert_eq!(store.balance("acct_7").unwrap(), 20_000_000);
    }
}

#[cfg(test)]
mod return_url_tests {
    use super::return_url;

    #[test]
    fn only_same_origin_return_urls_are_honoured() {
        let p = "http://localhost:1420";
        let d = || "DEFAULT".to_string();
        assert_eq!(
            return_url(
                p,
                Some("http://localhost:1420/#/account?topup=success"),
                d()
            ),
            "http://localhost:1420/#/account?topup=success"
        );
        for bad in [
            "https://evil.example/phish",
            "http://localhost:14200/x",
            "http://localhost:1420.evil.example/",
        ] {
            assert_eq!(return_url(p, Some(bad), d()), "DEFAULT", "{bad}");
        }
        assert_eq!(return_url(p, None, d()), "DEFAULT");
    }
}
