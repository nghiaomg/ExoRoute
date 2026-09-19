use super::super::{ApiResult, fail, internal};
use super::{
    ProviderRecordData,
    custom_headers::{custom_headers_for_input, encrypt_custom_headers, stored_custom_headers},
    ensure_provider_not_deleting, notify_enabled_provider_count_changed,
    provider_keys::{ProviderApiKeyRecordData, provider_api_key_record, store_provider_api_key},
    provider_model_prefix_value, provider_name_index_key, provider_prefix_index_key,
    provider_record,
    provider_view::{ProviderInput, ProviderKeyStrategyInput, ProviderThinkingSettingsInput},
    validation::{
        DEFAULT_KEY_STRATEGY, KEY_STRATEGY_ROUND_ROBIN, effective_adapter_id,
        normalized_provider_logo_url, normalized_provider_model_prefix_for_adapter, provider_keys,
        stored_key_strategy, stored_thinking_settings, updated_local_rpm_target, validate_provider,
    },
};
use crate::{
    infra::storage::{Field, Record, StorageError, Table},
    provider_adapters,
    security::encrypt_secret,
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde_json::json;

pub(crate) async fn update_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderInput>,
) -> ApiResult {
    if id != input.id {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "path id must match provider id",
        ));
    }
    let adapter_id = effective_adapter_id(&input);
    validate_provider(
        &input,
        &adapter_id,
        state.config.allow_private_provider_urls,
    )?;
    let provider_id = id.clone();
    let provider_state = state
        .db
        .read(move |transaction| transaction.get::<Record>(Table::Providers, &provider_id))
        .await
        .map_err(internal)?
        .ok_or_else(|| fail(StatusCode::NOT_FOUND, "provider not found"))?;
    ensure_provider_not_deleting(&provider_state).map_err(internal)?;
    let existing_custom_headers =
        stored_custom_headers(&provider_state, state.config.master_key.as_ref())
            .map_err(|error| fail(StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let custom_headers_were_updated = input.custom_headers.is_some();
    let normalized_custom_headers =
        custom_headers_for_input(input.custom_headers.as_deref(), &existing_custom_headers)
            .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?
            .unwrap_or(existing_custom_headers);
    let encrypted_custom_headers = if custom_headers_were_updated {
        encrypt_custom_headers(&normalized_custom_headers, state.config.master_key.as_ref())
            .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?
    } else {
        None
    };
    let model_prefix = match input.model_prefix.as_deref() {
        Some(value) => normalized_provider_model_prefix_for_adapter(Some(value), &id, &adapter_id)
            .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?,
        None => provider_model_prefix_value(&provider_state, &id).map_err(internal)?,
    };
    if provider_state.text("adapter_id").map_err(internal)? != adapter_id {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider adapter cannot be changed after the provider is created",
        ));
    }
    let local_rpm_target =
        updated_local_rpm_target(&input, &provider_state, &adapter_id).map_err(internal)?;
    let supported = serde_json::to_string(&input.supported_protocols).map_err(internal)?;
    let keys = provider_keys(&input);
    let effective_auth_type = if provider_adapters::is_oauth_only_adapter(&adapter_id) {
        provider_adapters::default_auth_type(&adapter_id)
            .expect("registered OAuth adapter has a default auth type")
    } else if input.auth_type != "none" {
        input.auth_type.as_str()
    } else if provider_state.integer("api_key_count").map_err(internal)? > 0 {
        if provider_state.text("auth_type").map_err(internal)? == "none" {
            "bearer"
        } else {
            provider_state.text("auth_type").map_err(internal)?
        }
    } else if !keys.is_empty() {
        "bearer"
    } else {
        "none"
    };
    let encrypted_keys = keys
        .iter()
        .map(|key| {
            encrypt_secret(state.config.master_key.as_ref(), key)
                .map_err(|error| fail(StatusCode::BAD_REQUEST, error))
                .and_then(|secret| {
                    secret.ok_or_else(|| fail(StatusCode::BAD_REQUEST, "provider API key is empty"))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut key_tests = Vec::with_capacity(keys.len());
    for (index, key) in keys.iter().enumerate() {
        let test = super::probes::test_provider_credential(
            &adapter_id,
            provider_adapters::AdapterApiKeyRequest {
                state: &state,
                base_url: &input.base_url,
                auth_type: effective_auth_type,
                auth_header: input.auth_header.as_deref(),
                custom_headers: &normalized_custom_headers,
                preferred_protocol: &input.preferred_protocol,
                credential: key,
            },
        )
        .await;
        key_tests.push(json!({"index":index,"test_passed":test.test_passed,"status":test.status,"message":test.message,"warning":test.warning}));
    }
    let timestamp = crate::infra::db::utc_timestamp_now().map_err(internal)?;
    let name_index = provider_name_index_key(&input.name, &id).map_err(internal)?;
    let prefix_index = provider_prefix_index_key(&model_prefix).map_err(internal)?;
    let base_url = input.base_url.trim_end_matches('/').to_owned();
    let logo_url = normalized_provider_logo_url(&input);
    let auth_header = input.auth_header.clone();
    let name = input.name.clone();
    let preferred_protocol = input.preferred_protocol.clone();
    let enabled = input.enabled;
    let live_state_changed = provider_state.boolean("enabled").map_err(internal)? != enabled;
    let adapter_id = adapter_id.clone();
    let model_prefix = model_prefix.clone();
    let supported = supported.clone();
    let effective_auth_type = effective_auth_type.to_owned();
    let mut key_records = Vec::with_capacity(encrypted_keys.len());
    for (index, (secret, test)) in encrypted_keys.iter().zip(key_tests.iter()).enumerate() {
        let key_id = uuid::Uuid::new_v4().to_string();
        let key_name = if index == 0 {
            "Added key".to_owned()
        } else {
            format!("Added key {}", index + 1)
        };
        key_records.push(provider_api_key_record(ProviderApiKeyRecordData {
            id: &key_id,
            provider_id: &id,
            name: &key_name,
            secret,
            last_error: test["message"]
                .as_str()
                .filter(|_| test["test_passed"] == false),
            last_test_passed: test["test_passed"].as_bool().unwrap_or(false),
            last_test_status: test["status"].as_i64(),
            tested_at: &timestamp,
            created_at: &timestamp,
        }));
    }
    state
        .db
        .write(move |transaction| {
            let current = transaction
                .get::<Record>(Table::Providers, &id)?
                .ok_or(StorageError::NotFound)?;
            ensure_provider_not_deleting(&current)?;
            let (thinking_mode, thinking_override) = stored_thinking_settings(&current)?;
            let key_strategy = stored_key_strategy(&current)?;
            if current.text("adapter_id")? != adapter_id {
                return Err(StorageError::Invalid(
                    "provider adapter cannot be changed after the provider is created".to_owned(),
                ));
            }
            let old_name_index = provider_name_index_key(current.text("name")?, &id)?;
            let old_prefix = provider_model_prefix_value(&current, &id)?;
            let old_prefix_index = provider_prefix_index_key(&old_prefix)?;
            if let Some(existing_id) = transaction.get::<String>(Table::Indexes, &prefix_index)?
                && existing_id != id
            {
                return Err(StorageError::Conflict);
            }
            let mut updated = provider_record(ProviderRecordData {
                id: &id,
                name: &name,
                base_url: &base_url,
                logo_url: logo_url.as_deref(),
                model_prefix: &model_prefix,
                adapter_id: &adapter_id,
                enabled,
                auth_type: &effective_auth_type,
                auth_header: auth_header.as_deref(),
                custom_headers: if custom_headers_were_updated {
                    encrypted_custom_headers.as_deref()
                } else {
                    current.optional_bytes("custom_headers")?
                },
                preferred_protocol: &preferred_protocol,
                supported_protocols: &supported,
                thinking_mode: &thinking_mode,
                thinking_override: thinking_override.as_deref(),
                key_strategy: &key_strategy,
                created_at: current.text("created_at")?,
                updated_at: &timestamp,
                api_key_count: current.integer("api_key_count")?,
                invalid_api_key_count: current.integer("invalid_api_key_count")?,
                model_count: current.integer("model_count")?,
                local_rpm_target,
            });
            if let Some(secret) = current.field("secret") {
                updated.insert("secret", secret.clone());
            }
            transaction.delete(Table::ProviderNameIndex, &old_name_index)?;
            transaction.delete(Table::Indexes, &old_prefix_index)?;
            transaction.put(Table::Providers, &id, &updated)?;
            transaction.put_if_absent(Table::ProviderNameIndex, &name_index, &id)?;
            transaction.put(Table::Indexes, &prefix_index, &id)?;
            for record in &key_records {
                store_provider_api_key(transaction, record)?;
                let mut provider = transaction
                    .get::<Record>(Table::Providers, &id)?
                    .ok_or(StorageError::NotFound)?;
                let count = provider
                    .integer("api_key_count")?
                    .checked_add(1)
                    .ok_or_else(|| {
                        StorageError::Invalid("provider API key count overflowed".to_owned())
                    })?;
                provider.insert("api_key_count", Field::I64(count));
                transaction.put(Table::Providers, &id, &provider)?;
            }
            Ok(())
        })
        .await
        .map_err(|error| match error {
            StorageError::Conflict => fail(
                StatusCode::CONFLICT,
                "provider is being deleted or its model prefix is already assigned",
            ),
            StorageError::NotFound => fail(StatusCode::NOT_FOUND, "provider not found"),
            other => internal(other),
        })?;
    if live_state_changed {
        notify_enabled_provider_count_changed(&state).await;
    }
    Ok(Json(json!({"ok":true,"key_tests":key_tests})))
}

pub(crate) async fn update_provider_thinking_settings(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
    Json(input): Json<ProviderThinkingSettingsInput>,
) -> ApiResult {
    crate::protocol::parse_thinking_handling(&input.mode, input.override_text.as_deref())
        .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    let timestamp = crate::infra::db::utc_timestamp_now().map_err(internal)?;
    let ProviderThinkingSettingsInput {
        mode,
        override_text,
    } = input;
    state
        .db
        .write(move |transaction| {
            let mut provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            ensure_provider_not_deleting(&provider)?;
            provider.insert("thinking_mode", Field::Text(mode.clone()));
            provider.insert(
                "thinking_override",
                override_text
                    .clone()
                    .map(Field::Text)
                    .unwrap_or(Field::Null),
            );
            provider.insert("updated_at", Field::Text(timestamp.clone()));
            transaction.put(Table::Providers, &provider_id, &provider)
        })
        .await
        .map_err(|error| match error {
            StorageError::NotFound => fail(StatusCode::NOT_FOUND, "provider not found"),
            StorageError::Conflict => fail(
                StatusCode::CONFLICT,
                "provider is being deleted and cannot be updated",
            ),
            other => internal(other),
        })?;
    Ok(Json(json!({"ok":true})))
}

/// Switches how the gateway picks the first key of a provider request. The
/// stored value is read per request, so the change applies to the next request
/// without restarting the process.
pub(crate) async fn update_provider_key_strategy(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
    Json(input): Json<ProviderKeyStrategyInput>,
) -> ApiResult {
    let strategy = input.strategy;
    if strategy != DEFAULT_KEY_STRATEGY && strategy != KEY_STRATEGY_ROUND_ROBIN {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "strategy must be priority or round_robin",
        ));
    }
    let timestamp = crate::infra::db::utc_timestamp_now().map_err(internal)?;
    let response = strategy.clone();
    state
        .db
        .write(move |transaction| {
            let mut provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            ensure_provider_not_deleting(&provider)?;
            provider.insert("key_strategy", Field::Text(strategy.clone()));
            provider.insert("updated_at", Field::Text(timestamp.clone()));
            transaction.put(Table::Providers, &provider_id, &provider)
        })
        .await
        .map_err(|error| match error {
            StorageError::NotFound => fail(StatusCode::NOT_FOUND, "provider not found"),
            StorageError::Conflict => fail(
                StatusCode::CONFLICT,
                "provider is being deleted and cannot be updated",
            ),
            other => internal(other),
        })?;
    Ok(Json(json!({"ok":true,"strategy":response})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::test_support::TestDatabase;

    async fn seed_strategy_provider(database: &TestDatabase, id: &str, deleting: bool) {
        let id = id.to_owned();
        database
            .db
            .write(move |transaction| {
                let mut provider = Record::new()
                    .with("id", Field::Text(id.clone()))
                    .with("name", Field::Text(format!("Provider {id}")))
                    .with("adapter_id", Field::Text("generic".to_owned()))
                    .with("enabled", Field::Bool(false));
                if deleting {
                    provider.insert("deleting", Field::Bool(true));
                    provider.insert("deletion_phase", Field::Text("api_keys".to_owned()));
                }
                transaction.put(Table::Providers, &id, &provider)
            })
            .await
            .expect("seed provider");
    }

    async fn stored_strategy(state: &AppState, provider_id: &str) -> Option<String> {
        let provider_id = provider_id.to_owned();
        state
            .db
            .read(move |transaction| {
                let Some(record) = transaction.get::<Record>(Table::Providers, &provider_id)?
                else {
                    return Ok(None);
                };
                Ok(record.optional_text("key_strategy")?.map(str::to_owned))
            })
            .await
            .expect("read provider")
    }

    fn strategy_input(strategy: &str) -> Json<ProviderKeyStrategyInput> {
        Json(ProviderKeyStrategyInput {
            strategy: strategy.to_owned(),
        })
    }

    #[tokio::test]
    async fn key_strategy_endpoint_stores_supported_strategies_and_rejects_unknown_ones() {
        let database = TestDatabase::open().await;
        let state = AppState::new(database.config(), database.db.clone());
        seed_strategy_provider(&database, "provider", false).await;

        let (status, _) = update_provider_key_strategy(
            State(state.clone()),
            Path("provider".to_owned()),
            strategy_input("random"),
        )
        .await
        .expect_err("unknown strategy is rejected");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(stored_strategy(&state, "provider").await, None);

        let response = update_provider_key_strategy(
            State(state.clone()),
            Path("provider".to_owned()),
            strategy_input(KEY_STRATEGY_ROUND_ROBIN),
        )
        .await
        .expect("round robin is stored");
        assert_eq!(response.0["strategy"], KEY_STRATEGY_ROUND_ROBIN);
        assert_eq!(
            stored_strategy(&state, "provider").await.as_deref(),
            Some(KEY_STRATEGY_ROUND_ROBIN)
        );

        let response = update_provider_key_strategy(
            State(state.clone()),
            Path("provider".to_owned()),
            strategy_input(DEFAULT_KEY_STRATEGY),
        )
        .await
        .expect("the default strategy is stored");
        assert_eq!(response.0["strategy"], DEFAULT_KEY_STRATEGY);
        assert_eq!(
            stored_strategy(&state, "provider").await.as_deref(),
            Some(DEFAULT_KEY_STRATEGY)
        );

        let (status, _) = update_provider_key_strategy(
            State(state.clone()),
            Path("missing".to_owned()),
            strategy_input(KEY_STRATEGY_ROUND_ROBIN),
        )
        .await
        .expect_err("unknown provider is rejected");
        assert_eq!(status, StatusCode::NOT_FOUND);

        seed_strategy_provider(&database, "deleting", true).await;
        let (status, _) = update_provider_key_strategy(
            State(state),
            Path("deleting".to_owned()),
            strategy_input(KEY_STRATEGY_ROUND_ROBIN),
        )
        .await
        .expect_err("a provider marked for deletion is rejected");
        assert_eq!(status, StatusCode::CONFLICT);
    }
}
