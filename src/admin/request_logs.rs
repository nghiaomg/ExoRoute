//! Request-log administration: paginated history, live SSE stream,
//! deletion, and cursor handling for the requests page.

use super::*;
use crate::infra::storage::{Record, StorageError, Table};
use crate::state::{RequestLiveEvent, RequestLiveRow, RequestLiveSnapshot, RequestLogRecord};
use axum::response::sse::{Event, KeepAlive, Sse};
use std::convert::Infallible;

#[derive(Debug, Deserialize, Default)]
pub(super) struct RequestLogQuery {
    limit: Option<i64>,
    cursor: Option<String>,
    api_key_id: Option<String>,
    model: Option<String>,
    provider_id: Option<String>,
    status: Option<String>,
}

struct RequestLogCursor {
    created_at: String,
    id: String,
}

pub(super) async fn list_requests(
    State(state): State<AppState>,
    Query(query): Query<RequestLogQuery>,
) -> ApiResult {
    let limit = query.limit.unwrap_or(50);
    if !(1..=200).contains(&limit) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "Request page size must be between 1 and 200.",
        ));
    }
    let cursor = query
        .cursor
        .as_deref()
        .map(decode_request_cursor)
        .transpose()?;
    let api_key_id = request_filter(query.api_key_id, "API key ID")?;
    let model = request_filter(query.model, "model")?;
    let provider_id = request_filter(query.provider_id, "provider ID")?;
    let status = match query
        .status
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => None,
        Some("success") => Some(RequestStatusFilter::Success),
        Some("failure") => Some(RequestStatusFilter::Failure),
        Some(value) => match value.parse::<i64>() {
            Ok(status) if (100..=599).contains(&status) => Some(RequestStatusFilter::Code(status)),
            _ => {
                return Err(fail(
                    StatusCode::BAD_REQUEST,
                    "Status must be success, failure, or an HTTP status from 100 to 599.",
                ));
            }
        },
    };

    let after = cursor
        .as_ref()
        .map(|cursor| crate::infra::db::request_log_index_key(&cursor.created_at, &cursor.id))
        .transpose()
        .map_err(internal)?;
    let limit = usize::try_from(limit).map_err(internal)?;
    let api_key_filter = api_key_id;
    let model_filter = model;
    let provider_filter = provider_id;
    let (requests, next_cursor) = state
        .db
        .read(move |transaction| {
            let mut rows = Vec::<(Value, String, String)>::with_capacity(limit.saturating_add(1));
            let mut next_key = after;
            let mut scanned = 0usize;
            let mut exhausted = false;
            while rows.len() <= limit && !exhausted {
                let remaining = 100_000usize.saturating_sub(scanned);
                if remaining == 0 {
                    return Err(StorageError::Busy);
                }
                let batch_limit = remaining.min(256);
                // The ascending index exists in every request-log format. Reading it in
                // reverse keeps installations created before the descending index usable.
                let page = transaction.scan_prefix_reverse_after::<String>(
                    Table::RequestLogIndex,
                    crate::infra::db::REQUEST_LOG_INDEX_PREFIX,
                    next_key.as_deref(),
                    batch_limit,
                )?;
                if page.is_empty() {
                    break;
                }
                let page_len = page.len();
                for (index_key, id) in page {
                    next_key = Some(index_key);
                    scanned += 1;
                    let record = transaction
                        .get::<Record>(Table::RequestLogs, &id)?
                        .ok_or_else(|| {
                            StorageError::Invalid(
                                "request log index points to a missing record".to_owned(),
                            )
                        })?;
                    let row_api_key_id = record.optional_text("api_key_id")?;
                    let matches_api_key = match api_key_filter.as_deref() {
                        Some(filter) if filter.eq_ignore_ascii_case("unknown") => {
                            row_api_key_id.is_none()
                        }
                        Some(filter) => row_api_key_id == Some(filter),
                        None => true,
                    };
                    let matches_model = match model_filter.as_deref() {
                        Some(filter) => record.text("model")? == filter,
                        None => true,
                    };
                    let matches_provider = match provider_filter.as_deref() {
                        Some(filter) => record.optional_text("provider_id")? == Some(filter),
                        None => true,
                    };
                    let code = record.integer("status")?;
                    let matches_status = match status {
                        Some(RequestStatusFilter::Success) => (200..300).contains(&code),
                        Some(RequestStatusFilter::Failure) => !(200..300).contains(&code),
                        Some(RequestStatusFilter::Code(expected)) => code == expected,
                        None => true,
                    };
                    if !(matches_api_key && matches_model && matches_provider && matches_status) {
                        continue;
                    }
                    let api_key_name = match row_api_key_id {
                        Some(key_id) => match transaction.get::<Record>(Table::ApiKeys, key_id)? {
                            Some(key) => key.optional_text("name")?.map(str::to_owned),
                            None => Some(key_id.to_owned()),
                        },
                        None => None,
                    };
                    let created_at = record.text("created_at")?.to_owned();
                    rows.push((
                        json!({
                            "id": id.clone(),
                            "request_id": record.text("request_id")?,
                            "route_alias": record.text("route_alias")?,
                            "provider_id": record.optional_text("provider_id")?,
                            "provider_credential_id": record.optional_text("provider_credential_id")?,
                            "api_key_id": row_api_key_id,
                            "api_key_name": api_key_name,
                            "model": record.text("model")?,
                            "client_protocol": record.text("client_protocol")?,
                            "upstream_protocol": record.optional_text("upstream_protocol")?,
                            "status": code,
                            "duration_ms": record.integer("duration_ms")?,
                            "input_tokens": record.optional_integer("input_tokens")?,
                            "output_tokens": record.optional_integer("output_tokens")?,
                            "cached_tokens": record.optional_integer("cached_tokens")?,
                            "cache_input_tokens": record.optional_integer("cache_input_tokens")?,
                            "cost_micro_usd": record.optional_integer("cost_micro_usd")?,
                            "error": record.optional_text("error")?,
                            "created_at": created_at
                        }),
                        created_at,
                        id,
                    ));
                    if rows.len() > limit {
                        break;
                    }
                }
                if page_len < batch_limit {
                    exhausted = true;
                }
            }
            let has_more = rows.len() > limit;
            rows.truncate(limit);
            let next_cursor = if has_more {
                rows.last()
                    .map(|(_, created_at, id)| encode_request_cursor(created_at, id))
            } else {
                None
            };
            Ok((
                rows.into_iter().map(|(row, _, _)| row).collect::<Vec<_>>(),
                next_cursor,
            ))
        })
        .await
        .map_err(internal)?;
    Ok(Json(
        json!({"requests":requests,"next_cursor":next_cursor,"page_size":limit}),
    ))
}

