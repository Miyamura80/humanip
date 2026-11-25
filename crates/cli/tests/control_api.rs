//! Control API (`/api/v1/control`) and token-guarded checkout, driven
//! in-process with `oneshot` over a temp-file SQLite ledger.
//!
//! `appctl` is a binary crate, so the route modules are compiled in here by
//! path, the same way `serve_http.rs` includes them.

#![allow(dead_code)]

#[path = "../src/billing_routes.rs"]
mod billing_routes;
#[path = "../src/control_routes.rs"]
mod control_routes;

use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode};
use axum::{Json, Router};
use billing_routes::{BillingConfig, BillingState};
use control_routes::{ControlState, ControlToken};
use humanip_billing::sign_payload;
use humanip_core::accounts::{AccountStore, UsageEvent};
use humanip_ledger::SqliteAccounts;
use serde_json::{json, Value};
use tower::ServiceExt;

const TOKEN: &str = "ctl_test_token";
const WHSEC: &str = "whsec_control_test";

struct Harness {
    _dir: tempfile::TempDir,
    ledger: Arc<SqliteAccounts>,
    app: Router,
}

/// The production wiring: one ledger, shared by control and billing.
fn harness_with(token: Option<&str>, ledger: bool, billing: BillingConfig) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(SqliteAccounts::open(dir.path().join("accounts.db")).unwrap());
    let shared = ledger.then(|| db.clone());
    let token = ControlToken::new(token);
    let billing = BillingState::new(
        billing,
        billing_routes::billing_store(shared.clone()),
        token.clone(),
    );
    let app = Router::new()
        .nest(
            "/api/v1/control",
            control_routes::router(ControlState::new(token, shared)),
        )
        .nest("/api/v1/billing", billing_routes::router(billing));
    Harness {
        _dir: dir,
        ledger: db,
        app,
    }
}

fn harness() -> Harness {
    harness_with(Some(TOKEN), true, BillingConfig::default())
}

fn req(method: &str, uri: &str, body: Option<Value>, token: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header("authorization", format!("Bearer {t}"));
    }
    match body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    }
}

async fn call(app: &Router, r: Request<Body>) -> (StatusCode, Value) {
    let resp = app.clone().oneshot(r).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, v)
}

async fn ok(app: &Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (s, v) = call(app, req(method, uri, body, Some(TOKEN))).await;
    assert_eq!(s, StatusCode::OK, "{method} {uri}: {v}");
    v
}

