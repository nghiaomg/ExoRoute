use super::*;
use crate::infra::storage::{Field, Record, StorageError, Table};

#[derive(Debug, Deserialize)]
pub(crate) struct ProviderKeyInput {
    pub(super) name: Option<String>,
    pub(super) api_key: String,
}

#[derive(Debug, Serialize)]
struct ProviderKeyView {
    id: String,
    name: String,
    credential_type: &'static str,
    enabled: bool,
    invalid: bool,
    usage_budget_5h_micros: Option<i64>,
    usage_budget_7d_micros: Option<i64>,
    usage_budget_30d_micros: Option<i64>,
    last_error: Option<String>,
    last_test_passed: Option<bool>,
    last_test_status: Option<i64>,
    created_at: String,
    last_tested_at: Option<String>,
    last_used_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProviderUsageAccountView {
    pub(super) key_id: String,
    pub(super) name: String,
    pub(super) status: &'static str,
    pub(super) snapshot: Option<Value>,
    pub(super) provider_quota: Value,
    pub(super) local_meter: Option<Value>,
    pub(super) fetched_at_ms: Option<i64>,
    pub(super) message: Option<String>,
}

pub(crate) struct ProviderUsageKey {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) credential_type: &'static str,
    pub(super) encrypted_secret: Option<Vec<u8>>,
    pub(super) usage_budget_5h_micros: Option<i64>,
    pub(super) usage_budget_7d_micros: Option<i64>,
    pub(super) usage_budget_30d_micros: Option<i64>,
    pub(super) enabled: bool,
    pub(super) invalid: bool,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ProviderKeyPageQuery {
    pub(super) cursor: Option<String>,
    pub(super) limit: Option<usize>,
}

pub(crate) async fn list_provider_keys(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ProviderKeyPageQuery>,
) -> ApiResult {
    let limit = query.limit.unwrap_or(DEFAULT_PROVIDER_KEY_PAGE_SIZE);
    if !(1..=MAX_PROVIDER_KEY_PAGE_SIZE).contains(&limit) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider key page size must be between 1 and 50",
        ));
    }
    let cursor = query
        .cursor
        .as_deref()
        .map(decode_provider_key_cursor)
        .transpose()?;
    let provider_id = id.clone();
    let after = cursor
        .as_ref()
        .map(|position| crate::infra::db::provider_api_key_order_key(&id, position))
        .transpose()
        .map_err(internal)?;
    let (provider_exists, adapter_id, rows) = state
        .db
        .read(move |transaction| {
            let Some(provider) = transaction.get::<Record>(Table::Providers, &provider_id)? else {
                return Ok((false, None, Vec::new()));
            };
            let adapter_id = provider.text("adapter_id")?.to_owned();
            let prefix = crate::infra::db::provider_api_key_created_prefix(&provider_id)?;
            let index_rows = transaction.scan_prefix_after::<String>(
                Table::ProviderApiKeyCreatedIndex,
                &prefix,
                after.as_deref(),
                limit.saturating_add(1),
            )?;
            let mut rows = Vec::with_capacity(index_rows.len());
            for (_, key_id) in index_rows {
                if let Some(record) = transaction.get::<Record>(Table::ProviderApiKeys, &key_id)? {
                    rows.push(record);
                }
            }
            Ok((true, Some(adapter_id), rows))
        })
        .await
        .map_err(internal)?;
    if !provider_exists {
        return Err(fail(StatusCode::NOT_FOUND, "provider not found"));
    }
    let adapter_id = adapter_id.as_deref();
    let has_more = rows.len() > limit;
    let page_rows = rows.into_iter().take(limit).collect::<Vec<_>>();
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
            let passed = match row.field("last_test_passed") {
                Some(Field::Bool(value)) => Some(*value),
                Some(Field::Null) | None => None,
                _ => {
                    return Err(internal(StorageError::Invalid(
                        "provider key test state is malformed".to_owned(),
                    )));
                }
            };
            Ok(ProviderKeyView {
                id: row.text("id").map_err(internal)?.to_owned(),
                name: row.text("name").map_err(internal)?.to_owned(),
                credential_type: credential_type_for_record(&row, adapter_id).map_err(internal)?,
                enabled: row.boolean("enabled").map_err(internal)?,
                invalid: row.boolean("invalid").map_err(internal)?,
                usage_budget_5h_micros: usage_budget(&row, "usage_budget_5h_micros")
                    .map_err(internal)?,
                usage_budget_7d_micros: usage_budget(&row, "usage_budget_7d_micros")
                    .map_err(internal)?,
                usage_budget_30d_micros: usage_budget(&row, "usage_budget_30d_micros")
                    .map_err(internal)?,
                last_error: row
                    .optional_text("last_error")
                    .map_err(internal)?
                    .map(str::to_owned),
                last_test_passed: passed,
                last_test_status: row.optional_integer("last_test_status").map_err(internal)?,
                created_at: row.text("created_at").map_err(internal)?.to_owned(),
                last_tested_at: row
                    .optional_text("last_tested_at")
                    .map_err(internal)?
                    .map(str::to_owned),
                last_used_at: row
                    .optional_text("last_used_at")
                    .map_err(internal)?
                    .map(str::to_owned),
            })
        })
        .collect::<Result<Vec<_>, (StatusCode, Json<Value>)>>()?;
    Ok(Json(json!({"keys":keys,"next_cursor":next_cursor})))
}

