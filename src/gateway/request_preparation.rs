use crate::{
    config::GatewayResourceLimits,
    infra::storage::{Record, StorageError, Table},
    infra::telemetry::RequestAnalytics,
    protocol::{self, Protocol, UpstreamProtocol},
    state::AppState,
    support::output_styles,
};
use axum::{
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde_json::Value;
use std::sync::Arc;

const MAX_ROUTE_TARGETS: usize = 16;

#[derive(Clone)]
pub(super) struct Target {
    pub(super) provider_id: String,
    pub(super) model: String,
    pub(super) protocol: Option<UpstreamProtocol>,
    pub(super) model_protocol: Option<UpstreamProtocol>,
    pub(super) priority: i64,
    pub(super) enabled: bool,
}

pub(super) struct StoredRoute {
    pub(super) enabled: bool,
    pub(super) strategy: String,
    pub(super) accepted_protocols: Vec<String>,
    pub(super) targets: Vec<Target>,
    pub(super) exceeds_target_limit: bool,
}

pub(super) struct PreparedGatewayRequest {
    pub(super) request_id: String,
    pub(super) request_api_key_id: Option<String>,
    pub(super) adapter_session_id: String,
    pub(super) canonical: protocol::CanonicalRequest,
    pub(super) route_alias: String,
    pub(super) targets: Vec<Target>,
    pub(super) background_continuity: bool,
}

pub(super) enum GatewayRequestPreparationError {
    Client { status: StatusCode, message: String },
    Database(StorageError),
}

impl GatewayRequestPreparationError {
    pub(super) fn into_response(self) -> Response {
        match self {
            Self::Client { status, message } => super::gateway_error(status, &message),
            Self::Database(error) => super::gateway_database_error(error),
        }
    }
}

pub(super) struct GatewayRequestPreparation {
    pub(super) state: AppState,
    pub(super) headers: HeaderMap,
    pub(super) body: Arc<Value>,
    pub(super) client_protocol: Protocol,
    pub(super) resource_limits: GatewayResourceLimits,
    pub(super) api_key_id: Option<String>,
    pub(super) analytics: Option<RequestAnalytics>,
    pub(super) target_rotation_offset: usize,
}

pub(super) async fn prepare_gateway_request(
    preparation: GatewayRequestPreparation,
) -> Result<PreparedGatewayRequest, GatewayRequestPreparationError> {
    let GatewayRequestPreparation {
        state,
        headers,
        body,
        client_protocol,
        resource_limits,
        api_key_id,
        analytics,
        target_rotation_offset,
    } = preparation;
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .filter(|s| !s.trim().is_empty() && s.len() <= 128)
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let request_api_key_id = analytics
        .as_ref()
        .map(|analytics| analytics.api_key_id().to_owned())
        .or(api_key_id.clone());
    let api_key_present = api_key_id.is_some();
    let adapter_session_id = [
        "x-opencode-session",
        "session_id",
        "session-id",
        "x-session-id",
    ]
    .iter()
    .find_map(|name| headers.get(*name).and_then(|value| value.to_str().ok()))
    .map(str::trim)
    .filter(|value| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
    })
    .map(str::to_owned)
    .or_else(|| {
        let fallback = request_id.trim();
        (!fallback.is_empty()
            && fallback.len() <= 128
            && fallback
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte)))
        .then(|| fallback.to_owned())
    })
    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut canonical =
        protocol::decode_request(client_protocol, body.as_ref()).map_err(|error| {
            GatewayRequestPreparationError::Client {
                status: StatusCode::BAD_REQUEST,
                message: error,
            }
        })?;
    // Capture the style snapshot only after decoding the bounded request. This
    // gives each request one atomic runtime view immediately before routing.
    let output_styles = state.output_styles();
    let style_result =
        output_styles::apply(&mut canonical, &output_styles.styles).map_err(|error| {
            tracing::error!(%error, "output style configuration could not be applied");
            GatewayRequestPreparationError::Client {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "output style configuration is invalid".to_owned(),
            }
        })?;
    state.record_output_styles_result(style_result);
    let route_alias = canonical.model.clone();
    let background_continuity =
        resource_limits.stream_continuity_enabled && !api_key_present && canonical.stream;
    let targets = resolve_gateway_targets(
        &state,
        &route_alias,
        client_protocol,
        target_rotation_offset,
    )
    .await?;

    Ok(PreparedGatewayRequest {
        request_id,
        request_api_key_id,
        adapter_session_id,
        canonical,
        route_alias,
        targets,
        background_continuity,
    })
}

