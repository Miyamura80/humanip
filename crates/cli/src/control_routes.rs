//! Service-to-service control API for `appctl serve`, nested under
//! `/api/v1/control` (contract: `docs/product-api.md` section 1).
//!
//! Every route needs `Authorization: Bearer <HUMANIP_CONTROL_TOKEN>`. With the
//! token unset (or empty), or no ledger at `HUMANIP_ACCOUNTS_DB`, every route
//! answers 503. Errors are `{"error": "..."}`. A key that belongs to another
//! account is a 404, the same as an unknown key.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use humanip_core::accounts::{AccountStore, KeyRecord, KycTier, MicroUsd, StoreError};
use humanip_ledger::{cli::parse_usd, hash_secret, SqliteAccounts};
use serde_json::{json, Value};

/// Longest accepted account or key name, in characters.
const MAX_NAME_CHARS: usize = 64;
/// Largest accepted monthly cap, in micro-USD ($100,000).
const MAX_CAP_MICRO: MicroUsd = 100_000 * 1_000_000;

/// Open the ledger at `HUMANIP_ACCOUNTS_DB` once, for both the control API and
/// billing. `None` when the variable is unset or the file cannot be opened.
pub fn ledger_from_env() -> Option<Arc<SqliteAccounts>> {
    let path = std::env::var("HUMANIP_ACCOUNTS_DB")
        .ok()
        .filter(|p| !p.trim().is_empty())?;
    match SqliteAccounts::open(&path) {
        Ok(store) => Some(Arc::new(store)),
        Err(e) => {
            tracing::error!(%path, error = %e, "cannot open HUMANIP_ACCOUNTS_DB");
            None
        }
    }
}

/// The product routes (`/control`, `/billing`) over one shared ledger: the
/// control API uses the concrete `SqliteAccounts` for its admin methods and
/// billing sees the same `Arc` as `dyn AccountStore`, so there is one
/// connection. Env: `HUMANIP_ACCOUNTS_DB`, `HUMANIP_CONTROL_TOKEN`, plus the
/// billing variables in `docs/billing.md`.
pub fn product_routes<S: Clone + Send + Sync + 'static>() -> Router<S> {
    let ledger = ledger_from_env();
    let token = ControlToken::from_env();
    if !token.is_configured() {
        tracing::warn!("HUMANIP_CONTROL_TOKEN not set; control API and checkout answer 503");
    }
    let billing = super::billing_routes::BillingState::new(
        super::billing_routes::BillingConfig::from_env(),
        super::billing_routes::billing_store(ledger.clone()),
        token.clone(),
    );
    Router::new()
        .nest("/control", router(ControlState::new(token, ledger)))
        .nest("/billing", super::billing_routes::router(billing))
}

/// Why a request failed the control-token check.
#[derive(Debug, PartialEq)]
pub enum AuthError {
    /// `HUMANIP_CONTROL_TOKEN` is unset or empty: 503.
    NotConfigured,
    /// Missing or wrong bearer token: 401.
    Unauthorized,
}

/// The shared service token. Holds only its SHA-256, so the comparison is
/// constant time and does not leak the token length.
#[derive(Clone, Default)]
pub struct ControlToken(Option<[u8; 32]>);

impl ControlToken {
    pub fn new(token: Option<&str>) -> Self {
        Self(token.filter(|t| !t.trim().is_empty()).map(hash_secret))
    }

    pub fn from_env() -> Self {
        Self::new(std::env::var("HUMANIP_CONTROL_TOKEN").ok().as_deref())
    }

    pub fn is_configured(&self) -> bool {
        self.0.is_some()
    }

    /// Check `Authorization: Bearer <token>`.
    pub fn check(&self, headers: &HeaderMap) -> Result<(), AuthError> {
        let want = self.0.ok_or(AuthError::NotConfigured)?;
        let presented = headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split_once(' '))
            .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
            .map(|(_, t)| t.trim())
            .ok_or(AuthError::Unauthorized)?;
        let got = hash_secret(presented);
        let diff = want
            .iter()
            .zip(got.iter())
            .fold(0u8, |a, (x, y)| a | (x ^ y));
        if std::hint::black_box(diff) == 0 {
            Ok(())
        } else {
            Err(AuthError::Unauthorized)
        }
    }
}

#[derive(Clone)]
pub struct ControlState {
    token: ControlToken,
    ledger: Option<Arc<SqliteAccounts>>,
}

impl ControlState {
    pub fn new(token: ControlToken, ledger: Option<Arc<SqliteAccounts>>) -> Self {
        Self { token, ledger }
    }
}

/// The control sub-router, ready to `nest` at `/api/v1/control`.
pub fn router<S>(state: ControlState) -> Router<S> {
    Router::new()
        .route("/accounts", post(create_account))
        .route("/accounts/:account_id", get(get_account))
        .route("/accounts/:account_id/keys", get(list_keys).post(issue_key))
        .route("/accounts/:account_id/keys/:key_id/rotate", post(rotate))
        .route("/accounts/:account_id/keys/:key_id/revoke", post(revoke))
        .route("/accounts/:account_id/keys/:key_id/cap", put(set_cap))
        .route("/accounts/:account_id/usage", get(usage))
        .with_state(state)
}