/// Encodes the opaque page cursor for provider key pages. The payload is the
/// ordering position of the last row on the page, so paging stays correct after
/// an operator reorders keys.
pub(crate) fn encode_provider_key_cursor(position: &str) -> String {
    let mut payload = Vec::with_capacity(2 + position.len());
    payload.extend_from_slice(&(position.len() as u16).to_be_bytes());
    payload.extend_from_slice(position.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload)
}

pub(crate) fn decode_provider_key_cursor(
    cursor: &str,
) -> Result<String, (StatusCode, Json<Value>)> {
    if cursor.len() > 2048 {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider key cursor is invalid",
        ));
    }
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "provider key cursor is invalid"))?;
    if decoded.len() < 3 {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider key cursor is invalid",
        ));
    }
    let position_len = u16::from_be_bytes([decoded[0], decoded[1]]) as usize;
    let position_end = 2usize.saturating_add(position_len);
    if position_len == 0 || position_end > decoded.len() {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider key cursor is invalid",
        ));
    }
    let position = std::str::from_utf8(&decoded[2..position_end])
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "provider key cursor is invalid"))?;
    if crate::infra::storage::validate_key(position).is_err() {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider key cursor is invalid",
        ));
    }
    Ok(position.to_owned())
}

pub(crate) fn credential_type_for_record(
    row: &Record,
    adapter_id: Option<&str>,
) -> Result<&'static str, StorageError> {
    let credential_type = match row.optional_text("credential_type")? {
        Some("oauth") => crate::provider_adapters::keys::ProviderCredentialType::OAuth,
        Some("api_key") => crate::provider_adapters::keys::ProviderCredentialType::ApiKey,
        Some(_) => Err(StorageError::Invalid(
            "provider credential type is invalid".to_owned(),
        ))?,
        None if adapter_id == Some(provider_adapters::CODEX_ADAPTER_ID) => {
            crate::provider_adapters::keys::ProviderCredentialType::OAuth
        }
        None => crate::provider_adapters::keys::ProviderCredentialType::ApiKey,
    };
    Ok(credential_type.as_str())
}

pub(crate) fn usage_budget(row: &Record, field: &str) -> Result<Option<i64>, StorageError> {
    let value = row.optional_integer(field)?;
    if value.is_some_and(|value| value <= 0) {
        return Err(StorageError::Invalid(
            "provider local usage budgets must be positive".to_owned(),
        ));
    }
    Ok(value)
}

pub(crate) fn unix_time_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

pub(crate) async fn create_provider_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderKeyInput>,
) -> ApiResult {
    let value =
        add_provider_key_with_id(&state, &id, input.name.as_deref(), &input.api_key, None).await?;
    Ok(Json(value))
}

