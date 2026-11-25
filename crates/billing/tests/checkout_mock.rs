//! `create_checkout` against a loopback mock of the Stripe API. No network.

use std::sync::{Arc, Mutex};

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use humanip_billing::{StripeClient, StripeError};
use serde_json::{json, Value};

#[derive(Clone, Default)]
struct Captured {
    reqs: Arc<Mutex<Vec<(HeaderMap, String)>>>,
}

async fn mock_sessions(
    State(c): State<Captured>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let text = String::from_utf8(body.to_vec()).unwrap();
    c.reqs.lock().unwrap().push((headers, text));
    (
        StatusCode::OK,
        Json(json!({
            "id": "cs_test_123",
            "object": "checkout.session",
            "url": "https://checkout.stripe.com/c/pay/cs_test_123"
        })),
    )
}

async fn mock_error() -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": { "message": "The payment method type \"crypto\" is invalid." } })),
    )
}

async fn spawn(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}")
}

fn client(base: &str) -> StripeClient {
    let _ = rustls::crypto::ring::default_provider().install_default();
    StripeClient::new("sk_test_mock", base).unwrap()
}

#[tokio::test]
async fn create_checkout_sends_exact_form_fields() {
    let cap = Captured::default();
    let base = spawn(
        Router::new()
            .route("/v1/checkout/sessions", post(mock_sessions))
            .with_state(cap.clone()),
    )
    .await;

    let s = client(&base)
        .create_checkout(
            "acct_42",
            25.0,
            "https://humanip.example/billing/success?session_id={CHECKOUT_SESSION_ID}",
            "https://humanip.example/billing/cancel",
        )
        .await
        .unwrap();
    assert_eq!(s.id, "cs_test_123");
    assert_eq!(s.url, "https://checkout.stripe.com/c/pay/cs_test_123");

    let reqs = cap.reqs.lock().unwrap();
    assert_eq!(reqs.len(), 1);
    let (headers, body) = &reqs[0];
    assert_eq!(headers["authorization"], "Bearer sk_test_mock");
    assert_eq!(headers["content-type"], "application/x-www-form-urlencoded");
    let fields: Vec<(String, String)> = form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect();
    let expected: Vec<(String, String)> = [
        ("mode", "payment"),
        ("line_items[0][quantity]", "1"),
        ("line_items[0][price_data][currency]", "usd"),
        (
            "line_items[0][price_data][product_data][name]",
            "humanip credit",
        ),
        ("line_items[0][price_data][unit_amount]", "2500"),
        ("client_reference_id", "acct_42"),
        ("metadata[account_id]", "acct_42"),
        ("payment_method_types[]", "card"),
        ("payment_method_types[]", "crypto"),
        (
            "success_url",
            "https://humanip.example/billing/success?session_id={CHECKOUT_SESSION_ID}",
        ),
        ("cancel_url", "https://humanip.example/billing/cancel"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    assert_eq!(fields, expected);
}

#[tokio::test]
async fn out_of_range_amount_never_reaches_stripe() {
    let cap = Captured::default();
    let base = spawn(
        Router::new()
            .route("/v1/checkout/sessions", post(mock_sessions))
            .with_state(cap.clone()),
    )
    .await;
    let c = client(&base);
    for amt in [4.0, 5_001.0] {
        let err = c
            .create_checkout("acct_1", amt, "s", "c")
            .await
            .unwrap_err();
        assert!(matches!(err, StripeError::InvalidInput(_)));
    }
    assert!(cap.reqs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn stripe_error_is_surfaced() {
    let base = spawn(Router::new().route("/v1/checkout/sessions", post(mock_error))).await;
    let err = client(&base)
        .create_checkout("acct_1", 10.0, "s", "c")
        .await
        .unwrap_err();
    match err {
        StripeError::Api { status, message } => {
            assert_eq!(status, 400);
            assert!(message.contains("crypto"));
        }
        other => panic!("unexpected {other:?}"),
    }
}