/// A control API error, rendered as `{"error": "..."}`.
#[derive(Debug)]
pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

fn err(status: StatusCode, message: impl Into<String>) -> ApiError {
    ApiError(status, message.into())
}

fn bad_request(message: impl Into<String>) -> ApiError {
    err(StatusCode::BAD_REQUEST, message)
}

fn store_error(e: StoreError) -> ApiError {
    match e {
        StoreError::NotFound(_) => err(StatusCode::NOT_FOUND, "not found"),
        StoreError::Storage(m) => {
            tracing::error!(error = %m, "control: ledger error");
            err(StatusCode::INTERNAL_SERVER_ERROR, "store error")
        }
    }
}

/// Auth, then the ledger. The single gate every handler goes through.
fn gate(st: &ControlState, headers: &HeaderMap) -> Result<Arc<SqliteAccounts>, ApiError> {
    match st.token.check(headers) {
        Ok(()) => {}
        Err(AuthError::NotConfigured) => {
            return Err(err(
                StatusCode::SERVICE_UNAVAILABLE,
                "control API not configured",
            ))
        }
        Err(AuthError::Unauthorized) => return Err(err(StatusCode::UNAUTHORIZED, "unauthorized")),
    }
    st.ledger.clone().ok_or_else(|| {
        err(
            StatusCode::SERVICE_UNAVAILABLE,
            "accounts store not configured",
        )
    })
}

/// Run a blocking ledger call off the async runtime.
async fn blocking<T, F>(ledger: Arc<SqliteAccounts>, f: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&SqliteAccounts) -> Result<T, StoreError> + Send + 'static,
{
    match tokio::task::spawn_blocking(move || f(&ledger)).await {
        Ok(r) => r.map_err(store_error),
        Err(e) => {
            tracing::error!(error = %e, "control: ledger task failed");
            Err(err(StatusCode::INTERNAL_SERVER_ERROR, "internal error"))
        }
    }
}

fn parse_body(body: &Bytes) -> Result<serde_json::Map<String, Value>, ApiError> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => Err(bad_request("body must be a JSON object")),
        Err(e) => Err(bad_request(format!("invalid JSON body: {e}"))),
    }
}

/// A trimmed name of 1 to 64 characters.
fn name_field(body: &serde_json::Map<String, Value>) -> Result<String, ApiError> {
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .ok_or_else(|| bad_request("name must be a string"))?;
    let n = name.chars().count();
    if n == 0 || n > MAX_NAME_CHARS {
        return Err(bad_request("name must be 1 to 64 characters"));
    }
    Ok(name.to_string())
}

/// `monthly_cap_usd`: a number with 0 < usd <= 100000, or null. Converted to
/// micro-USD exactly from its decimal text, never through float arithmetic.
/// A missing field is null unless `required`.
pub fn cap_field(
    body: &serde_json::Map<String, Value>,
    required: bool,
) -> Result<Option<MicroUsd>, ApiError> {
    let n = match body.get("monthly_cap_usd") {
        None if required => return Err(bad_request("monthly_cap_usd is required")),
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Number(n)) => n,
        Some(_) => return Err(bad_request("monthly_cap_usd must be a number or null")),
    };
    let micro = parse_usd(&n.to_string()).map_err(|_| {
        bad_request("monthly_cap_usd must be a positive amount, at most 6 decimals")
    })?;
    if micro <= 0 || micro > MAX_CAP_MICRO {
        return Err(bad_request("monthly_cap_usd must be > 0 and <= 100000"));
    }
    Ok(Some(micro))
}

/// The key, if it exists and belongs to `account_id`; otherwise `NotFound`.
fn owned_key(db: &SqliteAccounts, account_id: &str, key_id: &str) -> Result<KeyRecord, StoreError> {
    match db.get_key(key_id) {
        Ok(k) if k.account_id == account_id => Ok(k),
        Ok(_) => Err(StoreError::NotFound(key_id.into())),
        Err(e) => Err(e),
    }
}

type Resp = Result<Response, ApiError>;

async fn create_account(State(st): State<ControlState>, headers: HeaderMap, body: Bytes) -> Resp {
    let db = gate(&st, &headers)?;
    let name = name_field(&parse_body(&body)?)?;
    let id = blocking(db, move |db| db.create_account(&name)).await?;
    Ok(Json(json!({ "account_id": id })).into_response())
}

async fn get_account(
    State(st): State<ControlState>,
    headers: HeaderMap,
    Path(account_id): Path<String>,
) -> Resp {
    let db = gate(&st, &headers)?;
    let a = blocking(db, move |db| db.account(&account_id)).await?;
    Ok(Json(json!({
        "account_id": a.account_id,
        "name": a.name,
        "balance_micro": a.balance_micro,
    }))
    .into_response())
}

