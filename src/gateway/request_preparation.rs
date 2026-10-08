use crate::{
    config::{GatewayResourceLimits, MAX_COMBO_NESTING_DEPTH, MAX_ROUTE_TARGETS},
    infra::storage::{ReadTxn, Record, StorageError, Table},
    infra::telemetry::RequestAnalytics,
    protocol::{self, Protocol, UpstreamProtocol},
    security::api_key_scope::ApiKeyScope,
    state::AppState,
    support::output_styles,
};
use axum::{
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde_json::Value;
use std::sync::Arc;

/// One dispatch target.
///
/// Target priority is not carried here: it only orders the list, and the stored
/// key order already does that once the route is flattened, so the order of the
/// list is the priority.
#[derive(Clone)]
pub(super) struct Target {
    pub(super) provider_id: String,
    pub(super) model: String,
    pub(super) protocol: Option<UpstreamProtocol>,
    pub(super) model_protocol: Option<UpstreamProtocol>,
    pub(super) enabled: bool,
}

pub(super) struct StoredRoute {
    pub(super) enabled: bool,
    pub(super) strategy: String,
    pub(super) accepted_protocols: Vec<String>,
    pub(super) targets: Vec<Target>,
    /// Set when the stored target list cannot be routed as written. The save
    /// path rejects every case, so this is the safety net for data that did not
    /// come through it: an imported backup, a hand-edited database, or a graph
    /// that changed after it was checked.
    pub(super) flatten_issue: Option<FlattenIssue>,
}

/// Why a stored route's nested combo references could not be flattened.
#[derive(Clone, Debug)]
pub(super) enum FlattenIssue {
    /// The flattened list would hold more targets than a request may use.
    TooManyTargets { expanded: usize },
    /// A nested reference points at a combo that is not stored.
    Missing { combo_id: String },
    /// Nested references form a loop.
    Cycle { combo_id: String },
    /// Nested references go deeper than the supported nesting.
    Depth { combo_id: String },
}

impl FlattenIssue {
    /// The combo the issue is about, when it is about one combo.
    fn combo_id(&self) -> Option<&str> {
        match self {
            Self::TooManyTargets { .. } => None,
            Self::Missing { combo_id } | Self::Cycle { combo_id } | Self::Depth { combo_id } => {
                Some(combo_id)
            }
        }
    }
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

#[derive(Debug)]
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
    /// Access scope of the authenticated key, enforced before dispatch.
    pub(super) api_key_scope: Option<Arc<ApiKeyScope>>,
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
        api_key_scope,
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
    let mut targets = resolve_gateway_targets(
        &state,
        &route_alias,
        client_protocol,
        target_rotation_offset,
    )
    .await?;
    // Enforce the key scope before any target is prepared or dispatched, so a
    // denied request never reaches an upstream provider.
    apply_api_key_scope(api_key_scope.as_deref(), &route_alias, &mut targets)?;

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

/// Applies the authenticated key's access scope to the candidate targets.
///
/// Model rules are matched against the client-requested model (a route alias or
/// an imported `provider/model` name), and provider rules drop targets the key
/// may not use. Model rules intentionally restrict the client-facing name only:
/// which upstream model an allowed alias maps to stays an operator decision,
/// while the provider rules bound where the request can land. A key without a
/// scope passes through unchanged.
fn apply_api_key_scope(
    scope: Option<&ApiKeyScope>,
    requested_model: &str,
    targets: &mut Vec<Target>,
) -> Result<(), GatewayRequestPreparationError> {
    let Some(scope) = scope else {
        return Ok(());
    };
    if !scope.allows_model(requested_model) {
        return Err(GatewayRequestPreparationError::Client {
            status: StatusCode::FORBIDDEN,
            message: format!("this API key is not allowed to use model '{requested_model}'"),
        });
    }
    if scope.allowed_provider_ids().is_empty() {
        return Ok(());
    }
    targets.retain(|target| scope.allows_provider(&target.provider_id));
    if targets.is_empty() {
        return Err(GatewayRequestPreparationError::Client {
            status: StatusCode::FORBIDDEN,
            message: format!(
                "this API key is not allowed to use any provider configured for '{requested_model}'"
            ),
        });
    }
    Ok(())
}

async fn resolve_gateway_targets(
    state: &AppState,
    route_alias: &str,
    client_protocol: Protocol,
    target_rotation_offset: usize,
) -> Result<Vec<Target>, GatewayRequestPreparationError> {
    let route = load_stored_route(&state.db, route_alias, client_protocol)
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
        if let Some(issue) = &route.flatten_issue {
            tracing::warn!(
                route = %route_alias,
                combo = ?issue.combo_id(),
                issue = ?issue,
                "stored route cannot be routed"
            );
            let message = match issue {
                FlattenIssue::TooManyTargets { expanded } => format!(
                    "route expands to {expanded} targets; the maximum is {MAX_ROUTE_TARGETS}"
                ),
                FlattenIssue::Missing { .. }
                | FlattenIssue::Cycle { .. }
                | FlattenIssue::Depth { .. } => {
                    "route has invalid nested combo references".to_owned()
                }
            };
            return Err(GatewayRequestPreparationError::Client {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message,
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

/// Rotates the target list for a retry.
///
/// The order is whatever `load_stored_route` produced, and it is already the
/// failover order: stored keys sort by `(priority, ordinal)` and a nested combo
/// is expanded in place at the row that referenced it. Sorting here would move
/// a nested group's leaves by their own priorities and break that order, so
/// only the rotation is applied.
fn order_priority_targets(targets: &mut [Target], target_rotation_offset: usize) {
    let Some(target_count) = (!targets.is_empty()).then_some(targets.len()) else {
        return;
    };
    targets.rotate_left(target_rotation_offset % target_count);
}

/// Loads a stored route and flattens its nested combo references into one
/// target list, in the order the operator configured.
///
/// A target that references another combo is replaced by that combo's own
/// targets, in place, recursively. A nested combo is skipped, with a log line,
/// when it is disabled or does not accept the client protocol — a routing
/// decision the operator can see and change, not a silent drop. Any structural
/// problem (missing combo, loop, too deep, too many expanded targets) sets
/// `flatten_issue`, which the caller turns into a 503.
pub(super) async fn load_stored_route(
    db: &crate::infra::storage::Database,
    route_id: &str,
    client_protocol: Protocol,
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
        let mut flattened = FlattenedTargets::default();
        let mut path = vec![route_id.clone()];
        flatten_route_targets(
            transaction,
            &route_id,
            client_protocol,
            &mut path,
            &mut flattened,
        )?;
        Ok(Some(StoredRoute {
            enabled: route.boolean("enabled")?,
            strategy: route.text("strategy")?.to_owned(),
            accepted_protocols,
            targets: flattened.targets,
            flatten_issue: flattened.issue,
        }))
    })
    .await
}

/// The target list accumulated while flattening, plus the first structural
/// problem found. Flattening stops at the first problem: the route is unroutable
/// either way and the partial list is never dispatched.
#[derive(Default)]
struct FlattenedTargets {
    targets: Vec<Target>,
    issue: Option<FlattenIssue>,
}

/// Appends the enabled provider targets of `route_id`, in stored order,
/// replacing each nested combo reference with that combo's targets.
///
/// `path` holds the combo ids currently being expanded, innermost last, which is
/// what detects a loop. Each combo in the path is entered once, so the recursion
/// depth is bounded by `MAX_COMBO_NESTING_DEPTH` and every scan is bounded by
/// `MAX_ROUTE_TARGETS`; a cycle therefore cannot recurse without end, and a
/// self-referencing combo is reported as a cycle.
fn flatten_route_targets(
    transaction: &ReadTxn<'_, '_>,
    route_id: &str,
    client_protocol: Protocol,
    path: &mut Vec<String>,
    out: &mut FlattenedTargets,
) -> Result<(), StorageError> {
    let records = transaction.scan_prefix::<Record>(
        Table::RouteTargets,
        &route_target_prefix(route_id)?,
        MAX_ROUTE_TARGETS + 1,
    )?;
    if records.len() > MAX_ROUTE_TARGETS {
        // More stored rows than any combo may declare, including disabled ones.
        // Fail closed rather than routing a silently truncated list.
        out.issue = Some(FlattenIssue::TooManyTargets {
            expanded: records.len(),
        });
        return Ok(());
    }
    for (_, record) in records {
        if out.issue.is_some() {
            return Ok(());
        }
        if !record.boolean("enabled")? {
            continue;
        }
        let Some(combo_id) = record.optional_text("combo_id")? else {
            out.targets.push(build_target(transaction, &record)?);
            out.check_expanded();
            continue;
        };
        let combo_id = combo_id.to_owned();
        if path.iter().any(|id| id == &combo_id) {
            out.issue = Some(FlattenIssue::Cycle { combo_id });
            return Ok(());
        }
        if path.len() > MAX_COMBO_NESTING_DEPTH {
            out.issue = Some(FlattenIssue::Depth { combo_id });
            return Ok(());
        }
        let Some(nested) = transaction.get::<Record>(Table::Routes, &combo_id)? else {
            out.issue = Some(FlattenIssue::Missing { combo_id });
            return Ok(());
        };
        if !nested.boolean("enabled")? {
            tracing::warn!(
                combo = %combo_id,
                referenced_by = %route_id,
                "nested combo is disabled; skipping its targets"
            );
            continue;
        }
        let accepted: Vec<String> = serde_json::from_str(nested.text("accepted_protocols")?)
            .map_err(|error| {
                StorageError::Codec(format!("nested combo protocol list is invalid: {error}"))
            })?;
        if !accepted
            .iter()
            .any(|protocol| protocol == client_protocol.as_str())
        {
            tracing::warn!(
                combo = %combo_id,
                referenced_by = %route_id,
                protocol = client_protocol.as_str(),
                "nested combo does not accept this protocol; skipping its targets"
            );
            continue;
        }
        path.push(combo_id.clone());
        flatten_route_targets(transaction, &combo_id, client_protocol, path, out)?;
        path.pop();
    }
    Ok(())
}

impl FlattenedTargets {
    /// Records the first expansion that exceeds the request target cap.
    fn check_expanded(&mut self) {
        if self.targets.len() > MAX_ROUTE_TARGETS {
            self.issue = Some(FlattenIssue::TooManyTargets {
                expanded: self.targets.len(),
            });
        }
    }
}

/// Builds one provider target from its stored row.
fn build_target(transaction: &ReadTxn<'_, '_>, target: &Record) -> Result<Target, StorageError> {
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
    let model_protocol =
        if let Some(model_record) = transaction.get::<Record>(Table::ProviderModels, &model_key)? {
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
    Ok(Target {
        provider_id,
        model,
        protocol,
        model_protocol,
        enabled: target.boolean("enabled")?,
    })
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
            enabled: true,
        }))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(model: &str) -> Target {
        Target {
            provider_id: "provider".to_owned(),
            model: model.to_owned(),
            protocol: None,
            model_protocol: None,
            enabled: true,
        }
    }

    fn models(targets: &[Target]) -> Vec<&str> {
        targets.iter().map(|target| target.model.as_str()).collect()
    }

    #[test]
    fn retries_rotate_the_next_model_to_the_front() {
        let mut targets = vec![target("model-a"), target("model-b"), target("model-c")];

        order_priority_targets(&mut targets, 0);
        assert_eq!(models(&targets), ["model-a", "model-b", "model-c"]);

        order_priority_targets(&mut targets, 1);
        assert_eq!(models(&targets), ["model-b", "model-c", "model-a"]);

        // The offset wraps, and it rotates the list as it stands: no re-sort.
        order_priority_targets(&mut targets, 3);
        assert_eq!(models(&targets), ["model-b", "model-c", "model-a"]);
    }

    #[test]
    fn rotation_preserves_the_configured_row_order() {
        // Target rows are stored in `(priority, ordinal)` key order and a nested
        // combo expands in place, so the list arrives in failover order and must
        // not be re-sorted by the leaf priorities.
        let mut targets = vec![
            target("model-late"),
            target("model-early"),
            target("model-mid"),
        ];

        order_priority_targets(&mut targets, 0);
        assert_eq!(
            models(&targets),
            ["model-late", "model-early", "model-mid"],
            "the helper must not re-sort by priority"
        );

        order_priority_targets(&mut targets, 2);
        assert_eq!(models(&targets), ["model-mid", "model-late", "model-early"]);

        order_priority_targets(&mut [], 3);
    }

    fn provider_target(provider_id: &str, model: &str) -> Target {
        Target {
            provider_id: provider_id.to_owned(),
            model: model.to_owned(),
            protocol: None,
            model_protocol: None,
            enabled: true,
        }
    }

    fn api_key_scope(providers: &[&str], models: &[&str]) -> ApiKeyScope {
        let providers: Vec<String> = providers.iter().map(|value| (*value).to_owned()).collect();
        let models: Vec<String> = models.iter().map(|value| (*value).to_owned()).collect();
        ApiKeyScope::from_entries(&providers, &models).expect("valid API key scope")
    }

    fn scope_error(error: GatewayRequestPreparationError) -> (StatusCode, String) {
        match error {
            GatewayRequestPreparationError::Client { status, message } => (status, message),
            GatewayRequestPreparationError::Database(_) => {
                panic!("expected a client error from the API key scope")
            }
        }
    }

    #[test]
    fn keys_without_a_scope_reach_every_target() {
        let mut targets = vec![
            provider_target("provider-a", "model-a"),
            provider_target("provider-b", "model-b"),
        ];
        apply_api_key_scope(None, "coding", &mut targets).expect("unscoped key passes");
        apply_api_key_scope(Some(&ApiKeyScope::default()), "coding", &mut targets)
            .expect("unrestricted scope passes");
        assert_eq!(targets.len(), 2);
    }

    #[test]
    fn model_rules_reject_requests_before_target_selection() {
        let scope = api_key_scope(&[], &["gpt-4*"]);
        let mut allowed = vec![provider_target("provider-a", "model-a")];
        apply_api_key_scope(Some(&scope), "gpt-4o", &mut allowed).expect("prefix rule allows");
        let mut denied = vec![provider_target("provider-a", "model-a")];
        let (status, message) = scope_error(
            apply_api_key_scope(Some(&scope), "claude-3-5-sonnet", &mut denied)
                .expect_err("model rule denies"),
        );
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(message.contains("claude-3-5-sonnet"), "{message}");
    }

    #[test]
    fn provider_rules_filter_targets_and_fail_when_none_remain() {
        let scope = api_key_scope(&["provider-b"], &[]);
        let mut targets = vec![
            provider_target("provider-a", "model-a"),
            provider_target("provider-b", "model-b"),
        ];
        apply_api_key_scope(Some(&scope), "coding", &mut targets).expect("allowed provider");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].provider_id, "provider-b");

        let scope = api_key_scope(&["provider-c"], &[]);
        let mut targets = vec![provider_target("provider-a", "model-a")];
        let (status, message) = scope_error(
            apply_api_key_scope(Some(&scope), "coding", &mut targets)
                .expect_err("provider rule denies"),
        );
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(message.contains("coding"), "{message}");
    }
}