const REQUEST_LIVE_STREAM_DURATION: Duration = Duration::from_secs(5 * 60);

pub(super) async fn request_events(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let connection_permit = state
        .admin
        .admin_upstream_event_streams
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            fail(
                StatusCode::TOO_MANY_REQUESTS,
                "The ExoRoute API is busy. Try again shortly.",
            )
        })?;
    // Subscribe before taking the snapshot so events that race the initial
    // read remain queued and can be applied after the snapshot.
    let mut updates = state.subscribe_request_live();
    let initial = state.request_live_snapshot();
    let events = async_stream::stream! {
        let _connection_permit = connection_permit;
        let mut shutdown = state.shutdown_receiver();
        yield Ok::<Event, Infallible>(request_live_snapshot_event(&initial));

        let max_duration = tokio::time::sleep(REQUEST_LIVE_STREAM_DURATION);
        tokio::pin!(max_duration);
        loop {
            tokio::select! {
                _ = &mut max_duration => break,
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                update = updates.recv() => {
                    match update {
                        Ok(update) => yield Ok(request_live_event(update)),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            let snapshot = state.request_live_snapshot();
                            yield Ok(request_live_snapshot_event(&snapshot));
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    };
    Ok(Sse::new(events).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("keep-alive"),
    ))
}

fn request_live_snapshot_event(snapshot: &RequestLiveSnapshot) -> Event {
    let payload = json!({
        "requests": snapshot
            .requests
            .iter()
            .map(request_live_row_value)
            .collect::<Vec<_>>(),
        "truncated": snapshot.truncated,
        "limit": snapshot.limit,
        "active_count": snapshot.active_count,
    });
    request_sse_event("request-snapshot", payload)
}

fn request_live_event(event: RequestLiveEvent) -> Event {
    match event {
        RequestLiveEvent::Started { row, active_count } => {
            let mut payload = request_live_row_value(&row);
            payload["active_count"] = json!(active_count);
            request_sse_event("request-started", payload)
        }
        RequestLiveEvent::Updated {
            live_id,
            provider_id,
            latest_log_id,
        } => request_sse_event(
            "request-updated",
            json!({"live_id":live_id,"provider_id":provider_id,"latest_log_id":latest_log_id}),
        ),
        RequestLiveEvent::Finished {
            live_id,
            request,
            finished_at_ms,
            active_count,
        } => request_sse_event(
            "request-finished",
            json!({
                "live_id": live_id,
                "request": request.as_ref().map(|record| request_log_value(record, finished_at_ms)),
                "finished_at_ms": finished_at_ms,
                "active_count": active_count,
            }),
        ),
        RequestLiveEvent::Capacity {
            truncated,
            limit,
            active_count,
        } => request_sse_event(
            "request-capacity",
            json!({"truncated":truncated,"limit":limit,"active_count":active_count}),
        ),
    }
}

fn request_sse_event(name: &'static str, payload: Value) -> Event {
    Event::default().event(name).data(payload.to_string())
}

fn request_live_row_value(row: &RequestLiveRow) -> Value {
    json!({
        "live": true,
        "id": row.id,
        "live_id": row.live_id,
        "request_id": row.request_id,
        "route_alias": row.route_alias,
        "provider_id": row.provider_id,
        "provider_credential_id": row.provider_credential_id,
        "api_key_id": row.api_key_id,
        "api_key_name": null,
        "model": row.model,
        "client_protocol": row.client_protocol,
        "upstream_protocol": row.upstream_protocol,
        "status": null,
        "duration_ms": null,
        "input_tokens": null,
        "output_tokens": null,
        "cached_tokens": null,
        "cache_input_tokens": null,
        "cost_micro_usd": null,
        "error": null,
        "created_at": null,
        "started_at_ms": row.started_at_ms,
    })
}

fn request_log_value(record: &RequestLogRecord, finished_at_ms: i64) -> Value {
    json!({
        "live": false,
        "id": record.id,
        "request_id": record.request_id,
        "route_alias": record.route_alias,
        "provider_id": record.provider_id,
        "provider_credential_id": record.provider_credential_id,
        "api_key_id": record.api_key_id,
        "api_key_name": null,
        "model": record.model,
        "client_protocol": record.client_protocol,
        "upstream_protocol": record.upstream_protocol,
        "status": record.status,
        "duration_ms": record.duration_ms,
        "input_tokens": record.input_tokens,
        "output_tokens": record.output_tokens,
        "cached_tokens": record.cached_tokens,
        "cache_input_tokens": record.cache_input_tokens,
        "cost_micro_usd": record.cost_micro_usd,
        "error": record.error,
        "created_at": null,
        "finished_at_ms": finished_at_ms,
    })
}

#[derive(Clone, Copy)]
enum RequestStatusFilter {
    Success,
    Failure,
    Code(i64),
}

fn request_filter(
    value: Option<String>,
    name: &str,
) -> Result<Option<String>, (StatusCode, Json<Value>)> {
    let Some(value) = value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    if value.chars().count() > 256
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            format!("The {name} filter is invalid or too long."),
        ));
    }
    Ok(Some(value))
}

