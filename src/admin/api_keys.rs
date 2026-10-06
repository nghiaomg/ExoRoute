use super::*;
use crate::infra::storage::{Field, Record, StorageError, Table};
use crate::security::api_key_scope::ApiKeyScope;

const DEFAULT_API_KEY_PAGE_SIZE: usize = 50;
const MAX_API_KEY_PAGE_SIZE: usize = 50;
const MAX_API_KEY_SEARCH_BYTES: usize = 256;
const MAX_API_KEY_SEARCH_SCAN: usize = 100_000;
const API_KEY_INDEX_PREFIX: &str = "created/";
const API_KEY_TOKEN_INDEX_PREFIX: &str = "token/";

#[derive(Debug, Default, Deserialize)]
pub(super) struct ApiKeyPageQuery {
    cursor: Option<String>,
    limit: Option<usize>,
    q: Option<String>,
}

#[derive(Debug)]
struct ApiKeyCursor {
    created_at: String,
    id: String,
}

pub(super) async fn list_api_keys(
    State(state): State<AppState>,
    Query(query): Query<ApiKeyPageQuery>,
) -> ApiResult {
    let limit = query.limit.unwrap_or(DEFAULT_API_KEY_PAGE_SIZE);
    if !(1..=MAX_API_KEY_PAGE_SIZE).contains(&limit) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "API key page size must be between 1 and 50",
        ));
    }
    let cursor = query
        .cursor
        .as_deref()
        .map(decode_api_key_cursor)
        .transpose()?;
    let search = query
        .q
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if search
        .as_ref()
        .is_some_and(|value| value.len() > MAX_API_KEY_SEARCH_BYTES)
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "API key search query is too long",
        ));
    }
    let (rows, next_cursor) =
        query_api_key_page(&state.db, search.as_deref(), cursor.as_ref(), limit)
            .await
            .map_err(internal)?;
    let mut keys = Vec::with_capacity(rows.len());
    for row in rows {
        let request_count = row.integer("request_count").map_err(internal)?;
        keys.push(api_key_json(&row, request_count).map_err(internal)?);
    }
    Ok(Json(json!({"api_keys":keys,"next_cursor":next_cursor})))
}

/// Serializes one stored key for the dashboard. A corrupt scope is reported
/// as `scope_valid: false` with null lists so the admin can still see and
/// repair the key, while the gateway keeps rejecting its requests.
fn api_key_json(record: &Record, request_count: i64) -> Result<Value, StorageError> {
    let scope = ApiKeyScope::from_record(record);
    let (allowed_provider_ids, allowed_models, scope_restricted) = match &scope {
        Ok(scope) => (
            json!(scope.allowed_provider_ids()),
            json!(scope.stored_model_rules()),
            !scope.is_unrestricted(),
        ),
        // An unreadable scope is reported as restricted: the gateway rejects
        // every request from that key until an operator repairs it.
        Err(_) => (Value::Null, Value::Null, true),
    };
    Ok(json!({
        "id": record.text("id")?,
        "name": record.text("name")?,
        "enabled": record.boolean("enabled")?,
        "created_at": record.text("created_at")?,
        "last_used_at": record.optional_text("last_used_at")?,
        "request_count": request_count,
        "allowed_provider_ids": allowed_provider_ids,
        "allowed_models": allowed_models,
        "scope_restricted": scope_restricted,
        "scope_valid": scope.is_ok()
    }))
}

fn api_key_request_count(record: &Record, generation: i64) -> Result<i64, StorageError> {
    match record.field("request_count_generation") {
        Some(Field::I64(value)) if *value == generation => record.integer("request_count"),
        Some(Field::I64(_)) | None => Ok(0),
        _ => Err(StorageError::Invalid(
            "API key usage generation is malformed".to_owned(),
        )),
    }
}

