use super::keys::{
    ProviderKeyPageQuery, ProviderUsageAccountView, ProviderUsageKey, credential_type_for_record,
    decode_provider_key_cursor, encode_provider_key_cursor, unix_time_millis, usage_budget,
};
use super::*;
use crate::infra::storage::{Record, Table};
use futures_util::stream::{self, StreamExt};

pub(crate) async fn provider_usage(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ProviderKeyPageQuery>,
) -> ApiResult {
    provider_usage_response(state, id, query.cursor.as_deref(), false).await
}

pub(crate) async fn refresh_provider_usage(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ProviderKeyPageQuery>,
) -> ApiResult {
    provider_usage_response(state, id, query.cursor.as_deref(), true).await
}

async fn provider_usage_response(
    state: AppState,
    provider_id: String,
    cursor_value: Option<&str>,
    force: bool,
) -> ApiResult {
    let cursor = cursor_value.map(decode_provider_key_cursor).transpose()?;
    let provider_id_owned = provider_id.clone();
    let after = cursor
        .as_ref()
        .map(|position| crate::infra::db::provider_api_key_order_key(&provider_id, position))
        .transpose()
        .map_err(internal)?;
    let (adapter_id, base_url, rows) = state
        .db
        .read(move |transaction| {
            let Some(provider) = transaction.get::<Record>(Table::Providers, &provider_id_owned)?
            else {
                return Ok((None, None, Vec::new()));
            };
            let adapter_id = provider.text("adapter_id")?.to_owned();
            let base_url = provider.text("base_url")?.to_owned();
            let prefix = crate::infra::db::provider_api_key_created_prefix(&provider_id_owned)?;
            let ids = transaction.scan_prefix_after::<String>(
                Table::ProviderApiKeyCreatedIndex,
                &prefix,
                after.as_deref(),
                DEFAULT_PROVIDER_KEY_PAGE_SIZE.saturating_add(1),
            )?;
            let mut rows = Vec::with_capacity(ids.len());
            for (_, id) in ids {
                if let Some(record) = transaction.get::<Record>(Table::ProviderApiKeys, &id)? {
                    rows.push(record);
                }
            }
            Ok((Some(adapter_id), Some(base_url), rows))
        })
        .await
        .map_err(internal)?;
    let adapter_id = adapter_id.ok_or_else(|| fail(StatusCode::NOT_FOUND, "provider not found"))?;
    let base_url = base_url.ok_or_else(|| fail(StatusCode::NOT_FOUND, "provider not found"))?;
    if !provider_adapters::capabilities(&adapter_id).is_some_and(|capabilities| {
        capabilities.usage_limits || capabilities.api_key_usage || capabilities.local_usage_meter
    }) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "this provider adapter does not expose usage data",
        ));
    }
    let supports_api_key_usage = provider_adapters::capabilities(&adapter_id)
        .is_some_and(|capabilities| capabilities.api_key_usage);

    let has_more = rows.len() > DEFAULT_PROVIDER_KEY_PAGE_SIZE;
    let page_rows = rows
        .into_iter()
        .take(DEFAULT_PROVIDER_KEY_PAGE_SIZE)
        .collect::<Vec<_>>();
    let next_cursor = if has_more {
        page_rows
            .last()
            .map(|row| {
                Ok::<String, (StatusCode, Json<Value>)>(encode_provider_key_cursor(
                    &provider_api_key_order_position(row).map_err(internal)?,
                ))
            })
            .transpose()?
    } else {
        None
    };
    let keys = page_rows
        .into_iter()
        .map(|row| {
            let credential_type =
                credential_type_for_record(&row, Some(&adapter_id)).map_err(internal)?;
            Ok(ProviderUsageKey {
                id: row.text("id").map_err(internal)?.to_owned(),
                name: row.text("name").map_err(internal)?.to_owned(),
                credential_type,
                encrypted_secret: (supports_api_key_usage && credential_type == "api_key")
                    .then(|| row.bytes("secret").map(|value| value.to_vec()))
                    .transpose()
                    .map_err(internal)?,
                usage_budget_5h_micros: usage_budget(&row, "usage_budget_5h_micros")
                    .map_err(internal)?,
                usage_budget_7d_micros: usage_budget(&row, "usage_budget_7d_micros")
                    .map_err(internal)?,
                usage_budget_30d_micros: usage_budget(&row, "usage_budget_30d_micros")
                    .map_err(internal)?,
                enabled: row.boolean("enabled").map_err(internal)?,
                invalid: row.boolean("invalid").map_err(internal)?,
            })
        })
        .collect::<Result<Vec<_>, (StatusCode, Json<Value>)>>()?;

    let accounts = stream::iter(keys.into_iter().map(|key| {
        let state = state.clone();
        let adapter_id = adapter_id.clone();
        let base_url = base_url.clone();
        async move {
            let mut key = key;
            let encrypted_secret = key.encrypted_secret.take().unwrap_or_default();
            if supports_api_key_usage && key.credential_type == "api_key" {
                provider_api_key_usage_for_key(
                    &state,
                    &adapter_id,
                    &base_url,
                    encrypted_secret,
                    key,
                    force,
                )
                .await
            } else {
                provider_usage_for_key(&state, &adapter_id, key, force).await
            }
        }
    }))
    .buffer_unordered(4)
    .collect::<Vec<_>>()
    .await;
    Ok(Json(json!({"accounts":accounts,"next_cursor":next_cursor})))
}

