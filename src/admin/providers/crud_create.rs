use super::super::{ApiResult, fail, internal};
use super::{
    ProviderRecordData,
    custom_headers::{custom_headers_for_input, encrypt_custom_headers},
    notify_enabled_provider_count_changed,
    provider_keys::{ProviderApiKeyRecordData, provider_api_key_record, store_provider_api_key},
    provider_name_index_key, provider_prefix_index_key, provider_record,
    provider_view::ProviderInput,
    validation::{
        DEFAULT_KEY_STRATEGY, create_local_rpm_target, effective_adapter_id,
        normalized_provider_logo_url, normalized_provider_model_prefix_for_adapter, provider_keys,
        validate_provider,
    },
};
use crate::{
    infra::storage::{Record, StorageError, Table},
    provider_adapters,
    security::encrypt_secret,
    state::AppState,
};
use axum::{Json, extract::State, http::StatusCode};
use serde_json::json;
use std::collections::BTreeMap;

pub(crate) async fn create_provider(
    State(state): State<AppState>,
    Json(input): Json<ProviderInput>,
) -> ApiResult {
    let adapter_id = effective_adapter_id(&input);
    validate_provider(
        &input,
        &adapter_id,
        state.config.allow_private_provider_urls,
    )?;
    let local_rpm_target = create_local_rpm_target(&input, &adapter_id);
    let id = if input.id.trim().is_empty() {
        super::super::resource_id(&input.name)
    } else {
        input.id.clone()
    };
    let model_prefix = normalized_provider_model_prefix_for_adapter(
        input.model_prefix.as_deref(),
        &id,
        &adapter_id,
    )
    .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    let normalized_custom_headers =
        custom_headers_for_input(input.custom_headers.as_deref(), &BTreeMap::new())
            .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?
            .unwrap_or_default();
    let encrypted_custom_headers =
        encrypt_custom_headers(&normalized_custom_headers, state.config.master_key.as_ref())
            .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    let keys = provider_keys(&input);
    if state
        .db
        .read({
            let id = id.clone();
            move |transaction| Ok(transaction.get::<Record>(Table::Providers, &id)?.is_some())
        })
        .await
        .map_err(internal)?
    {
        return Err(fail(StatusCode::CONFLICT, "provider id already exists"));
    }
    let effective_auth_type = if provider_adapters::is_oauth_only_adapter(&adapter_id) {
        provider_adapters::default_auth_type(&adapter_id)
            .expect("registered OAuth adapter has a default auth type")
            .to_owned()
    } else if keys.is_empty() {
        let preset = provider_adapters::preset_for_adapter(&adapter_id);
        if preset.is_some_and(|preset| preset.supported_auth_types.contains(&"none")) {
            "none".to_owned()
        } else {
            provider_adapters::default_auth_type(&adapter_id)
                .unwrap_or(input.auth_type.as_str())
                .to_owned()
        }
    } else if input.auth_type == "none" {
        "bearer".to_owned()
    } else {
        input.auth_type.clone()
    };
    let mut key_tests = Vec::with_capacity(keys.len());
    let mut encrypted_keys = Vec::with_capacity(keys.len());
    for (index, key) in keys.iter().enumerate() {
        let result = super::probes::test_provider_credential(
            &adapter_id,
            provider_adapters::AdapterApiKeyRequest {
                state: &state,
                base_url: &input.base_url,
                auth_type: &effective_auth_type,
                auth_header: input.auth_header.as_deref(),
                custom_headers: &normalized_custom_headers,
                preferred_protocol: &input.preferred_protocol,
                credential: key,
            },
        )
        .await;
        encrypted_keys.push(
            encrypt_secret(state.config.master_key.as_ref(), key)
                .map_err(|error| fail(StatusCode::BAD_REQUEST, error))?
                .ok_or_else(|| fail(StatusCode::BAD_REQUEST, "provider API key is empty"))?,
        );
        key_tests.push(json!({
            "index": index,
            "test_passed": result.test_passed,
            "status": result.status,
            "message": result.message,
            "warning": result.warning,
        }));
    }
    let supported = serde_json::to_string(&input.supported_protocols).map_err(internal)?;
    let timestamp = crate::infra::db::utc_timestamp_now().map_err(internal)?;
    let provider_id = id.clone();
    let name_index = provider_name_index_key(&input.name, &id).map_err(internal)?;
    let prefix_index = provider_prefix_index_key(&model_prefix).map_err(internal)?;
    let base_url = input.base_url.trim_end_matches('/').to_owned();
    let logo_url = normalized_provider_logo_url(&input);
    let auth_header = input.auth_header.clone();
    let enabled = input.enabled;
    let preferred_protocol = input.preferred_protocol.clone();
    let api_key_count = i64::try_from(encrypted_keys.len()).map_err(internal)?;
    let invalid_count = 0_i64;
    let mut key_records = Vec::with_capacity(encrypted_keys.len());
    for (index, secret) in encrypted_keys.iter().enumerate() {
        let test = &key_tests[index];
        let key_name = if index == 0 {
            "Primary key".to_owned()
        } else {
            format!("Key {}", index + 1)
        };
        let key_id = uuid::Uuid::new_v4().to_string();
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
            if transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .is_some()
            {
                return Err(StorageError::Conflict);
            }
            if transaction
                .get::<String>(Table::Indexes, &prefix_index)?
                .is_some()
            {
                return Err(StorageError::Conflict);
            }
            let provider = provider_record(ProviderRecordData {
                id: &provider_id,
                name: &input.name,
                base_url: &base_url,
                logo_url: logo_url.as_deref(),
                model_prefix: &model_prefix,
                adapter_id: &adapter_id,
                enabled,
                auth_type: &effective_auth_type,
                auth_header: auth_header.as_deref(),
                custom_headers: encrypted_custom_headers.as_deref(),
                preferred_protocol: &preferred_protocol,
                supported_protocols: &supported,
                thinking_mode: crate::protocol::DEFAULT_THINKING_MODE,
                thinking_override: None,
                key_strategy: DEFAULT_KEY_STRATEGY,
                created_at: &timestamp,
                updated_at: &timestamp,
                api_key_count,
                invalid_api_key_count: invalid_count,
                model_count: 0,
                local_rpm_target,
            });
            transaction.put_if_absent(Table::Providers, &provider_id, &provider)?;
            transaction.put_if_absent(Table::ProviderNameIndex, &name_index, &provider_id)?;
            transaction.put_if_absent(Table::Indexes, &prefix_index, &provider_id)?;
            for record in &key_records {
                store_provider_api_key(transaction, record)?;
            }
            let count = transaction
                .get::<i64>(Table::Meta, "provider_count")?
                .ok_or_else(|| StorageError::Invalid("provider counter is missing".to_owned()))?;
            transaction.put(
                Table::Meta,
                "provider_count",
                &count
                    .checked_add(1)
                    .ok_or_else(|| StorageError::Invalid("provider count overflowed".to_owned()))?,
            )?;
            Ok(())
        })
        .await
        .map_err(|error| {
            if matches!(error, StorageError::Conflict) {
                fail(
                    StatusCode::CONFLICT,
                    "provider id or model prefix already exists",
                )
            } else {
                internal(error)
            }
        })?;
    if enabled {
        notify_enabled_provider_count_changed(&state).await;
    }
    Ok(Json(json!({"ok":true,"id":id,"key_tests":key_tests})))
}