/// Canonicalizes a submitted scope and rejects provider references that do not
/// exist, so a saved scope is always enforceable.
async fn validated_scope(
    db: &crate::infra::storage::Database,
    provider_ids: &[String],
    model_rules: &[String],
) -> Result<ApiKeyScope, (StatusCode, Json<Value>)> {
    let scope = ApiKeyScope::from_entries(provider_ids, model_rules).map_err(|message| {
        fail(
            StatusCode::BAD_REQUEST,
            format!("API key access scope is invalid: {message}"),
        )
    })?;
    if scope.allowed_provider_ids().is_empty() {
        return Ok(scope);
    }
    let requested = scope.allowed_provider_ids().to_vec();
    let missing = db
        .read(move |transaction| {
            let mut missing = Vec::new();
            for provider_id in &requested {
                if transaction
                    .get::<Record>(Table::Providers, provider_id)?
                    .is_none()
                {
                    missing.push(provider_id.clone());
                }
            }
            Ok(missing)
        })
        .await
        .map_err(internal)?;
    if let Some(provider_id) = missing.first() {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            format!("provider '{provider_id}' does not exist"),
        ));
    }
    Ok(scope)
}

pub(super) fn api_key_index_key(created_at: &str, id: &str) -> Result<String, StorageError> {
    let key = format!(
        "{API_KEY_INDEX_PREFIX}{}{}/{}",
        reverse_lexical_component(created_at),
        reverse_lexical_component(id),
        id
    );
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub(super) fn api_key_token_index_key(token_hash: &[u8]) -> String {
    format!("{API_KEY_TOKEN_INDEX_PREFIX}{}", hex(token_hash))
}

fn reverse_lexical_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len().saturating_mul(2).saturating_add(2));
    for byte in value.bytes() {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{:02x}", 255_u8.saturating_sub(byte));
    }
    encoded.push_str("ff");
    encoded
}

async fn query_api_key_page(
    db: &crate::infra::storage::Database,
    search: Option<&str>,
    cursor: Option<&ApiKeyCursor>,
    limit: usize,
) -> Result<(Vec<Record>, Option<String>), StorageError> {
    let after = cursor
        .map(|cursor| api_key_index_key(&cursor.created_at, &cursor.id))
        .transpose()?;
    let search = search.map(str::to_lowercase);
    db.read(move |transaction| {
        let generation = transaction
            .get::<i64>(Table::Meta, "statistics_generation")?
            .unwrap_or(0);
        let mut rows = Vec::with_capacity(limit.saturating_add(1));
        let mut next_key = after;
        let mut scanned = 0usize;
        let mut exhausted = false;
        while rows.len() <= limit && !exhausted {
            let remaining_scan = MAX_API_KEY_SEARCH_SCAN.saturating_sub(scanned);
            if remaining_scan == 0 {
                return Err(StorageError::Busy);
            }
            let page_limit = remaining_scan.min(256);
            let page = transaction.scan_prefix_after::<String>(
                Table::ApiKeyIndex,
                API_KEY_INDEX_PREFIX,
                next_key.as_deref(),
                page_limit,
            )?;
            if page.is_empty() {
                break;
            }
            let page_len = page.len();
            for (index_key, id) in page {
                next_key = Some(index_key);
                scanned += 1;
                let mut record =
                    transaction
                        .get::<Record>(Table::ApiKeys, &id)?
                        .ok_or_else(|| {
                            StorageError::Invalid(
                                "API key order index points to a missing record".to_owned(),
                            )
                        })?;
                let name = record.text("name")?;
                if search.as_ref().is_some_and(|term| {
                    !name.to_lowercase().contains(term) && !id.to_lowercase().contains(term)
                }) {
                    continue;
                }
                let request_count = match record.field("request_count_generation") {
                    Some(Field::I64(value)) if *value == generation => {
                        record.integer("request_count")?
                    }
                    Some(Field::I64(_)) | None => 0,
                    _ => {
                        return Err(StorageError::Invalid(
                            "API key usage generation is malformed".to_owned(),
                        ));
                    }
                };
                record.insert("request_count", Field::I64(request_count));
                rows.push(record);
                if rows.len() > limit {
                    break;
                }
            }
            if page_len < page_limit {
                exhausted = true;
            }
            if scanned >= MAX_API_KEY_SEARCH_SCAN && rows.len() <= limit {
                // Search remains bounded even when the requested substring is rare.
                return Err(StorageError::Busy);
            }
        }
        let has_more = rows.len() > limit;
        rows.truncate(limit);
        let next_cursor = if has_more {
            rows.last()
                .map(|row| {
                    Ok::<String, StorageError>(encode_api_key_cursor(
                        row.text("created_at")?,
                        row.text("id")?,
                    ))
                })
                .transpose()?
        } else {
            None
        };
        Ok((rows, next_cursor))
    })
    .await
}