pub(crate) async fn provider_key_usage(
    State(state): State<AppState>,
    Path((provider_id, key_id)): Path<(String, String)>,
) -> ApiResult {
    provider_key_usage_response(state, provider_id, key_id, false).await
}

pub(crate) async fn refresh_provider_key_usage(
    State(state): State<AppState>,
    Path((provider_id, key_id)): Path<(String, String)>,
) -> ApiResult {
    provider_key_usage_response(state, provider_id, key_id, true).await
}

pub(super) async fn provider_key_usage_response(
    state: AppState,
    provider_id: String,
    key_id: String,
    force: bool,
) -> ApiResult {
    let provider_id_for_read = provider_id.clone();
    let key_id_for_read = key_id.clone();
    let result = state
        .db
        .read(move |transaction| {
            let Some(provider) =
                transaction.get::<Record>(Table::Providers, &provider_id_for_read)?
            else {
                return Ok(None);
            };
            let Some(key) = transaction.get::<Record>(Table::ProviderApiKeys, &key_id_for_read)?
            else {
                return Ok(None);
            };
            if key.text("provider_id")? != provider_id_for_read {
                return Ok(None);
            }
            Ok(Some((provider, key)))
        })
        .await
        .map_err(internal)?
        .ok_or_else(|| fail(StatusCode::NOT_FOUND, "provider API key not found"))?;
    let (provider, row) = result;
    let adapter_id = provider.text("adapter_id").map_err(internal)?.to_owned();
    if !provider_adapters::capabilities(&adapter_id)
        .is_some_and(|capabilities| capabilities.api_key_usage || capabilities.local_usage_meter)
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "this provider adapter does not expose API-key usage data",
        ));
    }
    let base_url = provider.text("base_url").map_err(internal)?.to_owned();
    let key = ProviderUsageKey {
        id: row.text("id").map_err(internal)?.to_owned(),
        name: row.text("name").map_err(internal)?.to_owned(),
        credential_type: credential_type_for_record(&row, Some(&adapter_id)).map_err(internal)?,
        encrypted_secret: None,
        usage_budget_5h_micros: usage_budget(&row, "usage_budget_5h_micros").map_err(internal)?,
        usage_budget_7d_micros: usage_budget(&row, "usage_budget_7d_micros").map_err(internal)?,
        usage_budget_30d_micros: usage_budget(&row, "usage_budget_30d_micros").map_err(internal)?,
        enabled: row.boolean("enabled").map_err(internal)?,
        invalid: row.boolean("invalid").map_err(internal)?,
    };
    let encrypted_secret = row.bytes("secret").map_err(internal)?.to_vec();
    Ok(Json(json!({
        "account": provider_api_key_usage_for_key(
            &state,
            &adapter_id,
            &base_url,
            encrypted_secret,
            key,
            force,
        )
        .await
    })))
}