pub(crate) async fn add_provider_key_with_id(
    state: &AppState,
    id: &str,
    requested_name: Option<&str>,
    api_key: &str,
    requested_key_id: Option<&str>,
) -> Result<Value, (StatusCode, Json<Value>)> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err(fail(StatusCode::BAD_REQUEST, "api_key is required"));
    }
    if api_key.len() > MAX_PROVIDER_KEY_BYTES {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider API keys may contain at most 4096 bytes",
        ));
    }
    let provider_id = id.to_owned();
    let existing_key_id = requested_key_id.map(str::to_owned);
    let (provider, existing) = state
        .db
        .read(move |transaction| {
            let provider = transaction.get::<Record>(Table::Providers, &provider_id)?;
            let existing = if let Some(key_id) = existing_key_id.as_deref() {
                transaction.get::<Record>(Table::ProviderApiKeys, key_id)?
            } else {
                None
            };
            Ok((provider, existing))
        })
        .await
        .map_err(internal)?;
    let provider = provider.ok_or_else(|| fail(StatusCode::NOT_FOUND, "provider not found"))?;
    ensure_provider_not_deleting(&provider).map_err(internal)?;
    let base_url = provider.text("base_url").map_err(internal)?.to_owned();
    let adapter_id = provider.text("adapter_id").map_err(internal)?.to_owned();
    let stored_auth_type = provider.text("auth_type").map_err(internal)?.to_owned();
    let auth_header = provider
        .optional_text("auth_header")
        .map_err(internal)?
        .map(str::to_owned);
    let custom_headers = super::stored_custom_headers(&provider, state.config.master_key.as_ref())
        .map_err(|error| fail(StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let preferred_protocol = provider
        .text("preferred_protocol")
        .map_err(internal)?
        .to_owned();
    if !provider_adapters::capabilities(&adapter_id)
        .is_some_and(|capabilities| capabilities.api_keys)
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "this provider does not accept API keys",
        ));
    }
    let key_id = requested_key_id
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if key_id.is_empty() || key_id.len() > MAX_PROVIDER_ID_BYTES {
        return Err(fail(StatusCode::BAD_REQUEST, "provider key ID is invalid"));
    }
    if let Some(existing) = existing {
        let existing_provider = existing.text("provider_id").map_err(internal)?;
        if existing_provider != id {
            return Err(fail(StatusCode::CONFLICT, "provider key ID already exists"));
        }
        let passed = match existing.field("last_test_passed") {
            Some(Field::Bool(value)) => *value,
            Some(Field::Null) | None => false,
            _ => {
                return Err(internal(StorageError::Invalid(
                    "provider key test state is malformed".to_owned(),
                )));
            }
        };
        let status = existing
            .optional_integer("last_test_status")
            .map_err(internal)?;
        let warning = existing.optional_text("last_error").map_err(internal)?;
        return Ok(json!({
            "ok": true,
            "id": key_id,
            "test_passed": passed,
            "status": status,
            "warning": warning,
        }));
    }
    // Adding the first key to an unauthenticated provider conventionally turns
    // on bearer auth; custom-header providers already have their header saved.
    let auth_type = if stored_auth_type == "none" {
        "bearer"
    } else {
        stored_auth_type.as_str()
    };
    let test = test_provider_credential(
        &adapter_id,
        provider_adapters::AdapterApiKeyRequest {
            state,
            base_url: &base_url,
            auth_type,
            auth_header: auth_header.as_deref(),
            custom_headers: &custom_headers,
            preferred_protocol: &preferred_protocol,
            credential: api_key,
        },
    )
    .await;
    let secret = encrypt_secret(state.config.master_key.as_ref(), api_key)
        .map_err(|error| fail(StatusCode::BAD_REQUEST, error))?
        .ok_or_else(|| fail(StatusCode::BAD_REQUEST, "api_key is required"))?;
    let name = requested_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| "Added key".to_owned());
    if name.len() > MAX_PROVIDER_NAME_BYTES {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider API key names may contain at most 256 bytes",
        ));
    }
    let timestamp = crate::infra::db::utc_timestamp_now().map_err(internal)?;
    let record = provider_api_key_record(ProviderApiKeyRecordData {
        id: &key_id,
        provider_id: id,
        name: &name,
        secret: &secret,
        last_error: test.message.as_deref().filter(|_| !test.test_passed),
        last_test_passed: test.test_passed,
        last_test_status: test.status.map(i64::from),
        tested_at: &timestamp,
        created_at: &timestamp,
    });
    let provider_id = id.to_owned();
    let key_id_owned = key_id.clone();
    let stored_auth_type = stored_auth_type.clone();
    let inserted = state
        .db
        .write(move |transaction| {
            let mut provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            ensure_provider_not_deleting(&provider)?;
            if let Some(existing) =
                transaction.get::<Record>(Table::ProviderApiKeys, &key_id_owned)?
            {
                if existing.text("provider_id")? != provider_id {
                    return Err(StorageError::Conflict);
                }
                return Ok(false);
            }
            if stored_auth_type == "none" {
                provider.insert("auth_type", Field::Text("bearer".to_owned()));
                provider.insert("updated_at", Field::Text(timestamp.clone()));
            }
            let count = provider
                .integer("api_key_count")?
                .checked_add(1)
                .ok_or_else(|| {
                    StorageError::Invalid("provider API key count overflowed".to_owned())
                })?;
            provider.insert("api_key_count", Field::I64(count));
            transaction.put(Table::Providers, &provider_id, &provider)?;
            store_provider_api_key(transaction, &record)?;
            Ok(true)
        })
        .await
        .map_err(|error| match error {
            StorageError::Conflict => fail(
                StatusCode::CONFLICT,
                "provider is being deleted or provider key ID already exists",
            ),
            StorageError::NotFound => fail(StatusCode::NOT_FOUND, "provider not found"),
            other => internal(other),
        })?;
    if !inserted {
        return Ok(
            json!({"ok":true,"id":key_id,"test_passed":test.test_passed,"status":test.status,"warning":test.warning}),
        );
    }
    Ok(
        json!({"ok":true,"id":key_id,"test_passed":test.test_passed,"status":test.status,"warning":test.warning}),
    )
}