async fn resolve_gateway_targets(
    state: &AppState,
    route_alias: &str,
    client_protocol: Protocol,
    target_rotation_offset: usize,
) -> Result<Vec<Target>, GatewayRequestPreparationError> {
    let route = load_stored_route(&state.db, route_alias)
        .await
        .map_err(GatewayRequestPreparationError::Database)?;
    let (strategy, mut targets) = if let Some(route) = route {
        if !route.enabled {
            return Err(GatewayRequestPreparationError::Client {
                status: StatusCode::NOT_FOUND,
                message: format!("no enabled route for model '{route_alias}'"),
            });
        }
        if !route
            .accepted_protocols
            .iter()
            .any(|protocol| protocol == client_protocol.as_str())
        {
            return Err(GatewayRequestPreparationError::Client {
                status: StatusCode::NOT_FOUND,
                message: format!(
                    "route '{route_alias}' does not accept {}",
                    client_protocol.as_str()
                ),
            });
        }
        if route.exceeds_target_limit {
            return Err(GatewayRequestPreparationError::Client {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "route exceeds the maximum of 16 configured targets".to_owned(),
            });
        }
        (route.strategy, route.targets)
    } else {
        match resolve_provider_model_alias(&state.db, route_alias)
            .await
            .map_err(GatewayRequestPreparationError::Database)?
        {
            ProviderModelAliasResolution::Found(target) => ("priority".to_owned(), vec![target]),
            ProviderModelAliasResolution::NotFound => {
                return Err(GatewayRequestPreparationError::Client {
                    status: StatusCode::NOT_FOUND,
                    message: format!(
                        "no enabled route or imported provider model for '{route_alias}'"
                    ),
                });
            }
        }
    };
    targets.retain(|target| target.enabled);
    if targets.is_empty() {
        return Err(GatewayRequestPreparationError::Client {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "route has no enabled targets".to_owned(),
        });
    }
    if strategy == "round_robin" {
        let mut indexes = state.gates.round_robin.lock().await;
        let current = indexes.entry(route_alias.to_owned()).or_insert(0);
        let rotate_by = *current % targets.len();
        targets.rotate_left(rotate_by);
        *current = current.wrapping_add(1);
    } else {
        order_priority_targets(&mut targets, target_rotation_offset);
    }
    Ok(targets)
}

fn order_priority_targets(targets: &mut [Target], target_rotation_offset: usize) {
    targets.sort_by_key(|target| target.priority);
    let Some(target_count) = (!targets.is_empty()).then_some(targets.len()) else {
        return;
    };
    targets.rotate_left(target_rotation_offset % target_count);
}

pub(super) async fn load_stored_route(
    db: &crate::infra::storage::Database,
    route_id: &str,
) -> Result<Option<StoredRoute>, StorageError> {
    let route_id = route_id.to_owned();
    db.read(move |transaction| {
        let Some(route) = transaction.get::<Record>(Table::Routes, &route_id)? else {
            return Ok(None);
        };
        let accepted_protocols =
            serde_json::from_str(route.text("accepted_protocols")?).map_err(|error| {
                StorageError::Codec(format!("route protocol list is invalid: {error}"))
            })?;
        let target_prefix = route_target_prefix(&route_id)?;
        let target_records = transaction.scan_prefix::<Record>(
            Table::RouteTargets,
            &target_prefix,
            MAX_ROUTE_TARGETS + 1,
        )?;
        let exceeds_target_limit = target_records.len() > MAX_ROUTE_TARGETS;
        let mut targets = Vec::with_capacity(target_records.len().min(MAX_ROUTE_TARGETS));
        for (_, target) in target_records.into_iter().take(MAX_ROUTE_TARGETS) {
            let protocol = target
                .optional_text("protocol")?
                .map(|value| {
                    value.parse::<UpstreamProtocol>().map_err(|error| {
                        StorageError::Invalid(format!("route target protocol is invalid: {error}"))
                    })
                })
                .transpose()?;
            let provider_id = target.text("provider_id")?.to_owned();
            let model = target.text("model")?.to_owned();
            let model_key = crate::infra::db::provider_model_key(&provider_id, &model)?;
            let model_protocol = if let Some(model_record) =
                transaction.get::<Record>(Table::ProviderModels, &model_key)?
            {
                model_record
                    .optional_text("upstream_protocol")?
                    .map(|value| {
                        value.parse::<UpstreamProtocol>().map_err(|error| {
                            StorageError::Invalid(format!(
                                "saved provider model protocol is invalid: {error}"
                            ))
                        })
                    })
                    .transpose()?
            } else {
                None
            };
            targets.push(Target {
                provider_id,
                model,
                protocol,
                model_protocol,
                priority: target.integer("priority")?,
                enabled: target.boolean("enabled")?,
            });
        }
        Ok(Some(StoredRoute {
            enabled: route.boolean("enabled")?,
            strategy: route.text("strategy")?.to_owned(),
            accepted_protocols,
            targets,
            exceeds_target_limit,
        }))
    })
    .await
}