fn encode_api_key_cursor(created_at: &str, id: &str) -> String {
    let mut payload = Vec::with_capacity(2 + created_at.len() + id.len());
    payload.extend_from_slice(&(created_at.len() as u16).to_be_bytes());
    payload.extend_from_slice(created_at.as_bytes());
    payload.extend_from_slice(id.as_bytes());
    URL_SAFE_NO_PAD.encode(payload)
}

fn decode_api_key_cursor(cursor: &str) -> Result<ApiKeyCursor, (StatusCode, Json<Value>)> {
    if cursor.len() > 2048 {
        return Err(fail(StatusCode::BAD_REQUEST, "API key cursor is invalid"));
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "API key cursor is invalid"))?;
    if decoded.len() < 4 {
        return Err(fail(StatusCode::BAD_REQUEST, "API key cursor is invalid"));
    }
    let timestamp_length = usize::from(u16::from_be_bytes([decoded[0], decoded[1]]));
    let timestamp_end = 2_usize.saturating_add(timestamp_length);
    if timestamp_length == 0 || timestamp_length > 64 || timestamp_end >= decoded.len() {
        return Err(fail(StatusCode::BAD_REQUEST, "API key cursor is invalid"));
    }
    let created_at = std::str::from_utf8(&decoded[2..timestamp_end])
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "API key cursor is invalid"))?
        .to_owned();
    let id = std::str::from_utf8(&decoded[timestamp_end..])
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "API key cursor is invalid"))?
        .to_owned();
    if id.is_empty() || id.len() > 256 {
        return Err(fail(StatusCode::BAD_REQUEST, "API key cursor is invalid"));
    }
    Ok(ApiKeyCursor { created_at, id })
}

