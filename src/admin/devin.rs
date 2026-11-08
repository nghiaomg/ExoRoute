//! Devin cloud-agent integration: encrypted API-key storage plus a narrow
//! proxy to the official `https://api.devin.ai/v1` session API.
//!
//! The Devin API is a task-lifecycle REST surface (create session, poll
//! status, send a message), not a model-routing protocol, so this module is
//! a dashboard integration — it never touches the gateway routing path.
//! The key is stored encrypted with the installation master key and is
//! never returned to the client or logged.

use super::*;
use crate::infra::db::devin_store;
use crate::security::encrypt_secret;
use http::Method;
use std::collections::HashMap;

const DEVIN_API_BASE_URL: &str = "https://api.devin.ai/v1";
const DEVIN_RESPONSE_LIMIT_BYTES: usize = 512 * 1024;
const DEVIN_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DEVIN_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_API_KEY_CHARS: usize = 512;
const MAX_PROMPT_CHARS: usize = 100_000;
const MAX_MESSAGE_CHARS: usize = 100_000;
const MAX_SESSION_ID_CHARS: usize = 128;
const MAX_LIST_LIMIT: u64 = 100;
const DEFAULT_LIST_LIMIT: u64 = 20;

pub(super) async fn devin_settings(State(state): State<AppState>) -> ApiResult {
    let configured = stored_api_key(&state).await?.is_some();
    Ok(Json(json!({
        "configured": configured,
        "master_key_configured": state.config.master_key.is_some(),
    })))
}

pub(super) async fn save_devin_settings(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> ApiResult {
    let Some(master_key) = state.config.master_key.as_ref() else {
        return Err(fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "Set EXOROUTE_MASTER_KEY before storing the Devin API key.",
        ));
    };
    let api_key = body
        .get("api_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if api_key.is_empty() {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "Provide the Devin API key to store.",
        ));
    }
    if api_key.chars().count() > MAX_API_KEY_CHARS
        || api_key.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "The Devin API key is too long or contains control characters.",
        ));
    }
    let encrypted = encrypt_secret(Some(master_key), api_key)
        .map_err(|error| fail(StatusCode::SERVICE_UNAVAILABLE, error))?;
    devin_store::save_devin_api_key_blob(&state.db, encrypted)
        .await
        .map_err(internal)?;
    Ok(Json(json!({"configured": true})))
}

pub(super) async fn delete_devin_settings(State(state): State<AppState>) -> ApiResult {
    devin_store::save_devin_api_key_blob(&state.db, None)
        .await
        .map_err(internal)?;
    Ok(Json(json!({"configured": false})))
}

/// Live key check: one bounded list-sessions call against the real API.
pub(super) async fn verify_devin(State(state): State<AppState>) -> ApiResult {
    let api_key = require_api_key(&state).await?;
    let url = format!("{DEVIN_API_BASE_URL}/sessions?limit=1");
    match devin_request(&state, &api_key, Method::GET, &url, None).await {
        Ok((status, body)) => Ok(Json(json!({
            "verified": status.is_success(),
            "status": status.as_u16(),
            "detail": body.get("error").cloned().unwrap_or(Value::Null),
        }))),
        Err((_, body)) => {
            let detail = body
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("the Devin API request failed")
                .to_owned();
            Ok(Json(json!({
                "verified": false,
                "status": Value::Null,
                "detail": detail,
            })))
        }
    }
}

