//! Billing routes for `appctl serve`, nested under `/api/v1/billing`.
//!
//! - `POST /checkout` `{account_id, amount_usd}` -> `{id, url}` (Stripe Checkout)
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
use humanip_core::accounts::{AccountStore, MemoryAccounts};
use serde::Deserialize;
use serde_json::json;

/// Gateway take rate on provider cost (5%).
const TAKE_RATE: f64 = 0.05;

/// INTEGRATION POINT: the store that webhook credits land in.
///
/// Until the ledger crate lands this is an in-memory store, so credits are
/// lost on restart. Replace the body with the SQLite ledger (shared with the
/// gateway) when `humanip-ledger` is ready.
pub fn billing_store() -> Arc<dyn AccountStore> {
    Arc::new(MemoryAccounts::new(TAKE_RATE))
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
    /// Built on first checkout so constructing the router never needs TLS.
    stripe: Arc<OnceLock<Result<StripeClient, String>>>,
}

impl BillingState {
    pub fn new(config: BillingConfig, store: Arc<dyn AccountStore>) -> Self {
        Self {
            config: Arc::new(config),
            store,
            stripe: Arc::new(OnceLock::new()),
        }
    }

    /// Env config plus [`billing_store`]. Used by `serve_http::build_app`.
    pub fn from_env() -> Self {
        Self::new(BillingConfig::from_env(), billing_store())
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
}

async fn checkout(State(st): State<BillingState>, body: Bytes) -> Response {
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
    let success = format!("{public}/billing/success?session_id={{CHECKOUT_SESSION_ID}}");
    let cancel = format!("{public}/billing/cancel");
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

    fn app(config: BillingConfig, store: Arc<dyn AccountStore>) -> Router {
        Router::new().nest("/api/v1/billing", router(BillingState::new(config, store)))
    }

    fn webhook_config() -> BillingConfig {
        BillingConfig {
            webhook_secret: Some(WHSEC.into()),
            api_base: "http://127.0.0.1:9".into(),
            ..Default::default()
        }
    }

    fn post(uri: &str, body: Vec<u8>, sig: Option<&str>) -> Request<Body> {
        let mut b = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(s) = sig {
            b = b.header("Stripe-Signature", s);
        }
        b.body(Body::from(body)).unwrap()
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
        let resp = app(BillingConfig::default(), billing_store())
            .oneshot(post(
                "/api/v1/billing/checkout",
                br#"{"account_id":"acct_1","amount_usd":10}"#.to_vec(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            json_body(resp).await["error"]["message"],
            "billing not configured"
        );
    }

    #[tokio::test]
    async fn webhook_503_when_unconfigured() {
        let resp = app(BillingConfig::default(), billing_store())
            .oneshot(post("/api/v1/billing/webhook", paid_event("cs_x"), None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn checkout_rejects_out_of_range_amount_before_any_call() {
        // api_base points at a closed loopback port: a request would fail with 502.
        let cfg = BillingConfig {
            stripe_secret_key: Some("sk_test_unused".into()),
            public_url: Some("https://humanip.example".into()),
            api_base: "http://127.0.0.1:9".into(),
            ..Default::default()
        };
        let _ = rustls::crypto::ring::default_provider().install_default();
        let resp = app(cfg, billing_store())
            .oneshot(post(
                "/api/v1/billing/checkout",
                br#"{"account_id":"acct_1","amount_usd":4.5}"#.to_vec(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn checkout_returns_session_from_mock_stripe() {
        // Loopback mock of POST /v1/checkout/sessions; echoes the success_url.
        async fn mock(body: Bytes) -> Json<Value> {
            let form = form_pairs(&body);
            Json(json!({ "id": "cs_mock", "url": form["success_url"] }))
        }
        fn form_pairs(body: &[u8]) -> std::collections::HashMap<String, String> {
            String::from_utf8_lossy(body)
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .map(|(k, v)| (k.to_string(), v.replace("%3A", ":").replace("%2F", "/")))
                .collect()
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mock_app = Router::new().route("/v1/checkout/sessions", axum::routing::post(mock));
        tokio::spawn(async move { axum::serve(listener, mock_app).await.unwrap() });

        let _ = rustls::crypto::ring::default_provider().install_default();
        let cfg = BillingConfig {
            stripe_secret_key: Some("sk_test_mock".into()),
            public_url: Some("https://humanip.example/".into()),
            api_base: format!("http://{addr}"),
            ..Default::default()
        };
        let resp = app(cfg, billing_store())
            .oneshot(post(
                "/api/v1/billing/checkout",
                br#"{"account_id":"acct_1","amount_usd":10}"#.to_vec(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        assert_eq!(body["id"], "cs_mock");
        assert!(body["url"]
            .as_str()
            .unwrap()
            .starts_with("https://humanip.example/billing/success"));
    }

    #[tokio::test]
    async fn webhook_400_on_bad_signature() {
        let store = billing_store();
        let body = paid_event("cs_bad");
        let t = now_unix();
        let sig = format!("t={t},v1={}", sign_payload(&body, "whsec_wrong", t));
        let resp = app(webhook_config(), store.clone())
            .oneshot(post("/api/v1/billing/webhook", body, Some(&sig)))
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
        let store = billing_store();
        let body = paid_event("cs_ok");
        let t = now_unix();
        let sig = format!("t={t},v1={}", sign_payload(&body, WHSEC, t));
        for expected in ["applied", "duplicate"] {
            let resp = app(webhook_config(), store.clone())
                .oneshot(post("/api/v1/billing/webhook", body.clone(), Some(&sig)))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            assert_eq!(json_body(resp).await["result"]["outcome"], expected);
        }
        assert_eq!(store.balance("acct_7").unwrap(), 20_000_000);
    }
}