#[derive(Deserialize)]
pub(super) struct ApiKeyInput {
    name: String,
    allowed_provider_ids: Option<Vec<String>>,
    allowed_models: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub(super) struct ApiKeyUpdateInput {
    name: Option<String>,
    enabled: Option<bool>,
    allowed_provider_ids: Option<Vec<String>>,
    allowed_models: Option<Vec<String>>,
}

pub(super) async fn create_api_key(
    State(state): State<AppState>,
    Json(input): Json<ApiKeyInput>,
) -> ApiResult {
    if input.name.trim().is_empty() {
        return Err(fail(StatusCode::BAD_REQUEST, "key name is required"));
    }
    if input.name.len() > 256 {
        return Err(fail(StatusCode::BAD_REQUEST, "key name is too long"));
    }
    let scope = validated_scope(
        &state.db,
        input.allowed_provider_ids.as_deref().unwrap_or_default(),
        input.allowed_models.as_deref().unwrap_or_default(),
    )
    .await?;
    let id = uuid::Uuid::new_v4().to_string();
    let token = format!("exo_{}", uuid::Uuid::new_v4().simple());
    let token_hash = token_hash(&token);
    let created_at = crate::infra::db::utc_timestamp_now().map_err(internal)?;
    let order_key = api_key_index_key(&created_at, &id).map_err(internal)?;
    let token_key = api_key_token_index_key(&token_hash);
    let mut record = Record::new()
        .with("id", Field::Text(id.clone()))
        .with("name", Field::Text(input.name.clone()))
        .with("token_hash", Field::Bytes(token_hash))
        .with("enabled", Field::Bool(true))
        .with("created_at", Field::Text(created_at))
        .with("last_used_at", Field::Null)
        .with("request_count", Field::I64(0))
        .with("request_count_generation", Field::I64(0));
    scope.write_to_record(&mut record).map_err(internal)?;
    let stored_id = id.clone();
    let index_id = id.clone();
    state
        .db
        .write(move |transaction| {
            transaction.put_if_absent(Table::ApiKeys, &stored_id, &record)?;
            transaction.put_if_absent(Table::ApiKeyIndex, &order_key, &index_id)?;
            transaction.put_if_absent(Table::ApiKeyTokenIndex, &token_key, &index_id)
        })
        .await
        .map_err(internal)?;
    Ok(Json(json!({
        "id":id,
        "name":input.name,
        "key":token,
        "warning":"Copy this key now. It is shown only once.",
        "allowed_provider_ids":scope.allowed_provider_ids(),
        "allowed_models":scope.stored_model_rules(),
        "scope_restricted":!scope.is_unrestricted(),
        "scope_valid":true
    })))
}

/// Updates the editable fields of a gateway API key. Scope fields are replaced
/// together: sending one list without the other is rejected so a client can
/// never clear a restriction by omission.
pub(super) async fn update_api_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ApiKeyUpdateInput>,
) -> ApiResult {
    if let Some(name) = input.name.as_ref() {
        if name.trim().is_empty() {
            return Err(fail(StatusCode::BAD_REQUEST, "key name is required"));
        }
        if name.len() > 256 {
            return Err(fail(StatusCode::BAD_REQUEST, "key name is too long"));
        }
    }
    let scope = match (&input.allowed_provider_ids, &input.allowed_models) {
        (None, None) => None,
        (Some(providers), Some(models)) => {
            Some(validated_scope(&state.db, providers, models).await?)
        }
        _ => {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "allowed_provider_ids and allowed_models must be provided together",
            ));
        }
    };
    let name = input.name;
    let enabled = input.enabled;
    let updated = state
        .db
        .write(move |transaction| {
            let Some(mut record) = transaction.get::<Record>(Table::ApiKeys, &id)? else {
                return Ok(None);
            };
            if let Some(name) = name.as_ref() {
                record.insert("name", Field::Text(name.clone()));
            }
            if let Some(enabled) = enabled {
                record.insert("enabled", Field::Bool(enabled));
            }
            if let Some(scope) = scope.as_ref() {
                scope.write_to_record(&mut record)?;
            }
            transaction.put(Table::ApiKeys, &id, &record)?;
            let generation = transaction
                .get::<i64>(Table::Meta, "statistics_generation")?
                .unwrap_or(0);
            let request_count = api_key_request_count(&record, generation)?;
            Ok(Some(api_key_json(&record, request_count)?))
        })
        .await
        .map_err(internal)?;
    match updated {
        Some(updated) => Ok(Json(updated)),
        None => Err(fail(StatusCode::NOT_FOUND, "API key not found")),
    }
}

pub(super) async fn delete_api_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult {
    let result = state
        .db
        .write(move |transaction| {
            let Some(record) = transaction.get::<Record>(Table::ApiKeys, &id)? else {
                return Ok(false);
            };
            let created_at = record.text("created_at")?.to_owned();
            let hash = record.bytes("token_hash")?.to_vec();
            transaction.delete(Table::ApiKeys, &id)?;
            transaction.delete(Table::ApiKeyIndex, &api_key_index_key(&created_at, &id)?)?;
            transaction.delete(Table::ApiKeyTokenIndex, &api_key_token_index_key(&hash))?;
            let run_indexes = transaction.scan_prefix::<String>(
                Table::StreamRunApiKeyIndex,
                &format!("{id}/"),
                1_000,
            )?;
            for (index, run_id) in run_indexes {
                transaction.delete(Table::StreamRunApiKeyIndex, &index)?;
                transaction.delete(Table::StreamRuns, &run_id)?;
                transaction.delete_prefix(Table::StreamEvents, &format!("{run_id}/"))?;
            }
            Ok(true)
        })
        .await
        .map_err(internal)?;
    if !result {
        return Err(fail(StatusCode::NOT_FOUND, "API key not found"));
    }
    Ok(Json(json!({"ok":true})))
}

