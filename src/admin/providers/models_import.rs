//! Upstream catalog discovery orchestration for the import endpoint.
//!
//! Selects the discovery strategy per adapter (OAuth accounts, public
//! catalog, or per-credential probing) and persists the merged result via
//! `models_store`.

use super::models_store::{ProviderModelSource, prune_stale_models, store_provider_models};
use super::*;
use crate::infra::storage::{Record, Table};

pub(crate) async fn import_provider_models(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult {
    let _probe_permit = state
        .admin
        .admin_provider_probe_in_flight
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "Provider probe capacity is busy; retry the model import shortly.",
            )
        })?;
    let provider_id = id.clone();
    let (provider, key_rows) = state
        .db
        .read(move |transaction| {
            let provider = transaction.get::<Record>(Table::Providers, &provider_id)?;
            let Some(provider) = provider else {
                return Ok((None, Vec::new()));
            };
            if provider_is_deleting(&provider)? {
                return Ok((None, Vec::new()));
            }
            let prefix = crate::infra::db::provider_api_key_created_prefix(&provider_id)?;
            let rows =
                transaction.scan_prefix::<String>(Table::ProviderApiKeyCreatedIndex, &prefix, 8)?;
            let mut keys = Vec::with_capacity(rows.len());
            for (_, key_id) in rows {
                if let Some(key) = transaction.get::<Record>(Table::ProviderApiKeys, &key_id)?
                    && key.boolean("enabled")?
                    && !key.boolean("invalid")?
                {
                    keys.push(key);
                }
            }
            Ok((Some(provider), keys))
        })
        .await
        .map_err(internal)?;
    let provider = provider.ok_or_else(|| fail(StatusCode::NOT_FOUND, "provider not found"))?;

    let base_url = provider.text("base_url").map_err(internal)?.to_owned();
    let adapter_id = provider.text("adapter_id").map_err(internal)?.to_owned();
    let auth_type = provider.text("auth_type").map_err(internal)?.to_owned();
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
    let provider_name = provider_adapters::preset_for_adapter(&adapter_id)
        .map(|preset| preset.name)
        .unwrap_or("provider");
    let legacy_secret = provider
        .optional_bytes("secret")
        .map_err(internal)?
        .map(<[u8]>::to_vec);
    if provider_adapters::is_oauth_only_adapter(&adapter_id) {
        return import_oauth_models(&state, &id, &adapter_id, provider_name, &key_rows).await;
    }
    if provider_adapters::has_public_model_catalog(&adapter_id) {
        return import_public_catalog_models(
            &state,
            &id,
            &adapter_id,
            &base_url,
            &auth_type,
            auth_header.as_deref(),
            &custom_headers,
            &preferred_protocol,
        )
        .await;
    }
    import_credential_probed_models(
        &state,
        &id,
        &adapter_id,
        &auth_type,
        &base_url,
        auth_header.as_deref(),
        &custom_headers,
        &preferred_protocol,
        legacy_secret.as_deref(),
        &key_rows,
    )
    .await
}

