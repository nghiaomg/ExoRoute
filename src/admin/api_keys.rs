use super::*;
use crate::infra::storage::{Field, Record, StorageError, Table};

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
        keys.push(json!({
            "id": row.text("id").map_err(internal)?,
            "name": row.text("name").map_err(internal)?,
            "enabled": row.boolean("enabled").map_err(internal)?,
            "created_at": row.text("created_at").map_err(internal)?,
            "last_used_at": row.optional_text("last_used_at").map_err(internal)?,
            "request_count": row.integer("request_count").map_err(internal)?
        }));
    }
    Ok(Json(json!({"api_keys":keys,"next_cursor":next_cursor})))
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
    let id = uuid::Uuid::new_v4().to_string();
    let token = format!("exo_{}", uuid::Uuid::new_v4().simple());
    let token_hash = token_hash(&token);
    let created_at = crate::infra::db::utc_timestamp_now().map_err(internal)?;
    let order_key = api_key_index_key(&created_at, &id).map_err(internal)?;
    let token_key = api_key_token_index_key(&token_hash);
    let record = Record::new()
        .with("id", Field::Text(id.clone()))
        .with("name", Field::Text(input.name.clone()))
        .with("token_hash", Field::Bytes(token_hash))
        .with("enabled", Field::Bool(true))
        .with("created_at", Field::Text(created_at))
        .with("last_used_at", Field::Null)
        .with("request_count", Field::I64(0))
        .with("request_count_generation", Field::I64(0));
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
    Ok(Json(
        json!({"id":id,"name":input.name,"key":token,"warning":"Copy this key now. It is shown only once."}),
    ))
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
}