fn hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::db;
    use std::{collections::HashSet, fs};

    #[tokio::test]
    async fn api_key_pages_are_bounded_stable_and_searchable() {
        let path = std::env::temp_dir().join(format!("exoroute-api-keys-{}", uuid::Uuid::new_v4()));
        let database = db::connect(&path)
            .await
            .expect("open LMDB test environment");
        database
            .write(move |transaction| {
                for index in 0..137 {
                    let id = format!("id-{index:03}");
                    let name = format!("service-{index:03}");
                    let created_at = "2026-09-13 12:00:00";
                    let record = Record::new()
                        .with("id", Field::Text(id.clone()))
                        .with("name", Field::Text(name))
                        .with("enabled", Field::Bool(true))
                        .with("created_at", Field::Text(created_at.to_owned()))
                        .with("last_used_at", Field::Null)
                        .with("request_count", Field::I64(index))
                        .with("request_count_generation", Field::I64(0))
                        .with("token_hash", Field::Bytes(vec![index as u8]));
                    transaction.put(Table::ApiKeys, &id, &record)?;
                    transaction.put(
                        Table::ApiKeyIndex,
                        &api_key_index_key(created_at, &id)?,
                        &id,
                    )?;
                }
                Ok(())
            })
            .await
            .expect("seed API keys");

        let mut cursor = None;
        let mut ids = HashSet::new();
        loop {
            let page_cursor = cursor
                .as_deref()
                .map(decode_api_key_cursor)
                .transpose()
                .expect("decode next cursor");
            let (rows, next) = query_api_key_page(&database, None, page_cursor.as_ref(), 50)
                .await
                .expect("fetch bounded page");
            assert!(rows.len() <= 50);
            for row in rows {
                assert!(ids.insert(row.text("id").expect("API key id").to_owned()));
            }
            cursor = next;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(ids.len(), 137);

        let (matches, next) = query_api_key_page(&database, Some("%service-120%"), None, 50)
            .await
            .expect("literal wildcard search");
        assert!(matches.is_empty());
        assert!(next.is_none());
        let (matches, _) = query_api_key_page(&database, Some("service-120"), None, 50)
            .await
            .expect("search API keys");
        assert_eq!(matches.len(), 1);

        drop(database);
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn api_key_cursor_rejects_malformed_values() {
        assert!(decode_api_key_cursor("not-a-cursor").is_err());
        let cursor = encode_api_key_cursor("2026-09-13 12:00:00", "id-1");
        let decoded = decode_api_key_cursor(&cursor).expect("round-trip cursor");
        assert_eq!(decoded.created_at, "2026-09-13 12:00:00");
        assert_eq!(decoded.id, "id-1");
    }

    #[test]
    fn api_key_json_reports_a_corrupt_scope_as_invalid_and_restricted() {
        let record = Record::new()
            .with("id", Field::Text("key-1".to_owned()))
            .with("name", Field::Text("Key".to_owned()))
            .with("enabled", Field::Bool(true))
            .with("created_at", Field::Text("2026-09-13 12:00:00".to_owned()))
            .with("last_used_at", Field::Null)
            .with("allowed_models", Field::Text("{".to_owned()));
        let payload = api_key_json(&record, 3).expect("serialize API key");
        assert_eq!(payload["scope_valid"], json!(false));
        assert_eq!(payload["scope_restricted"], json!(true));
        assert_eq!(payload["allowed_models"], Value::Null);
        assert_eq!(payload["request_count"], json!(3));
    }

    #[test]
    fn api_key_request_count_resets_with_the_statistics_generation() {
        let record = Record::new()
            .with("request_count", Field::I64(9))
            .with("request_count_generation", Field::I64(2));
        assert_eq!(
            api_key_request_count(&record, 2).expect("current generation"),
            9
        );
        assert_eq!(
            api_key_request_count(&record, 3).expect("stale generation"),
            0
        );
    }

    #[tokio::test]
    async fn create_and_update_validate_and_persist_the_access_scope() {
        let database = crate::support::test_support::TestDatabase::open().await;
        let state = AppState::new(database.config(), database.db.clone());
        crate::support::test_support::seed_provider(
            &database.db,
            crate::support::test_support::ProviderSeed {
                id: "provider-a",
                name: "Provider A",
                base_url: "http://127.0.0.1:9/v1",
                adapter_id: "generic",
                auth_type: "none",
                model_prefix: "mock-a",
                preferred_protocol: "chat_completions",
                supported_protocols: &["chat_completions"],
            },
        )
        .await
        .expect("seed provider");

        let created = create_api_key(
            State(state.clone()),
            Json(ApiKeyInput {
                name: "scoped".to_owned(),
                allowed_provider_ids: Some(vec![
                    " provider-a ".to_owned(),
                    "provider-a".to_owned(),
                ]),
                allowed_models: Some(vec![" coding ".to_owned(), "gpt-4*".to_owned()]),
            }),
        )
        .await
        .expect("create scoped key")
        .0;
        let id = created["id"].as_str().expect("created id").to_owned();
        assert_eq!(created["allowed_provider_ids"], json!(["provider-a"]));
        assert_eq!(created["allowed_models"], json!(["coding", "gpt-4*"]));
        assert_eq!(created["scope_restricted"], json!(true));
        assert_eq!(created["scope_valid"], json!(true));

        let stored = database
            .db
            .read({
                let id = id.clone();
                move |transaction| transaction.get::<Record>(Table::ApiKeys, &id)
            })
            .await
            .expect("read stored key")
            .expect("stored key exists");
        let scope = ApiKeyScope::from_record(&stored).expect("stored scope parses");
        assert_eq!(scope.allowed_provider_ids(), ["provider-a"]);
        assert_eq!(scope.stored_model_rules(), ["coding", "gpt-4*"]);

        let updated = update_api_key(
            State(state.clone()),
            Path(id.clone()),
            Json(ApiKeyUpdateInput {
                name: None,
                enabled: Some(false),
                allowed_provider_ids: Some(Vec::new()),
                allowed_models: Some(vec!["coding".to_owned()]),
            }),
        )
        .await
        .expect("update scope")
        .0;
        assert_eq!(updated["enabled"], json!(false));
        assert_eq!(updated["scope_restricted"], json!(true));
        assert_eq!(updated["allowed_provider_ids"], json!([]));
        assert_eq!(updated["allowed_models"], json!(["coding"]));

        // A partial scope update could silently drop a restriction.
        let error = update_api_key(
            State(state.clone()),
            Path(id.clone()),
            Json(ApiKeyUpdateInput {
                name: None,
                enabled: None,
                allowed_provider_ids: Some(Vec::new()),
                allowed_models: None,
            }),
        )
        .await
        .expect_err("partial scope update is rejected");
        assert_eq!(error.0, StatusCode::BAD_REQUEST);

        let missing = update_api_key(
            State(state),
            Path("missing-key".to_owned()),
            Json(ApiKeyUpdateInput {
                name: Some("renamed".to_owned()),
                enabled: None,
                allowed_provider_ids: None,
                allowed_models: None,
            }),
        )
        .await
        .expect_err("unknown key is rejected");
        assert_eq!(missing.0, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn create_rejects_unknown_providers_and_malformed_model_rules() {
        let database = crate::support::test_support::TestDatabase::open().await;
        let state = AppState::new(database.config(), database.db.clone());
        let unknown_provider = create_api_key(
            State(state.clone()),
            Json(ApiKeyInput {
                name: "unknown".to_owned(),
                allowed_provider_ids: Some(vec!["missing-provider".to_owned()]),
                allowed_models: None,
            }),
        )
        .await
        .expect_err("unknown provider is rejected");
        assert_eq!(unknown_provider.0, StatusCode::BAD_REQUEST);
        assert_eq!(
            unknown_provider.1.0["error"]["message"],
            json!("provider 'missing-provider' does not exist")
        );

        let malformed_rule = create_api_key(
            State(state),
            Json(ApiKeyInput {
                name: "malformed".to_owned(),
                allowed_provider_ids: None,
                allowed_models: Some(vec!["gp*t".to_owned()]),
            }),
        )
        .await
        .expect_err("malformed model rule is rejected");
        assert_eq!(malformed_rule.0, StatusCode::BAD_REQUEST);
    }
}