async fn import_oauth_models(
    state: &AppState,
    id: &str,
    adapter_id: &str,
    provider_name: &str,
    key_rows: &[Record],
) -> ApiResult {
    let accounts = key_rows
        .iter()
        .map(|record| record.text("id").map(str::to_owned).map_err(internal))
        .collect::<Result<Vec<_>, _>>()?;
    if accounts.is_empty() {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            format!("connect a {provider_name} account before importing models"),
        ));
    }
    let mut last_error = None;
    for account_id in accounts {
        match provider_adapters::discover_oauth_models(adapter_id, state, &account_id).await {
            Ok(models) if models.is_empty() => {
                last_error = Some(format!("{provider_name} returned no supported models"));
            }
            Ok(models) => {
                let pruned = if adapter_id == provider_adapters::ANTIGRAVITY_ADAPTER_ID {
                    prune_stale_models(&state.db, id, &models).await?
                } else {
                    Vec::new()
                };
                let outcome = store_provider_models(
                    &state.db,
                    id,
                    &models,
                    ProviderModelSource::Import,
                    provider_adapters::model_catalog_authoritative(adapter_id),
                )
                .await?;
                return Ok(Json(json!({
                    "models": models,
                    "available": true,
                    "truncated": false,
                    "pruned": pruned,
                    "manual_kept": outcome.manual_kept,
                })));
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(fail(
        StatusCode::BAD_GATEWAY,
        last_error
            .as_deref()
            .unwrap_or("Could not load provider models"),
    ))
}

#[allow(clippy::too_many_arguments)]
async fn import_public_catalog_models(
    state: &AppState,
    id: &str,
    adapter_id: &str,
    base_url: &str,
    auth_type: &str,
    auth_header: Option<&str>,
    custom_headers: &std::collections::BTreeMap<String, String>,
    preferred_protocol: &str,
) -> ApiResult {
    let discovery = provider_adapters::discover_api_key_models(
        adapter_id,
        provider_adapters::AdapterApiKeyRequest {
            state,
            base_url,
            auth_type,
            auth_header,
            custom_headers,
            preferred_protocol,
            credential: "",
        },
    )
    .await
    .map_err(|message| fail(StatusCode::BAD_GATEWAY, message))?;
    let provider_adapters::ModelDiscoveryResult::Available { models, truncated } = discovery else {
        return Err(fail(
            StatusCode::BAD_GATEWAY,
            "Cline model catalog is unavailable",
        ));
    };
    store_provider_models(&state.db, id, &models, ProviderModelSource::Import, false).await?;
    Ok(Json(json!({
        "models": models,
        "available": true,
        "truncated": truncated,
    })))
}

#[allow(clippy::too_many_arguments)]
async fn import_credential_probed_models(
    state: &AppState,
    id: &str,
    adapter_id: &str,
    auth_type: &str,
    base_url: &str,
    auth_header: Option<&str>,
    custom_headers: &std::collections::BTreeMap<String, String>,
    preferred_protocol: &str,
    legacy_secret: Option<&[u8]>,
    key_rows: &[Record],
) -> ApiResult {
    let mut credentials = Vec::new();
    if let Some(secret) =
        decrypt_secret(state.config.master_key.as_ref(), legacy_secret).map_err(|_| {
            fail(
                StatusCode::BAD_REQUEST,
                "could not decrypt provider credentials; check EXOROUTE_MASTER_KEY",
            )
        })?
    {
        credentials.push(secret);
    }

    for row in key_rows {
        let secret = row.bytes("secret").map_err(internal)?;
        if let Some(secret) = decrypt_secret(state.config.master_key.as_ref(), Some(secret))
            .map_err(|_| {
                fail(
                    StatusCode::BAD_REQUEST,
                    "could not decrypt provider credentials; check EXOROUTE_MASTER_KEY",
                )
            })?
        {
            credentials.push(secret);
        }
    }

    if credentials.is_empty() && auth_type != "none" {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider has no enabled API keys; add a key before importing models",
        ));
    }
    if credentials.is_empty() {
        credentials.push(String::new());
    }

    let mut last_error = None;
    let mut unsupported = false;
    for credential in credentials {
        match provider_adapters::discover_api_key_models(
            adapter_id,
            provider_adapters::AdapterApiKeyRequest {
                state,
                base_url,
                auth_type,
                auth_header,
                custom_headers,
                preferred_protocol,
                credential: &credential,
            },
        )
        .await
        {
            Ok(provider_adapters::ModelDiscoveryResult::Available { models, truncated }) => {
                let outcome = store_provider_models(
                    &state.db,
                    id,
                    &models,
                    ProviderModelSource::Import,
                    provider_adapters::model_catalog_authoritative(adapter_id),
                )
                .await?;
                return Ok(Json(json!({
                    "models": models,
                    "available": true,
                    "truncated": truncated,
                    "manual_kept": outcome.manual_kept,
                })));
            }
            Ok(provider_adapters::ModelDiscoveryResult::Unsupported) => unsupported = true,
            Err(error) => last_error = Some(error),
        }
    }

    if let Some(error) = last_error {
        return Err(fail(StatusCode::BAD_GATEWAY, error));
    }
    if unsupported {
        return Ok(Json(
            json!({"models":[],"available":false,"truncated":false}),
        ));
    }
    Ok(Json(
        json!({"models":[],"available":false,"truncated":false}),
    ))
}