pub(super) async fn provider_api_key_usage_for_key(
    state: &AppState,
    adapter_id: &str,
    base_url: &str,
    encrypted_secret: Vec<u8>,
    key: ProviderUsageKey,
    force: bool,
) -> ProviderUsageAccountView {
    if provider_adapters::capabilities(adapter_id)
        .is_some_and(|capabilities| capabilities.local_usage_meter)
    {
        return provider_local_meter_for_key(state, key).await;
    }
    let disabled = !key.enabled;
    if key.invalid {
        return cached_usage_view(
            state,
            key,
            if disabled {
                "disabled"
            } else {
                "reauth_required"
            },
            Some(if disabled {
                "This provider key is disabled.".to_owned()
            } else {
                "This provider key is marked invalid.".to_owned()
            }),
        )
        .await;
    }

    // Serialize quota refreshes for the same key so concurrent dashboard reads
    // reuse one upstream response; dropping the future releases this RAII guard.
    let lock = state.provider_usage_lock(&key.id).await;
    let _usage_guard = lock.lock().await;
    let cache_epoch = state.provider_usage_cache_epoch();
    let previous = state.cached_provider_usage(&key.id).await;
    let now = std::time::Instant::now();
    if !force && let Some(entry) = previous.as_ref() {
        let cache_ttl = provider_usage_cache_ttl(adapter_id);
        if entry.snapshot.is_some()
            && entry.last_error.is_none()
            && entry
                .fetched_at
                .is_some_and(|fetched| now.saturating_duration_since(fetched) < cache_ttl)
        {
            return usage_view(
                &key,
                entry,
                if disabled { "disabled" } else { "fresh" },
                disabled.then(|| "This provider key is disabled.".to_owned()),
            );
        }
        if entry.last_error.is_some()
            && now.saturating_duration_since(entry.last_attempt_at) < Duration::from_secs(30)
        {
            return cached_usage_view(
                state,
                key,
                if disabled { "disabled" } else { "" },
                disabled.then(|| "This provider key is disabled.".to_owned()),
            )
            .await;
        }
    }

    let _permit = match state
        .admin
        .admin_provider_usage_in_flight
        .clone()
        .acquire_owned()
        .await
    {
        Ok(permit) => permit,
        Err(_) => {
            return cached_usage_view(
                state,
                key,
                "",
                Some("Usage checks are temporarily unavailable.".to_owned()),
            )
            .await;
        }
    };
    let result = match decrypt_secret(state.config.master_key.as_ref(), Some(&encrypted_secret)) {
        Ok(Some(credential)) => {
            provider_adapters::fetch_api_key_usage(adapter_id, state, base_url, &credential).await
        }
        Ok(None) => Err("provider API key is empty".to_owned()),
        Err(_) => {
            Err("could not decrypt provider credentials; check EXOROUTE_MASTER_KEY".to_owned())
        }
    };
    let attempted_at = std::time::Instant::now();
    match result {
        Ok(snapshot) => {
            let fetched_at_ms = unix_time_millis();
            let snapshot = serde_json::to_value(snapshot).ok();
            let entry = crate::state::ProviderUsageCacheEntry {
                snapshot,
                fetched_at_ms: Some(fetched_at_ms),
                fetched_at: Some(attempted_at),
                last_attempt_at: attempted_at,
                last_error: None,
            };
            state
                .cache_provider_usage(key.id.clone(), entry.clone(), cache_epoch)
                .await;
            usage_view(
                &key,
                &entry,
                if disabled { "disabled" } else { "fresh" },
                disabled.then(|| "This provider key is disabled.".to_owned()),
            )
        }
        Err(error) => {
            let needs_reauth = error.contains("rejected this API key");
            let entry = crate::state::ProviderUsageCacheEntry {
                snapshot: previous.as_ref().and_then(|entry| entry.snapshot.clone()),
                fetched_at_ms: previous.as_ref().and_then(|entry| entry.fetched_at_ms),
                fetched_at: previous.as_ref().and_then(|entry| entry.fetched_at),
                last_attempt_at: attempted_at,
                last_error: Some(error.clone()),
            };
            state
                .cache_provider_usage(key.id.clone(), entry.clone(), cache_epoch)
                .await;
            let status = if disabled {
                "disabled"
            } else if needs_reauth {
                "reauth_required"
            } else if entry.snapshot.is_some() {
                "stale"
            } else {
                "unavailable"
            };
            usage_view(
                &key,
                &entry,
                status,
                Some(if disabled {
                    "This provider key is disabled.".to_owned()
                } else {
                    error
                }),
            )
        }
    }
}