pub(crate) async fn delete_provider_key(
    State(state): State<AppState>,
    Path((id, key_id)): Path<(String, String)>,
) -> ApiResult {
    let deleted = state
        .db
        .write(move |transaction| {
            let Some(record) = transaction.get::<Record>(Table::ProviderApiKeys, &key_id)? else {
                return Ok(false);
            };
            if record.text("provider_id")? != id {
                return Ok(false);
            }
            let provider = transaction
                .get::<Record>(Table::Providers, &id)?
                .ok_or(StorageError::NotFound)?;
            ensure_provider_not_deleting(&provider)?;
            let (all_index, created_index, _) = provider_api_key_index_keys(&record)?;
            let record_id = record.text("id")?.to_owned();
            transaction.delete(Table::ProviderApiKeys, &record_id)?;
            transaction.delete_prefix(Table::ProviderUsageMeters, &format!("{record_id}/"))?;
            transaction.delete(Table::ProviderApiKeyIndex, &all_index)?;
            transaction.delete(Table::ProviderApiKeyCreatedIndex, &created_index)?;
            if let Some(identity_index) = provider_api_key_identity_index_key(&record)? {
                transaction.delete(Table::ProviderApiKeyIndex, &identity_index)?;
            }
            if record.boolean("enabled")? && !record.boolean("invalid")? {
                let available =
                    crate::infra::db::provider_api_key_index_key(&id, record.text("id")?, true)?;
                transaction.delete(Table::ProviderApiKeyAvailabilityIndex, &available)?;
            }
            let mut provider = provider;
            provider.insert(
                "api_key_count",
                Field::I64(provider.integer("api_key_count")?.saturating_sub(1).max(0)),
            );
            if record.boolean("invalid")? {
                provider.insert(
                    "invalid_api_key_count",
                    Field::I64(
                        provider
                            .integer("invalid_api_key_count")?
                            .saturating_sub(1)
                            .max(0),
                    ),
                );
            }
            transaction.put(Table::Providers, &id, &provider)?;
            Ok(true)
        })
        .await
        .map_err(internal)?;
    if !deleted {
        return Err(fail(StatusCode::NOT_FOUND, "provider API key not found"));
    }
    state.invalidate_provider_usage_cache().await;
    Ok(Json(json!({"ok":true})))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderKeyUpdateInput {
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) enabled: Option<bool>,
}

