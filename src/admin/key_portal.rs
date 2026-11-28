//! Self-service API-key portal: isolated read-only session and statistics.
//!
//! A key holder proves possession of the raw `exo_...` key once, then receives
//! a signed opaque cookie bound to that key id. No raw key is stored in the
//! cookie or returned in any response. Every request re-reads the key row, so
//! a disabled or removed key loses access immediately. Routes live outside the
//! admin middleware and never share the admin cookie name.

use super::cookie_policy::cookie_is_secure;
use super::errors::admin_rate_limited_response;
use super::*;
use crate::infra::storage::StorageError;
use crate::security::api_key_scope::ApiKeyScope;
use axum::{
    Router,
    body::Body,
    extract::{Query, Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;

const COOKIE_NAME: &str = "exoroute_key_session";
const COOKIE_PATH: &str = "/api/v1/key-portal";
const SESSION_TTL_SECONDS: u64 = 24 * 60 * 60;
const MAX_TOKEN_BYTES: usize = 2048;
const MAX_RECENT_REQUESTS: usize = 64;
const MAX_LOG_SCAN: usize = 512;
const SESSION_DOMAIN: &[u8] = b"exoroute-key-portal-v1";

#[derive(Debug, Deserialize)]
struct LoginRequest {
    api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StatsQuery {
    #[serde(default = "default_range")]
    range: String,
}

fn default_range() -> String {
    "7d".to_owned()
}

fn range_window(range: &str) -> Option<(&'static str, i64)> {
    match range {
        "24h" => Some(("1d", 24 * 60)),
        "7d" => Some(("7d", 7 * 24 * 60)),
        "30d" => Some(("30d", 30 * 24 * 60)),
        _ => None,
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionPayload {
    v: u8,
    k: String,
    e: u64,
    n: String,
    /// Short digest of the key's `token_hash`, so a recreated key id cannot
    /// reuse an older session.
    t: String,
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

/// Uses the installation secret: the master key when configured, otherwise a
/// domain-separated digest of the admin key. With neither configured the portal
/// fails closed instead of signing with a predictable fallback secret.
async fn signing_secret(state: &AppState) -> Result<[u8; 32], ()> {
    if let Some(master_key) = state.config.master_key {
        return Ok(master_key);
    }
    let admin_key = state.admin_key.read().await;
    let Some(material) = admin_key.as_deref() else {
        return Err(());
    };
    let mut hasher = Sha256::new();
    hasher.update(SESSION_DOMAIN);
    hasher.update(material.as_bytes());
    Ok(hasher.finalize().into())
}

fn mac(secret: &[u8; 32], payload: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(SESSION_DOMAIN);
    hasher.update(secret);
    hasher.update(payload.as_bytes());
    hasher.finalize().into()
}

fn encode_session(payload: &SessionPayload, secret: &[u8; 32]) -> String {
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(payload).unwrap_or_default());
    let signature = mac(secret, &encoded);
    format!(
        "{encoded}.{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature)
    )
}

fn decode_session(token: &str, secret: &[u8; 32]) -> Option<SessionPayload> {
    let (encoded, signature) = token.split_once('.')?;
    if token.len() > MAX_TOKEN_BYTES || encoded.is_empty() || signature.is_empty() {
        return None;
    }
    let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(signature)
        .ok()?;
    if signature.len() != 32 || !bool::from(mac(secret, encoded).ct_eq(&signature)) {
        return None;
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .ok()?;
    let payload: SessionPayload = serde_json::from_slice(&bytes).ok()?;
    if payload.v != 1
        || payload.k.is_empty()
        || payload.k.len() > 128
        || payload.e <= unix_seconds()
    {
        return None;
    }
    Some(payload)
}

fn no_store(response: &mut Response) {
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
}

fn token_version(record: &crate::infra::storage::Record) -> Result<String, StorageError> {
    let hash = record.bytes("token_hash")?;
    Ok(hex(&hash[..hash.len().min(8)]))
}

fn cookie_value(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .find_map(|value| {
            value.to_str().ok().and_then(|raw| {
                raw.split(';').find_map(|part| {
                    let (name, value) = part.trim().split_once('=')?;
                    (name == COOKIE_NAME).then_some(value)
                })
            })
        })
}

fn set_cookie(response: &mut Response, token: &str, secure: bool) {
    let suffix = if secure { "; Secure" } else { "" };
    let value = format!(
        "{COOKIE_NAME}={token}; Path={COOKIE_PATH}; Max-Age={SESSION_TTL_SECONDS}; HttpOnly; SameSite=Lax{suffix}"
    );
    response.headers_mut().append(
        axum::http::header::SET_COOKIE,
        value.parse().expect("valid cookie header"),
    );
}

fn clear_cookie(response: &mut Response, secure: bool) {
    let suffix = if secure { "; Secure" } else { "" };
    let value =
        format!("{COOKIE_NAME}=; Path={COOKIE_PATH}; Max-Age=0; HttpOnly; SameSite=Lax{suffix}");
    response.headers_mut().append(
        axum::http::header::SET_COOKIE,
        value.parse().expect("valid cookie header"),
    );
}

async fn key_record(
    state: &AppState,
    key_id: &str,
) -> Result<Option<crate::infra::storage::Record>, StorageError> {
    let key_id = key_id.to_owned();
    state
        .db
        .read(move |transaction| {
            let Some(record) = transaction.get::<crate::infra::storage::Record>(
                crate::infra::storage::Table::ApiKeys,
                &key_id,
            )?
            else {
                return Ok(None);
            };
            if record.boolean("enabled")? {
                Ok(Some(record))
            } else {
                Ok(None)
            }
        })
        .await
}

fn key_json(record: &crate::infra::storage::Record) -> Result<Value, StorageError> {
    let id = record.text("id")?;
    let scope = ApiKeyScope::from_record(record);
    let (allowed_providers, allowed_models) = match &scope {
        Ok(scope) => (
            json!(scope.allowed_provider_ids()),
            json!(scope.stored_model_rules()),
        ),
        Err(_) => (Value::Null, Value::Null),
    };
    Ok(json!({
        "id": id,
        "name": record.text("name")?,
        "prefix": format!("exo_{}", id.chars().take(8).collect::<String>()),
        "enabled": record.boolean("enabled")?,
        "allowed_providers": allowed_providers,
        "allowed_models": allowed_models,
        "created_at": record.text("created_at")?,
        "last_used_at": record.optional_text("last_used_at")?,
    }))
}

fn unauthorized() -> (StatusCode, Json<Value>) {
    fail(StatusCode::UNAUTHORIZED, "key portal session required")
}

fn missing_secret() -> (StatusCode, Json<Value>) {
    fail(
        StatusCode::SERVICE_UNAVAILABLE,
        "key portal is unavailable: no signing secret is configured",
    )
}

async fn session_payload(state: &AppState, headers: &HeaderMap) -> Option<SessionPayload> {
    let token = cookie_value(headers)?;
    let secret = signing_secret(state).await.ok()?;
    let payload = decode_session(token, &secret)?;
    if state.is_key_portal_session_revoked(&payload.n) {
        return None;
    }
    Some(payload)
}

pub(crate) async fn login(State(state): State<AppState>, mut request: Request<Body>) -> Response {
    let input: LoginRequest = match super::auth::parse_auth_json(&mut request).await {
        Ok(input) => input,
        Err(error) => return error.into_response(),
    };
    let peer_ip = crate::security::client_ip::client_ip(&request, state.config.trust_proxy_headers);
    let decision = state.admin.key_portal_login_limiter.check(&peer_ip);
    if !decision.allowed {
        return admin_rate_limited_response("key portal login rate limit exceeded", decision)
            .into_response();
    }
    let Some(token) = input
        .api_key
        .filter(|token| !token.is_empty() && token.len() <= 4096 && token.starts_with("exo_"))
    else {
        return unauthorized().into_response();
    };
    let candidate = crate::security::token_hash(&token);
    let lookup_hash = candidate.clone();
    let index = format!("token/{}", hex(&candidate));
    let key_id = match state
        .db
        .read(move |transaction| {
            let Some(key_id) = transaction
                .get::<String>(crate::infra::storage::Table::ApiKeyTokenIndex, &index)?
            else {
                return Ok(None);
            };
            let Some(record) = transaction.get::<crate::infra::storage::Record>(
                crate::infra::storage::Table::ApiKeys,
                &key_id,
            )?
            else {
                return Ok(None);
            };
            if !record.boolean("enabled")?
                || !crate::security::secure_eq(record.bytes("token_hash")?, &lookup_hash)
            {
                return Ok(None);
            }
            Ok(Some(key_id))
        })
        .await
    {
        Ok(Some(key_id)) => key_id,
        Ok(None) => return unauthorized().into_response(),
        Err(error) => return internal(error).into_response(),
    };
    let record = match key_record(&state, &key_id).await {
        Ok(Some(record)) => record,
        Ok(None) => return unauthorized().into_response(),
        Err(error) => return internal(error).into_response(),
    };
    let key = match key_json(&record) {
        Ok(key) => key,
        Err(error) => return internal(error).into_response(),
    };
    let version = match token_version(&record) {
        Ok(version) => version,
        Err(error) => return internal(error).into_response(),
    };
    let secret = match signing_secret(&state).await {
        Ok(secret) => secret,
        Err(()) => return missing_secret().into_response(),
    };
    let signed = encode_session(
        &SessionPayload {
            v: 1,
            k: key_id,
            e: unix_seconds().saturating_add(SESSION_TTL_SECONDS),
            n: uuid::Uuid::new_v4().simple().to_string(),
            t: version,
        },
        &secret,
    );
    let mut response = (
        StatusCode::OK,
        Json(json!({"authenticated": true, "key": key})),
    )
        .into_response();
    set_cookie(&mut response, &signed, cookie_is_secure(&state, &request));
    no_store(&mut response);
    response
}

pub(crate) async fn logout(State(state): State<AppState>, request: Request<Body>) -> Response {
    if let Some(payload) = session_payload(&state, request.headers()).await {
        state.revoke_key_portal_session(&payload.n, payload.e);
    }
    let mut response = (StatusCode::OK, Json(json!({"authenticated": false}))).into_response();
    clear_cookie(&mut response, cookie_is_secure(&state, &request));
    no_store(&mut response);
    response
}

pub(crate) async fn session(State(state): State<AppState>, request: Request<Body>) -> Response {
    let payload = match session_payload(&state, request.headers()).await {
        Some(payload) => payload,
        None => return unauthorized().into_response(),
    };
    let record = match key_record(&state, &payload.k).await {
        Ok(Some(record)) => record,
        Ok(None) => return unauthorized().into_response(),
        Err(error) => return internal(error).into_response(),
    };
    let version = match token_version(&record) {
        Ok(version) => version,
        Err(error) => return internal(error).into_response(),
    };
    if !crate::security::secure_eq(payload.t.as_bytes(), version.as_bytes()) {
        return unauthorized().into_response();
    }
    let key = match key_json(&record) {
        Ok(key) => key,
        Err(error) => return internal(error).into_response(),
    };
    let mut response = (
        StatusCode::OK,
        Json(json!({"authenticated": true, "key": key})),
    )
        .into_response();
    no_store(&mut response);
    response
}

async fn recent_requests(state: &AppState, key_id: &str) -> Result<Vec<Value>, StorageError> {
    let key_id = key_id.to_owned();
    state
        .db
        .read(move |transaction| {
            let rows = transaction.scan_prefix::<crate::infra::storage::Record>(
                crate::infra::storage::Table::RequestLogs,
                "",
                MAX_LOG_SCAN,
            )?;
            let mut requests = Vec::new();
            for (_, record) in rows {
                if record
                    .optional_text("api_key_id")?
                    .filter(|id| *id == key_id)
                    .is_none()
                {
                    continue;
                }
                requests.push(json!({
                    "id": record.text("id")?,
                    "timestamp": record.text("created_at")?,
                    "model": record.text("model")?,
                    "provider": record.optional_text("provider_id")?,
                    "status_code": record.integer("status")?,
                    "input_tokens": record.optional_integer("input_tokens")?.unwrap_or(0),
                    "output_tokens": record.optional_integer("output_tokens")?.unwrap_or(0),
                    "cached_tokens": record.optional_integer("cached_tokens")?.unwrap_or(0),
                    "latency_ms": record.integer("duration_ms")?,
                    "cost_micro_usd": record.optional_integer("cost_micro_usd")?,
                }));
                if requests.len() >= MAX_RECENT_REQUESTS {
                    break;
                }
            }
            Ok(requests)
        })
        .await
}

pub(crate) async fn stats(
    State(state): State<AppState>,
    Query(query): Query<StatsQuery>,
    request: Request<Body>,
) -> Response {
    let payload = match session_payload(&state, request.headers()).await {
        Some(payload) => payload,
        None => return unauthorized().into_response(),
    };
    let record = match key_record(&state, &payload.k).await {
        Ok(Some(record)) => record,
        Ok(None) => return unauthorized().into_response(),
        Err(error) => return internal(error).into_response(),
    };
    let version = match token_version(&record) {
        Ok(version) => version,
        Err(error) => return internal(error).into_response(),
    };
    if !crate::security::secure_eq(payload.t.as_bytes(), version.as_bytes()) {
        return unauthorized().into_response();
    }
    let key = match key_json(&record) {
        Ok(key) => key,
        Err(error) => return internal(error).into_response(),
    };
    let (range, minutes) = match range_window(&query.range) {
        Some(value) => value,
        None => {
            return fail(
                StatusCode::BAD_REQUEST,
                "Statistics range must be 24h, 7d, or 30d",
            )
            .into_response();
        }
    };
    let snapshot = match crate::infra::telemetry::api_key_statistics_snapshot(
        &state.db,
        &payload.k,
        range,
        minutes,
        &state.telemetry,
    )
    .await
    {
        Ok(snapshot) => snapshot,
        Err(error) => return internal(error).into_response(),
    };
    let number = |field: &str| -> i64 { snapshot.get(field).and_then(Value::as_i64).unwrap_or(0) };
    let input_tokens = number("input_tokens");
    let output_tokens = number("output_tokens");
    let cached_tokens = number("cached_tokens");
    let models = snapshot
        .get("model_breakdown")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    let average_latency_ms = row
                        .get("average_latency_ms")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                    json!({
                        "model": row.get("model").and_then(Value::as_str).unwrap_or(""),
                        "provider": row.get("provider").and_then(Value::as_str).unwrap_or(""),
                        "requests": row.get("requests").and_then(Value::as_i64).unwrap_or(0),
                        "failed": row.get("failures").and_then(Value::as_i64).unwrap_or(0),
                        "total_tokens": row.get("input_tokens").and_then(Value::as_i64).unwrap_or(0)
                            + row.get("output_tokens").and_then(Value::as_i64).unwrap_or(0)
                            + row.get("cached_tokens").and_then(Value::as_i64).unwrap_or(0),
                        "cost_micro_usd": row.get("cost_micro_usd").and_then(Value::as_i64).unwrap_or(0),
                        "avg_latency_ms": average_latency_ms,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let recent = match recent_requests(&state, &payload.k).await {
        Ok(recent) => recent,
        Err(error) => return internal(error).into_response(),
    };
    let mut response = (
        StatusCode::OK,
        Json(json!({
            "key": key,
            "summary": {
                "total_requests": number("total_requests"),
                "success_requests": number("successes"),
                "failed_requests": number("failures"),
                "success_rate": snapshot.get("success_rate").and_then(Value::as_f64).unwrap_or(0.0),
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "cached_tokens": cached_tokens,
                "total_tokens": input_tokens.saturating_add(output_tokens).saturating_add(cached_tokens),
                "total_cost_micro_usd": snapshot
                    .get("cost")
                    .and_then(|cost| cost.get("cost_micro_usd"))
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
                "avg_latency_ms": snapshot.get("average_latency_ms").and_then(Value::as_f64).unwrap_or(0.0),
            },
            "models": models,
            "recent_requests": recent,
        })),
    )
        .into_response();
    no_store(&mut response);
    response
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/key-portal/login", post(login))
        .route("/api/v1/key-portal/logout", post(logout))
        .route("/api/v1/key-portal/session", get(session))
        .route("/api/v1/key-portal/stats", get(stats))
}

#[cfg(test)]
#[path = "key_portal/tests.rs"]
mod tests;