pub(super) async fn list_devin_sessions(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult {
    let api_key = require_api_key(&state).await?;
    let limit = params
        .get("limit")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .min(MAX_LIST_LIMIT);
    let offset = params
        .get("offset")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let url = format!("{DEVIN_API_BASE_URL}/sessions?limit={limit}&offset={offset}");
    let (status, body) = devin_request(&state, &api_key, Method::GET, &url, None).await?;
    upstream_json(status, body)
}

pub(super) async fn create_devin_session(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> ApiResult {
    let api_key = require_api_key(&state).await?;
    let prompt = text_field(&body, "prompt", MAX_PROMPT_CHARS)?;
    let mut payload = json!({"prompt": prompt});
    if let Some(repository) = optional_https_url(&body, "repository")? {
        payload["repository_url"] = Value::String(repository);
    }
    if let Some(branch) = optional_text(&body, "branch", 256)? {
        payload["branch"] = Value::String(branch);
    }
    let url = format!("{DEVIN_API_BASE_URL}/sessions");
    let (status, body) = devin_request(&state, &api_key, Method::POST, &url, Some(payload)).await?;
    upstream_json(status, body)
}

pub(super) async fn get_devin_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> ApiResult {
    let api_key = require_api_key(&state).await?;
    let session_id = valid_session_id(&session_id)?;
    let url = format!("{DEVIN_API_BASE_URL}/sessions/{session_id}");
    let (status, body) = devin_request(&state, &api_key, Method::GET, &url, None).await?;
    upstream_json(status, body)
}

pub(super) async fn send_devin_message(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let api_key = require_api_key(&state).await?;
    let session_id = valid_session_id(&session_id)?;
    let message = text_field(&body, "message", MAX_MESSAGE_CHARS)?;
    let url = format!("{DEVIN_API_BASE_URL}/sessions/{session_id}/message");
    let (status, body) = devin_request(
        &state,
        &api_key,
        Method::POST,
        &url,
        Some(json!({"message": message})),
    )
    .await?;
    upstream_json(status, body)
}

async fn stored_api_key(state: &AppState) -> Result<Option<String>, (StatusCode, Json<Value>)> {
    let Some(master_key) = state.config.master_key.as_ref() else {
        return Err(fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "Set EXOROUTE_MASTER_KEY before using the Devin integration.",
        ));
    };
    let blob = devin_store::load_devin_api_key_blob(&state.db)
        .await
        .map_err(internal)?;
    let Some(blob) = blob else {
        return Ok(None);
    };
    decrypt_secret(Some(master_key), Some(&blob))
        .map_err(|error| fail(StatusCode::SERVICE_UNAVAILABLE, error))
}

async fn require_api_key(state: &AppState) -> Result<String, (StatusCode, Json<Value>)> {
    stored_api_key(state).await?.ok_or_else(|| {
        fail(
            StatusCode::CONFLICT,
            "A Devin API key is not configured yet. Save one in the Devin panel first.",
        )
    })
}

/// One bounded, egress-validated call to the Devin API. The key never
/// appears in errors or logs; upstream error text is bounded before it is
/// surfaced.
async fn devin_request(
    state: &AppState,
    api_key: &str,
    method: Method,
    url: &str,
    payload: Option<Value>,
) -> Result<(StatusCode, Value), (StatusCode, Json<Value>)> {
    let operational = state.operational_settings().settings;
    let (_url, client) = crate::security::egress::provider_client(
        url,
        false,
        operational.connect_timeout.min(DEVIN_CONNECT_TIMEOUT),
        operational.request_timeout.min(DEVIN_REQUEST_TIMEOUT),
        false,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        operational.upstream,
    )
    .await
    .map_err(|error| {
        fail(
            StatusCode::BAD_GATEWAY,
            format!("the Devin API endpoint was rejected: {error}"),
        )
    })?;
    let mut request = match method {
        Method::POST => client.post(url),
        _ => client.get(url),
    };
    request = request
        .bearer_auth(api_key)
        .header("Content-Type", "application/json");
    if let Some(payload) = payload {
        request = request.json(&payload);
    }
    let response = request.send().await.map_err(|error| {
        fail(
            StatusCode::BAD_GATEWAY,
            format!("the Devin API request failed: {error}"),
        )
    })?;
    let status = response.status();
    let body_bytes =
        crate::provider_adapters::read_limited_response(response, DEVIN_RESPONSE_LIMIT_BYTES)
            .await
            .map_err(|error| {
                fail(
                    StatusCode::BAD_GATEWAY,
                    format!("the Devin API response was invalid: {error}"),
                )
            })?;
    let body = serde_json::from_slice::<Value>(&body_bytes).unwrap_or_else(|_| json!({}));
    Ok((status, body))
}

fn upstream_json(status: StatusCode, body: Value) -> ApiResult {
    if status.is_success() {
        return Ok(Json(body));
    }
    let detail = body
        .get("error")
        .cloned()
        .unwrap_or_else(|| json!(status.as_u16().to_string()));
    Err(fail(
        StatusCode::BAD_GATEWAY,
        format!(
            "the Devin API returned {}: {}",
            status.as_u16(),
            compact_detail(&detail)
        ),
    ))
}

fn text_field(
    body: &Value,
    name: &str,
    max_chars: usize,
) -> Result<String, (StatusCode, Json<Value>)> {
    let value = body
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if value.is_empty() {
        return Err(fail(StatusCode::BAD_REQUEST, format!("Provide {name}.")));
    }
    if value.chars().count() > max_chars
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() && byte != b'\n' && byte != b'\t')
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            format!("{name} is too long or contains unsupported characters."),
        ));
    }
    Ok(value.to_owned())
}