fn route_target_prefix(route_id: &str) -> Result<String, StorageError> {
    let mut encoded = String::with_capacity(route_id.len().saturating_mul(2));
    for byte in route_id.bytes() {
        use std::fmt::Write as _;
        write!(encoded, "{byte:02x}")
            .map_err(|_| StorageError::Invalid("could not encode route id".to_owned()))?;
    }
    let prefix = format!("r/{encoded}/");
    crate::infra::storage::validate_key(&prefix)?;
    Ok(prefix)
}

pub(super) enum ProviderModelAliasResolution {
    NotFound,
    Found(Target),
}

pub(super) async fn resolve_provider_model_alias(
    db: &crate::infra::storage::Database,
    public_model: &str,
) -> Result<ProviderModelAliasResolution, StorageError> {
    let Some((prefix, model)) = public_model.split_once('/') else {
        return Ok(ProviderModelAliasResolution::NotFound);
    };
    if prefix.is_empty() || model.is_empty() {
        return Ok(ProviderModelAliasResolution::NotFound);
    }
    let model_prefix = prefix.to_ascii_lowercase();
    let model_name = model.to_owned();
    db.read(move |transaction| {
        let prefix_key = format!("provider_model_prefix/{model_prefix}");
        let Some(provider_id) = transaction.get::<String>(Table::Indexes, &prefix_key)? else {
            return Ok(ProviderModelAliasResolution::NotFound);
        };
        let Some(provider) = transaction.get::<Record>(Table::Providers, &provider_id)? else {
            return Err(StorageError::Invalid(
                "provider prefix index points to a missing provider".to_owned(),
            ));
        };
        if !provider.boolean("enabled")? {
            return Ok(ProviderModelAliasResolution::NotFound);
        }
        let model_key = crate::infra::db::provider_model_index_key(&provider_id, &model_name)?;
        let Some(saved_model) = transaction.get::<String>(Table::ProviderModelIndex, &model_key)?
        else {
            return Ok(ProviderModelAliasResolution::NotFound);
        };
        if saved_model != model_name {
            return Ok(ProviderModelAliasResolution::NotFound);
        }
        let model_primary_key = crate::infra::db::provider_model_key(&provider_id, &model_name)?;
        let model_record = transaction
            .get::<Record>(Table::ProviderModels, &model_primary_key)?
            .ok_or_else(|| {
                StorageError::Invalid(
                    "provider model index points to a missing model record".to_owned(),
                )
            })?;
        let model_protocol = model_record
            .optional_text("upstream_protocol")?
            .map(|value| {
                value.parse::<UpstreamProtocol>().map_err(|error| {
                    StorageError::Invalid(format!(
                        "saved provider model protocol is invalid: {error}"
                    ))
                })
            })
            .transpose()?;
        Ok(ProviderModelAliasResolution::Found(Target {
            provider_id,
            model: saved_model,
            protocol: None,
            model_protocol,
            priority: 0,
            enabled: true,
        }))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(model: &str, priority: i64) -> Target {
        Target {
            provider_id: "provider".to_owned(),
            model: model.to_owned(),
            protocol: None,
            model_protocol: None,
            priority,
            enabled: true,
        }
    }

    #[test]
    fn priority_targets_rotate_from_the_next_model_on_each_retry() {
        let mut targets = vec![
            target("model-c", 3),
            target("model-a", 1),
            target("model-b", 2),
        ];

        order_priority_targets(&mut targets, 0);
        assert_eq!(
            targets
                .iter()
                .map(|target| target.model.as_str())
                .collect::<Vec<_>>(),
            ["model-a", "model-b", "model-c"]
        );

        order_priority_targets(&mut targets, 1);
        assert_eq!(
            targets
                .iter()
                .map(|target| target.model.as_str())
                .collect::<Vec<_>>(),
            ["model-b", "model-c", "model-a"]
        );

        order_priority_targets(&mut targets, 3);
        assert_eq!(
            targets
                .iter()
                .map(|target| target.model.as_str())
                .collect::<Vec<_>>(),
            ["model-a", "model-b", "model-c"]
        );
    }
}