pub(crate) async fn update_provider_key(
    State(state): State<AppState>,
    Path((id, key_id)): Path<(String, String)>,
    Json(input): Json<ProviderKeyUpdateInput>,
) -> ApiResult {
    let name = match input.name.as_deref().map(str::trim) {
        Some("") => {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "provider API key name is required",
            ));
        }
        Some(value) if value.len() > MAX_PROVIDER_NAME_BYTES => {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "provider API key names may contain at most 256 bytes",
            ));
        }
        Some(value) => Some(value.to_owned()),
        None => None,
    };
    let enabled = input.enabled;
    if name.is_none() && enabled.is_none() {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provide a provider key name or enabled state",
        ));
    }
    let provider_id = id.clone();
    let record_id = key_id.clone();
    let updated = state
        .db
        .write(move |transaction| {
            let Some(mut record) = transaction.get::<Record>(Table::ProviderApiKeys, &record_id)?
            else {
                return Ok(None);
            };
            if record.text("provider_id")? != provider_id {
                return Ok(None);
            }
            let provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            ensure_provider_not_deleting(&provider)?;
            let was_available = record.boolean("enabled")? && !record.boolean("invalid")?;
            if let Some(name) = name.as_deref() {
                record.insert("name", Field::Text(name.to_owned()));
            }
            if let Some(enabled) = enabled {
                record.insert("enabled", Field::Bool(enabled));
            }
            let is_available = record.boolean("enabled")? && !record.boolean("invalid")?;
            transaction.put(Table::ProviderApiKeys, &record_id, &record)?;
            if was_available != is_available {
                let available =
                    crate::infra::db::provider_api_key_index_key(&provider_id, &record_id, true)?;
                if is_available {
                    transaction.put(
                        Table::ProviderApiKeyAvailabilityIndex,
                        &available,
                        &record_id,
                    )?;
                } else {
                    transaction.delete(Table::ProviderApiKeyAvailabilityIndex, &available)?;
                }
            }
            Ok(Some((
                record.text("name")?.to_owned(),
                record.boolean("enabled")?,
            )))
        })
        .await
        .map_err(|error| match error {
            StorageError::NotFound => fail(StatusCode::NOT_FOUND, "provider not found"),
            StorageError::Conflict => fail(StatusCode::CONFLICT, "provider is being deleted"),
            other => internal(other),
        })?;
    let Some((name, enabled)) = updated else {
        return Err(fail(StatusCode::NOT_FOUND, "provider API key not found"));
    };
    // Toggling or renaming a credential does not change the upstream quota
    // snapshot. Keep the process-local cache so a disabled account can still
    // render its last known quota while the next read refreshes it.
    Ok(Json(
        json!({"ok":true,"id":key_id,"name":name,"enabled":enabled}),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderKeyOrderInput {
    pub(super) swap_with: String,
}

/// Swaps the fallback positions of two keys of the same provider. The dashboard
/// sends the adjacent key when an operator moves a key up or down; the stored
/// position keeps the swap stable when two keys share a creation timestamp.
pub(crate) async fn swap_provider_key_order(
    State(state): State<AppState>,
    Path((id, key_id)): Path<(String, String)>,
    Json(input): Json<ProviderKeyOrderInput>,
) -> ApiResult {
    let other_id = input.swap_with.trim().to_owned();
    if other_id.is_empty() || other_id.len() > MAX_PROVIDER_ID_BYTES || other_id == key_id {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider key order swap needs two different keys",
        ));
    }
    let provider_id = id.clone();
    let first_id = key_id.clone();
    let swapped_with = other_id.clone();
    let swapped = state
        .db
        .write(move |transaction| {
            let Some(mut first) = transaction.get::<Record>(Table::ProviderApiKeys, &first_id)?
            else {
                return Ok(false);
            };
            let Some(mut second) =
                transaction.get::<Record>(Table::ProviderApiKeys, &swapped_with)?
            else {
                return Ok(false);
            };
            if first.text("provider_id")? != provider_id
                || second.text("provider_id")? != provider_id
            {
                return Ok(false);
            }
            let provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            ensure_provider_not_deleting(&provider)?;
            let first_position = provider_api_key_order_position(&first)?;
            let second_position = provider_api_key_order_position(&second)?;
            if first_position == second_position {
                return Err(StorageError::Invalid(
                    "provider key positions are not unique".to_owned(),
                ));
            }
            first.insert("order_position", Field::Text(second_position.clone()));
            second.insert("order_position", Field::Text(first_position.clone()));
            transaction.put(Table::ProviderApiKeys, &first_id, &first)?;
            transaction.put(Table::ProviderApiKeys, &swapped_with, &second)?;
            transaction.put(
                Table::ProviderApiKeyCreatedIndex,
                &crate::infra::db::provider_api_key_order_key(&provider_id, &first_position)?,
                &swapped_with,
            )?;
            transaction.put(
                Table::ProviderApiKeyCreatedIndex,
                &crate::infra::db::provider_api_key_order_key(&provider_id, &second_position)?,
                &first_id,
            )?;
            Ok(true)
        })
        .await
        .map_err(|error| match error {
            StorageError::NotFound => fail(StatusCode::NOT_FOUND, "provider not found"),
            StorageError::Conflict => fail(StatusCode::CONFLICT, "provider is being deleted"),
            other => internal(other),
        })?;
    if !swapped {
        return Err(fail(StatusCode::NOT_FOUND, "provider API key not found"));
    }
    state.invalidate_provider_usage_cache().await;
    Ok(Json(json!({"ok":true,"id":key_id,"swapped_with":other_id})))
}