fn decode_request_cursor(value: &str) -> Result<RequestLogCursor, (StatusCode, Json<Value>)> {
    let invalid = || {
        fail(
            StatusCode::BAD_REQUEST,
            "The request log cursor is invalid.",
        )
    };
    if value.is_empty() || value.len() > 512 {
        return Err(invalid());
    }
    let decoded = URL_SAFE_NO_PAD.decode(value).map_err(|_| invalid())?;
    let decoded = String::from_utf8(decoded).map_err(|_| invalid())?;
    let (created_at, id) = decoded.split_once('\0').ok_or_else(invalid)?;
    if created_at.is_empty()
        || created_at.len() > 64
        || id.is_empty()
        || id.len() > 128
        || created_at.bytes().any(|byte| byte.is_ascii_control())
        || id.bytes().any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(invalid());
    }
    Ok(RequestLogCursor {
        created_at: created_at.to_owned(),
        id: id.to_owned(),
    })
}

fn encode_request_cursor(created_at: &str, id: &str) -> String {
    URL_SAFE_NO_PAD.encode(format!("{created_at}\0{id}"))
}

pub(super) async fn delete_request_logs(State(state): State<AppState>) -> ApiResult {
    let deleted = state
        .telemetry
        .clear_request_history()
        .await
        .map_err(internal_message)?;
    Ok(Json(json!({"deleted_count":deleted})))
}

#[cfg(test)]
#[cfg(test)]
mod request_log_tests {
    use super::*;
    use crate::{
        infra::storage::{Field, Record, Table},
        state::AppState,
        support::test_support::TestDatabase,
    };

