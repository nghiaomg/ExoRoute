use super::*;
use crate::infra::storage::{Record, StorageError, Table};
use crate::protocol::{Protocol, UpstreamProtocol};
pub(super) fn apply_path_id_for_update(
    path_id: &str,
    input: &mut ComboInput,
) -> Result<(), &'static str> {
    if !input.id.is_empty() && path_id != input.id {
        return Err("path id must match combo id");
    }
    input.id = path_id.to_owned();
    Ok(())
}

pub(super) async fn validate_saved_combo_models(
    state: &AppState,
    input: &ComboInput,
) -> Result<(), (StatusCode, Json<Value>)> {
    let targets = input.targets.clone();
    let validation_error = state
        .db
        .read(move |transaction| {
            for target in &targets {
                let key =
                    crate::infra::db::provider_model_key(&target.provider_id, target.model.trim())?;
                if transaction
                    .get::<Record>(Table::ProviderModels, &key)?
                    .is_none()
                {
                    return Ok(Some(
                        "target model is not saved for its provider; import provider models first",
                    ));
                }
                let Some(provider) =
                    transaction.get::<Record>(Table::Providers, &target.provider_id)?
                else {
                    return Ok(Some("target provider was not found"));
                };
                if let Some(value) = target.protocol.as_deref() {
                    let protocol = value.parse::<UpstreamProtocol>().map_err(|error| {
                        StorageError::Invalid(format!("combo target protocol is invalid: {error}"))
                    })?;
                    let adapter_id = provider.text("adapter_id")?;
                    if !crate::provider_adapters::supports_upstream_protocol(adapter_id, protocol) {
                        return Ok(Some(
                            "provider adapter does not support the selected target protocol",
                        ));
                    }
                    let supported: Vec<String> =
                        serde_json::from_str(provider.text("supported_protocols")?)
                            .map_err(|error| StorageError::Codec(error.to_string()))?;
                    if !supported.iter().any(|value| value == protocol.as_str()) {
                        return Ok(Some(
                            "provider does not declare support for the selected target protocol",
                        ));
                    }
                }
            }
            Ok(None)
        })
        .await
        .map_err(internal)?;
    if let Some(message) = validation_error {
        return Err(fail(StatusCode::BAD_REQUEST, message));
    }
    Ok(())
}

pub(super) fn validate_combo(
    input: &ComboInput,
    max_targets: Option<usize>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if input.name.trim().is_empty() || input.name.len() > 256 {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "combo name is required and must not exceed 256 bytes",
        ));
    }
    if input.name.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "combo name contains unsupported control characters",
        ));
    }
    if input.id.len() > MAX_ROUTE_ID_BYTES {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "route ID must not exceed 128 bytes",
        ));
    }
    if input.id.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "route ID contains unsupported control characters",
        ));
    }
    if !["priority", "round_robin"].contains(&input.strategy.as_str()) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "strategy must be priority or round_robin",
        ));
    }
    if input.targets.is_empty() {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "combo must have at least one target",
        ));
    }
    if input.targets.len() > MAX_ROUTE_TARGETS
        || max_targets.is_some_and(|maximum| input.targets.len() > maximum)
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "combo has more targets than the supported limit",
        ));
    }
    for protocol in &input.accepted_protocols {
        protocol
            .parse::<Protocol>()
            .map_err(|e| fail(StatusCode::BAD_REQUEST, e))?;
    }
    for target in &input.targets {
        if target.provider_id.trim().is_empty()
            || target.provider_id.len() > MAX_PROVIDER_ID_BYTES
            || target.model.trim().is_empty()
            || target.model.len() > MAX_MODEL_BYTES
        {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "each target needs a provider ID and model within the supported limits",
            ));
        }
        if let Some(protocol) = &target.protocol {
            protocol
                .parse::<UpstreamProtocol>()
                .map_err(|e| fail(StatusCode::BAD_REQUEST, e))?;
        }
    }
    Ok(())
}