fn optional_text(
    body: &Value,
    name: &str,
    max_chars: usize,
) -> Result<Option<String>, (StatusCode, Json<Value>)> {
    let value = body
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().count() > max_chars {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            format!("{name} is too long."),
        ));
    }
    Ok(Some(value.to_owned()))
}

fn optional_https_url(
    body: &Value,
    name: &str,
) -> Result<Option<String>, (StatusCode, Json<Value>)> {
    let Some(value) = optional_text(body, name, 512)? else {
        return Ok(None);
    };
    let url = reqwest::Url::parse(&value)
        .ok()
        .filter(|url| url.scheme() == "https" && url.host_str().is_some())
        .ok_or_else(|| {
            fail(
                StatusCode::BAD_REQUEST,
                "repository must be an HTTPS URL, for example https://github.com/owner/repo",
            )
        })?;
    Ok(Some(url.as_str().to_owned()))
}

/// Session ids are opaque Devin identifiers; only allow the characters the
/// API produces so an id can never smuggle extra URL path segments.
fn valid_session_id(value: &str) -> Result<String, (StatusCode, Json<Value>)> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().count() > MAX_SESSION_ID_CHARS
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(fail(StatusCode::BAD_REQUEST, "Invalid Devin session id."));
    }
    Ok(value.to_owned())
}

fn compact_detail(detail: &Value) -> String {
    let text = match detail {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    text.chars().take(300).collect()
}

#[cfg(test)]
mod devin_tests {
    use super::*;
    use crate::infra::storage::Record;
    use crate::support::test_support::TestDatabase;

    #[tokio::test]
    async fn settings_round_trip_stores_only_the_encrypted_key() {
        let database = TestDatabase::open().await;
        let mut config = database.config();
        config.master_key = Some([37_u8; 32]);
        let state = AppState::new(config, database.db.clone());

        let saved = save_devin_settings(
            State(state.clone()),
            Json(json!({"api_key": "cog_test_key_material"})),
        )
        .await;
        assert!(saved.is_ok(), "save failed: {saved:?}");

        // The raw record never holds plaintext.
        let raw = database
            .db
            .read(|transaction| {
                transaction.get::<Record>(crate::infra::storage::Table::Integrations, "singleton")
            })
            .await
            .unwrap()
            .expect("integration record exists");
        let blob = raw.bytes("devin_api_key").expect("encrypted blob");
        assert!(!String::from_utf8_lossy(blob).contains("cog_test_key_material"));

        let settings = devin_settings(State(state.clone())).await;
        let settings = settings.into_response();
        assert!(settings.status().is_success());

        let key = stored_api_key(&state).await.unwrap().expect("key decrypts");
        assert_eq!(key, "cog_test_key_material");

        let _ = delete_devin_settings(State(state)).await.unwrap();
        let raw = database
            .db
            .read(|transaction| {
                transaction.get::<Record>(crate::infra::storage::Table::Integrations, "singleton")
            })
            .await
            .unwrap()
            .expect("tombstone record exists");
        assert!(raw.optional_bytes("devin_api_key").unwrap().is_none());
    }

    #[tokio::test]
    async fn saving_a_key_without_a_master_key_fails_closed() {
        let database = TestDatabase::open().await;
        let state = AppState::new(database.config(), database.db.clone());

        let result = save_devin_settings(State(state), Json(json!({"api_key": "cog_x"}))).await;
        let (status, body) = result.unwrap_err();
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(body.to_string().contains("EXOROUTE_MASTER_KEY"));
    }

    #[test]
    fn session_ids_accept_only_api_identifier_characters() {
        assert_eq!(valid_session_id("devin-abc_123").unwrap(), "devin-abc_123");
        for bad in ["../etc", "devin/x", "devin x", "", "a".repeat(129).as_str()] {
            assert!(valid_session_id(bad).is_err(), "rejected: {bad}");
        }
    }

    #[test]
    fn repository_fields_must_be_https_urls() {
        assert!(
            optional_https_url(
                &json!({"repository": "https://github.com/o/r"}),
                "repository"
            )
            .unwrap()
            .is_some()
        );
        for bad in ["http://github.com/o/r", "not a url", "file:///etc/passwd"] {
            assert!(optional_https_url(&json!({"repository": bad}), "repository").is_err());
        }
    }
}