async fn provider_usage_for_key(
    state: &AppState,
    adapter_id: &str,
    key: ProviderUsageKey,
    force: bool,
) -> ProviderUsageAccountView {
    if provider_adapters::capabilities(adapter_id)
        .is_some_and(|capabilities| capabilities.local_usage_meter)
    {
        return provider_local_meter_for_key(state, key).await;
    }
    let disabled = !key.enabled;
    if key.invalid {
        return cached_usage_view(
            state,
            key,
            if disabled {
                "disabled"
            } else {
                "reauth_required"
            },
            Some(if disabled {
                "This provider account is disabled.".to_owned()
            } else {
                "OpenAI Codex account needs to be reconnected.".to_owned()
            }),
        )
        .await;
    }

    let lock = state.provider_usage_lock(&key.id).await;
    let _usage_guard = lock.lock().await;
    let cache_epoch = state.provider_usage_cache_epoch();
    let previous = state.cached_provider_usage(&key.id).await;
    let now = std::time::Instant::now();
    if !force && let Some(entry) = previous.as_ref() {
        let cache_ttl = provider_usage_cache_ttl(adapter_id);
        if entry.snapshot.is_some()
            && entry.last_error.is_none()
            && entry
                .fetched_at
                .is_some_and(|fetched| now.saturating_duration_since(fetched) < cache_ttl)
        {
            return usage_view(
                &key,
                entry,
                if disabled { "disabled" } else { "fresh" },
                disabled
                    .then(|| "This provider account is disabled.".to_owned())
                    .or_else(|| entry.last_error.clone()),
            );
        }
        if entry.last_error.is_some()
            && now.saturating_duration_since(entry.last_attempt_at) < Duration::from_secs(30)
        {
            return cached_usage_view(
                state,
                key,
                if disabled { "disabled" } else { "" },
                disabled.then(|| "This provider account is disabled.".to_owned()),
            )
            .await;
        }
    }

    let _permit = match state
        .admin
        .admin_provider_usage_in_flight
        .clone()
        .acquire_owned()
        .await
    {
        Ok(permit) => permit,
        Err(_) => {
            return cached_usage_view(
                state,
                key,
                "",
                Some("Usage checks are temporarily unavailable.".to_owned()),
            )
            .await;
        }
    };
    let result = provider_adapters::fetch_oauth_usage(adapter_id, state, &key.id).await;
    let attempted_at = std::time::Instant::now();
    match result {
        Ok(snapshot) => {
            let fetched_at_ms = unix_time_millis();
            let snapshot = serde_json::to_value(snapshot).ok();
            let entry = crate::state::ProviderUsageCacheEntry {
                snapshot,
                fetched_at_ms: Some(fetched_at_ms),
                fetched_at: Some(attempted_at),
                last_attempt_at: attempted_at,
                last_error: None,
            };
            state
                .cache_provider_usage(key.id.clone(), entry.clone(), cache_epoch)
                .await;
            usage_view(
                &key,
                &entry,
                if disabled { "disabled" } else { "fresh" },
                disabled.then(|| "This provider account is disabled.".to_owned()),
            )
        }
        Err(error) => {
            let needs_reauth =
                error.contains("reconnected") || error.contains("reconnect the account");
            let entry = crate::state::ProviderUsageCacheEntry {
                snapshot: previous.as_ref().and_then(|entry| entry.snapshot.clone()),
                fetched_at_ms: previous.as_ref().and_then(|entry| entry.fetched_at_ms),
                fetched_at: previous.as_ref().and_then(|entry| entry.fetched_at),
                last_attempt_at: attempted_at,
                last_error: Some(error.clone()),
            };
            state
                .cache_provider_usage(key.id.clone(), entry.clone(), cache_epoch)
                .await;
            let status = if disabled {
                "disabled"
            } else if needs_reauth {
                "reauth_required"
            } else if entry.snapshot.is_some() {
                "stale"
            } else {
                "unavailable"
            };
            usage_view(
                &key,
                &entry,
                status,
                Some(if disabled {
                    "This provider account is disabled.".to_owned()
                } else {
                    error
                }),
            )
        }
    }
}