    #[tokio::test]
    async fn request_live_sse_sends_snapshot_and_releases_connection_slot() {
        use http_body_util::BodyExt;

        let database = TestDatabase::open().await;
        let state = AppState::new(database.config(), database.db.clone());
        let response = request_events(State(state.clone()))
            .await
            .expect("request live SSE response")
            .into_response();
        assert!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("text/event-stream"))
        );
        let mut body = response.into_body();
        let frame = tokio::time::timeout(Duration::from_secs(1), body.frame())
            .await
            .expect("request live snapshot timeout")
            .expect("request live snapshot frame")
            .expect("valid request live SSE frame")
            .into_data()
            .expect("request live SSE data");
        let frame = String::from_utf8_lossy(&frame);
        assert!(frame.contains("request-snapshot"));
        assert!(frame.contains(r#""requests":[]"#));
        assert!(frame.contains(r#""truncated":false"#));
        assert!(frame.contains(r#""limit":64"#));
        assert_eq!(
            state.admin.admin_upstream_event_streams.available_permits(),
            7
        );
        drop(body);
        assert_eq!(
            state.admin.admin_upstream_event_streams.available_permits(),
            8
        );
    }

    fn request_log_record(id: &str, created_at: &str, error: Option<&str>) -> Record {
        Record::new()
            .with("id", Field::Text(id.to_owned()))
            .with("request_id", Field::Text(format!("request-{id}")))
            .with("route_alias", Field::Text("coding".to_owned()))
            .with("provider_id", Field::Text("provider".to_owned()))
            .with("provider_credential_id", Field::Null)
            .with("api_key_id", Field::Null)
            .with("model", Field::Text("mock-model".to_owned()))
            .with(
                "client_protocol",
                Field::Text("chat_completions".to_owned()),
            )
            .with(
                "upstream_protocol",
                Field::Text("chat_completions".to_owned()),
            )
            .with(
                "status",
                Field::I64(if error.is_some() { 502 } else { 200 }),
            )
            .with("duration_ms", Field::I64(10))
            .with("input_tokens", Field::Null)
            .with("output_tokens", Field::Null)
            .with("cost_micro_usd", Field::Null)
            .with(
                "error",
                error
                    .map(|value| Field::Text(value.to_owned()))
                    .unwrap_or(Field::Null),
            )
            .with("created_at", Field::Text(created_at.to_owned()))
    }

    #[tokio::test]
    async fn requests_page_reads_legacy_ascending_time_index_and_keeps_provider_errors() {
        let database = TestDatabase::open().await;
        let entries = [
            (
                "log-old",
                "2026-09-15 07:08:00",
                Some("provider 'provider' returned HTTP 502: upstream unavailable"),
            ),
            ("log-middle", "2026-09-15 07:09:00", None),
            ("log-new", "2026-09-15 07:10:00", None),
        ];
        database
            .db
            .write(move |transaction| {
                for (id, created_at, error) in entries {
                    transaction.put(
                        Table::RequestLogs,
                        id,
                        &request_log_record(id, created_at, error),
                    )?;
                    let index_key = crate::infra::db::request_log_index_key(created_at, id)?;
                    transaction.put(Table::RequestLogIndex, &index_key, &id.to_owned())?;
                }
                Ok(())
            })
            .await
            .expect("seed legacy request logs");

        let state = AppState::new(database.config(), database.db.clone());
        let Json(first_page) = list_requests(
            State(state.clone()),
            Query(RequestLogQuery {
                limit: Some(2),
                ..RequestLogQuery::default()
            }),
        )
        .await
        .expect("list first request page");
        assert_eq!(
            first_page["requests"]
                .as_array()
                .expect("request array")
                .iter()
                .map(|request| request["id"].as_str().expect("request id"))
                .collect::<Vec<_>>(),
            ["log-new", "log-middle"]
        );

        let cursor = first_page["next_cursor"]
            .as_str()
            .expect("next request cursor")
            .to_owned();
        let Json(second_page) = list_requests(
            State(state.clone()),
            Query(RequestLogQuery {
                limit: Some(2),
                cursor: Some(cursor),
                ..RequestLogQuery::default()
            }),
        )
        .await
        .expect("list second request page");
        let request = &second_page["requests"]
            .as_array()
            .expect("second request array")[0];
        assert_eq!(request["id"], "log-old");
        assert_eq!(
            request["error"],
            "provider 'provider' returned HTTP 502: upstream unavailable"
        );
    }
}