async fn new_account(app: &Router, name: &str) -> String {
    let v = ok(
        app,
        "POST",
        "/api/v1/control/accounts",
        Some(json!({ "name": name })),
    )
    .await;
    v["account_id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn missing_or_wrong_token_is_401() {
    let h = harness();
    for token in [None, Some("nope")] {
        let (s, v) = call(
            &h.app,
            req("GET", "/api/v1/control/accounts/acct_x", None, token),
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert!(v["error"].is_string());
    }
}

#[tokio::test]
async fn unconfigured_token_or_ledger_is_503() {
    for (token, ledger) in [(None, true), (Some(""), true), (Some(TOKEN), false)] {
        let h = harness_with(token, ledger, BillingConfig::default());
        let body = Some(json!({ "name": "acme" }));
        let (s, v) = call(
            &h.app,
            req("POST", "/api/v1/control/accounts", body, Some(TOKEN)),
        )
        .await;
        assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{token:?} {ledger}");
        assert!(v["error"].is_string());
    }
    let h = harness_with(None, true, BillingConfig::default());
    let (_, v) = call(&h.app, req("GET", "/api/v1/control/accounts/a", None, None)).await;
    assert_eq!(v, json!({ "error": "control API not configured" }));
}

#[tokio::test]
async fn full_key_lifecycle() {
    let h = harness();
    let app = &h.app;
    let id = new_account(app, "acme").await;
    let base = format!("/api/v1/control/accounts/{id}");

    let acct = ok(app, "GET", &base, None).await;
    assert_eq!(
        acct,
        json!({ "account_id": id, "name": "acme", "balance_micro": 0 })
    );

    let keys = format!("{base}/keys");
    let issued = ok(
        app,
        "POST",
        &keys,
        Some(json!({ "name": "main", "monthly_cap_usd": null })),
    )
    .await;
    let key_id = issued["key"]["key_id"].as_str().unwrap().to_string();
    let secret = issued["secret"].as_str().unwrap().to_string();
    assert_eq!(issued["key"]["account_id"], id.as_str());
    assert!(issued["key"]["monthly_cap_micro"].is_null());
    assert_eq!(
        h.ledger.authenticate(&secret).unwrap().unwrap().key_id,
        key_id
    );

    let listed = ok(app, "GET", &keys, None).await;
    assert_eq!(
        listed,
        json!([{ "key_id": key_id, "name": "main", "kyc_tier": "none",
                 "monthly_cap_micro": null, "revoked": false }])
    );

    let key = format!("{keys}/{key_id}");
    let rotated = ok(app, "POST", &format!("{key}/rotate"), None).await;
    let new_secret = rotated["secret"].as_str().unwrap();
    assert_ne!(new_secret, secret);
    assert!(h.ledger.authenticate(&secret).unwrap().is_none());
    assert_eq!(
        h.ledger.authenticate(new_secret).unwrap().unwrap().key_id,
        key_id
    );

    let cap = format!("{key}/cap");
    let v = ok(app, "PUT", &cap, Some(json!({ "monthly_cap_usd": 12.5 }))).await;
    assert_eq!(v, json!({ "ok": true }));
    assert_eq!(
        h.ledger.get_key(&key_id).unwrap().monthly_cap_micro,
        Some(12_500_000)
    );
    ok(app, "PUT", &cap, Some(json!({ "monthly_cap_usd": null }))).await;
    assert_eq!(h.ledger.get_key(&key_id).unwrap().monthly_cap_micro, None);

    let v = ok(app, "POST", &format!("{key}/revoke"), None).await;
    assert_eq!(v, json!({ "ok": true }));
    assert_eq!(ok(app, "GET", &keys, None).await[0]["revoked"], true);
}

#[tokio::test]
async fn validation_errors_are_400() {
    let h = harness();
    let app = &h.app;
    let id = new_account(app, "acme").await;
    let keys = format!("/api/v1/control/accounts/{id}/keys");
    let long = "x".repeat(65);
    for body in [
        json!({ "name": "" }),
        json!({ "name": long }),
        json!({ "name": 5 }),
        json!({ "name": "k", "monthly_cap_usd": 0 }),
        json!({ "name": "k", "monthly_cap_usd": 100000.5 }),
        json!({ "name": "k", "monthly_cap_usd": -3 }),
    ] {
        let (s, v) = call(app, req("POST", &keys, Some(body.clone()), Some(TOKEN))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{body}");
        assert!(v["error"].is_string());
    }
    let ok64 = "y".repeat(64);
    ok(
        app,
        "POST",
        &keys,
        Some(json!({ "name": ok64, "monthly_cap_usd": 100000 })),
    )
    .await;
    let (s, _) = call(
        app,
        req(
            "POST",
            "/api/v1/control/accounts",
            Some(json!({})),
            Some(TOKEN),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let usage = format!("/api/v1/control/accounts/{id}/usage?since=soon");
    let (s, _) = call(app, req("GET", &usage, None, Some(TOKEN))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn other_accounts_keys_and_unknown_accounts_are_404() {
    let h = harness();
    let app = &h.app;
    let a = new_account(app, "a").await;
    let b = new_account(app, "b").await;
    let issued = ok(
        app,
        "POST",
        &format!("/api/v1/control/accounts/{a}/keys"),
        Some(json!({ "name": "k" })),
    )
    .await;
    let key_id = issued["key"]["key_id"].as_str().unwrap();
    let theirs = format!("/api/v1/control/accounts/{b}/keys/{key_id}");
    for (m, uri, body) in [
        ("POST", format!("{theirs}/rotate"), None),
        ("POST", format!("{theirs}/revoke"), None),
        (
            "PUT",
            format!("{theirs}/cap"),
            Some(json!({ "monthly_cap_usd": 5 })),
        ),
    ] {
        let (s, v) = call(app, req(m, &uri, body, Some(TOKEN))).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{m} {uri}");
        assert!(v["error"].is_string());
    }
    assert!(!h.ledger.get_key(key_id).unwrap().revoked, "untouched");
    for uri in ["", "/keys", "/usage"] {
        let uri = format!("/api/v1/control/accounts/acct_nobody{uri}");
        let (s, _) = call(app, req("GET", &uri, None, Some(TOKEN))).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{uri}");
    }
    let (s, _) = call(
        app,
        req(
            "POST",
            "/api/v1/control/accounts/acct_nobody/keys",
            Some(json!({ "name": "k" })),
            Some(TOKEN),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn usage_reflects_recorded_events() {
    let h = harness();
    let app = &h.app;
    let id = new_account(app, "acme").await;
    let issued = ok(
        app,
        "POST",
        &format!("/api/v1/control/accounts/{id}/keys"),
        Some(json!({ "name": "k" })),
    )
    .await;
    let key_id = issued["key"]["key_id"].as_str().unwrap().to_string();
    h.ledger.credit(&id, 10_000_000, "seed").unwrap();
    let ev = UsageEvent {
        key_id: key_id.clone(),
        provider: "p".into(),
        domain: "example.com".into(),
        bytes_up: 0,
        bytes_down: 500_000_000,
        provider_price_per_gb: 4.0,
        at_unix: 1_795_608_000,
    };
    h.ledger.record_usage(&ev).unwrap();
    let rows = ok(
        app,
        "GET",
        &format!("/api/v1/control/accounts/{id}/usage?since=0"),
        None,
    )
    .await;
    assert_eq!(
        rows,
        json!([{ "key_id": key_id, "provider": "p", "domain": "example.com",
                 "bytes": 500_000_000, "total_micro": 2_100_000 }])
    );
    let later = ok(
        app,
        "GET",
        &format!("/api/v1/control/accounts/{id}/usage?since=1795608001"),
        None,
    )
    .await;
    assert_eq!(later, json!([]));
    let acct = ok(app, "GET", &format!("/api/v1/control/accounts/{id}"), None).await;
    assert_eq!(acct["balance_micro"], 7_900_000);
}

/// Loopback mock of Stripe's `POST /v1/checkout/sessions`.
async fn mock_stripe() -> String {
    async fn mock(_body: Bytes) -> Json<Value> {
        Json(json!({ "id": "cs_mock", "url": "https://checkout.stripe.test/cs_mock" }))
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new().route("/v1/checkout/sessions", axum::routing::post(mock));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn checkout_requires_token_and_known_account() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cfg = BillingConfig {
        stripe_secret_key: Some("sk_test_mock".into()),
        public_url: Some("https://humanip.example".into()),
        api_base: mock_stripe().await,
        ..Default::default()
    };
    let h = harness_with(Some(TOKEN), true, cfg);
    let app = &h.app;
    let id = new_account(app, "acme").await;
    let body = |a: &str| Some(json!({ "account_id": a, "amount_usd": 10 }));
    let uri = "/api/v1/billing/checkout";
    let (s, _) = call(app, req("POST", uri, body(&id), None)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = call(app, req("POST", uri, body("acct_nobody"), Some(TOKEN))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, v) = call(app, req("POST", uri, body(&id), Some(TOKEN))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["id"], "cs_mock");
    assert!(v["url"].is_string());
}

#[tokio::test]
async fn webhook_credit_shows_in_account_balance() {
    let cfg = BillingConfig {
        webhook_secret: Some(WHSEC.into()),
        ..Default::default()
    };
    let h = harness_with(Some(TOKEN), true, cfg);
    let app = &h.app;
    let id = new_account(app, "acme").await;
    let event = json!({
        "id": "evt_1",
        "type": "checkout.session.completed",
        "data": { "object": {
            "id": "cs_paid", "payment_status": "paid", "amount_total": 2500,
            "currency": "usd", "client_reference_id": id, "metadata": { "account_id": id }
        }}
    })
    .to_string();
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let sig = format!("t={t},v1={}", sign_payload(event.as_bytes(), WHSEC, t));
    let r = Request::builder()
        .method("POST")
        .uri("/api/v1/billing/webhook")
        .header("stripe-signature", sig)
        .body(Body::from(event))
        .unwrap();
    let (s, v) = call(app, r).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let acct = ok(app, "GET", &format!("/api/v1/control/accounts/{id}"), None).await;
    assert_eq!(acct["balance_micro"], 25_000_000);
}

/// `product_routes` reads env and opens the ledger once; the only test here
/// that touches env, so no other test races it.
#[tokio::test]
async fn product_routes_share_one_ledger_from_env() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("env.db");
    std::env::set_var("HUMANIP_ACCOUNTS_DB", &path);
    std::env::set_var("HUMANIP_CONTROL_TOKEN", TOKEN);
    let app: Router = Router::new().nest("/api/v1", control_routes::product_routes());
    let id = new_account(&app, "env").await;
    let (s, _) = call(&app, req("POST", "/api/v1/billing/checkout", None, None)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let on_disk = SqliteAccounts::open(&path).unwrap();
    assert_eq!(on_disk.account(&id).unwrap().name, "env");
}