async fn provider_local_meter_for_key(
    state: &AppState,
    key: ProviderUsageKey,
) -> ProviderUsageAccountView {
    let (mut local_meter, meter_error) =
        match crate::infra::telemetry::provider_usage_meter_snapshot(&state.db, &key.id).await {
            Ok(value) => (value, None),
            Err(_) => (
                json!({
                    "source": "exoroute_local_meter",
                    "status": "unknown",
                    "windows": {}
                }),
                Some("Local cost meter is temporarily unavailable.".to_owned()),
            ),
        };
    let meter_status = local_meter.get("status").and_then(Value::as_str);
    let meter_read_ok = meter_error.is_none();
    let status = if !key.enabled {
        "disabled"
    } else if key.invalid {
        "reauth_required"
    } else if meter_error.is_some() {
        "unknown"
    } else {
        match meter_status {
            Some("fresh") => "fresh",
            Some("stale") => "stale",
            Some("partial") => "partial",
            Some("unknown") => "unknown",
            _ => "unknown",
        }
    };
    if let Some(object) = local_meter.as_object_mut() {
        object.insert(
            "budget_usd".to_owned(),
            json!({
                "5h": key.usage_budget_5h_micros.map(|value| value as f64 / 1_000_000.0),
                "7d": key.usage_budget_7d_micros.map(|value| value as f64 / 1_000_000.0),
                "30d": key.usage_budget_30d_micros.map(|value| value as f64 / 1_000_000.0),
            }),
        );
        object.insert(
            "credential_type".to_owned(),
            Value::String(key.credential_type.to_owned()),
        );
    }
    let message = if key.invalid {
        Some("Cline account must be reconnected.".to_owned())
    } else {
        meter_error
    };
    ProviderUsageAccountView {
        key_id: key.id,
        name: key.name,
        status,
        snapshot: None,
        provider_quota: json!({
            "status": "unsupported",
            "source": "cline_api",
            "message": "Cline does not publish a public quota API."
        }),
        local_meter: Some(local_meter),
        fetched_at_ms: meter_read_ok.then(unix_time_millis),
        message,
    }
}

async fn cached_usage_view(
    state: &AppState,
    key: ProviderUsageKey,
    fallback_status: &'static str,
    message: Option<String>,
) -> ProviderUsageAccountView {
    match state.cached_provider_usage(&key.id).await {
        Some(entry) => {
            let status = if fallback_status.is_empty() {
                if entry.snapshot.is_some() {
                    "stale"
                } else {
                    "unavailable"
                }
            } else {
                fallback_status
            };
            usage_view(
                &key,
                &entry,
                status,
                message.or_else(|| entry.last_error.clone()),
            )
        }
        None => ProviderUsageAccountView {
            key_id: key.id,
            name: key.name,
            status: if fallback_status.is_empty() {
                "unavailable"
            } else {
                fallback_status
            },
            snapshot: None,
            provider_quota: json!({
                "status": "unknown",
                "source": "upstream",
                "message": "No upstream quota snapshot is available."
            }),
            local_meter: None,
            fetched_at_ms: None,
            message,
        },
    }
}

fn usage_view(
    key: &ProviderUsageKey,
    entry: &crate::state::ProviderUsageCacheEntry,
    status: &'static str,
    message: Option<String>,
) -> ProviderUsageAccountView {
    ProviderUsageAccountView {
        key_id: key.id.clone(),
        name: key.name.clone(),
        status,
        snapshot: entry.snapshot.clone(),
        provider_quota: json!({
            "status": if entry.snapshot.is_some() { "available" } else { "unknown" },
            "source": "upstream",
            "snapshot": entry.snapshot.clone()
        }),
        local_meter: None,
        fetched_at_ms: entry.fetched_at_ms,
        message,
    }
}

fn provider_usage_cache_ttl(adapter_id: &str) -> Duration {
    if matches!(
        adapter_id,
        provider_adapters::OPENCODE_GO_ADAPTER_ID | provider_adapters::ANTIGRAVITY_ADAPTER_ID
    ) {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(5 * 60)
    }
}