async fn list_keys(
    State(st): State<ControlState>,
    headers: HeaderMap,
    Path(account_id): Path<String>,
) -> Resp {
    let db = gate(&st, &headers)?;
    let keys = blocking(db, move |db| db.list_keys(&account_id)).await?;
    let rows: Vec<Value> = keys
        .into_iter()
        .map(|k| {
            json!({
                "key_id": k.key_id,
                "name": k.name,
                "kyc_tier": k.kyc_tier,
                "monthly_cap_micro": k.monthly_cap_micro,
                "revoked": k.revoked,
            })
        })
        .collect();
    Ok(Json(rows).into_response())
}

async fn issue_key(
    State(st): State<ControlState>,
    headers: HeaderMap,
    Path(account_id): Path<String>,
    body: Bytes,
) -> Resp {
    let db = gate(&st, &headers)?;
    let body = parse_body(&body)?;
    let name = name_field(&body)?;
    let cap = cap_field(&body, false)?;
    let (key, secret) = blocking(db, move |db| {
        db.issue_key(&account_id, &name, KycTier::None, cap)
    })
    .await?;
    Ok(Json(json!({ "key": key, "secret": secret })).into_response())
}

async fn rotate(
    State(st): State<ControlState>,
    headers: HeaderMap,
    Path((account_id, key_id)): Path<(String, String)>,
) -> Resp {
    let db = gate(&st, &headers)?;
    let secret = blocking(db, move |db| {
        owned_key(db, &account_id, &key_id)?;
        db.rotate_key(&key_id)
    })
    .await?;
    Ok(Json(json!({ "secret": secret })).into_response())
}

async fn revoke(
    State(st): State<ControlState>,
    headers: HeaderMap,
    Path((account_id, key_id)): Path<(String, String)>,
) -> Resp {
    let db = gate(&st, &headers)?;
    blocking(db, move |db| {
        owned_key(db, &account_id, &key_id)?;
        db.revoke_key(&key_id)
    })
    .await?;
    Ok(Json(json!({ "ok": true })).into_response())
}

async fn set_cap(
    State(st): State<ControlState>,
    headers: HeaderMap,
    Path((account_id, key_id)): Path<(String, String)>,
    body: Bytes,
) -> Resp {
    let db = gate(&st, &headers)?;
    let cap = cap_field(&parse_body(&body)?, true)?;
    blocking(db, move |db| {
        owned_key(db, &account_id, &key_id)?;
        db.set_cap(&key_id, cap)
    })
    .await?;
    Ok(Json(json!({ "ok": true })).into_response())
}

async fn usage(
    State(st): State<ControlState>,
    headers: HeaderMap,
    Path(account_id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Resp {
    let db = gate(&st, &headers)?;
    let since = match q.get("since") {
        None => 0,
        Some(s) => s
            .parse::<i64>()
            .map_err(|_| bad_request("since must be a unix timestamp in seconds"))?,
    };
    let rows = blocking(db, move |db| {
        db.account(&account_id)?;
        db.usage_since(Some(&account_id), since)
    })
    .await?;
    Ok(Json(rows).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn bearer(v: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, HeaderValue::from_str(v).unwrap());
        h
    }

    #[test]
    fn token_check_needs_configured_matching_bearer() {
        assert_eq!(
            ControlToken::new(None).check(&bearer("Bearer x")),
            Err(AuthError::NotConfigured)
        );
        assert_eq!(
            ControlToken::new(Some("  ")).check(&bearer("Bearer  ")),
            Err(AuthError::NotConfigured)
        );
        let t = ControlToken::new(Some("s3cret"));
        assert_eq!(t.check(&bearer("Bearer s3cret")), Ok(()));
        assert_eq!(t.check(&bearer("bearer s3cret")), Ok(()));
        assert_eq!(
            t.check(&bearer("Bearer s3cre")),
            Err(AuthError::Unauthorized)
        );
        assert_eq!(
            t.check(&bearer("Basic s3cret")),
            Err(AuthError::Unauthorized)
        );
        assert_eq!(t.check(&HeaderMap::new()), Err(AuthError::Unauthorized));
    }

    #[test]
    fn caps_convert_exactly_and_respect_bounds() {
        let cap = |v: Value| {
            let m = json!({ "monthly_cap_usd": v }).as_object().unwrap().clone();
            cap_field(&m, true).ok()
        };
        assert_eq!(cap(json!(12.5)), Some(Some(12_500_000)));
        assert_eq!(cap(json!(0.1)), Some(Some(100_000)));
        assert_eq!(cap(json!(100000)), Some(Some(MAX_CAP_MICRO)));
        assert_eq!(cap(json!(12.345678)), Some(Some(12_345_678)));
        assert_eq!(cap(Value::Null), Some(None));
        for bad in [
            json!(0),
            json!(-1),
            json!(100000.01),
            json!(1.2345678),
            json!("5"),
        ] {
            assert_eq!(cap(bad.clone()), None, "{bad}");
        }
        assert!(cap_field(&serde_json::Map::new(), true).is_err());
        assert_eq!(cap_field(&serde_json::Map::new(), false).ok(), Some(None));
    }
}
