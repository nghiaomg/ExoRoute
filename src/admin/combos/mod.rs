use super::*;
use crate::infra::storage::{Field, Record, StorageError, Table};

mod storage;
mod validation;

#[cfg(test)]
pub(super) use storage::route_target_key;
pub(super) use storage::{
    route_name_index_key, route_target_provider_index_key, route_target_provider_index_prefix,
};
use storage::{route_record, route_target_prefix, route_target_records, save_combo};
use validation::{apply_path_id_for_update, validate_combo, validate_saved_combo_models};

const MAX_COMBO_TARGETS: usize = 32;
pub(super) const MAX_ROUTE_TARGETS: usize = 10_000;
const MAX_ROUTE_ID_BYTES: usize = 128;
const MAX_PROVIDER_ID_BYTES: usize = 128;
const MAX_MODEL_BYTES: usize = 256;
const COMBO_PROVIDER_OPTIONS_PAGE_SIZE: usize = 200;
const MAX_COMBO_PROVIDER_CURSOR_BYTES: usize = 256;
const ROUTE_NAME_INDEX_PREFIX: &str = "name/";

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ComboTargetInput {
    pub provider_id: String,
    pub model: String,
    pub protocol: Option<String>,
    #[serde(default)]
    pub priority: u32,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub weight: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ComboInput {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default = "priority")]
    pub strategy: String,
    #[serde(default = "all_protocols")]
    pub accepted_protocols: Vec<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub targets: Vec<ComboTargetInput>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct ComboProviderOptionsQuery {
    pub(super) cursor: Option<String>,
}

#[derive(Debug, Serialize)]
struct ComboProviderOption {
    id: String,
    name: String,
    adapter_id: String,
    supported_upstream_protocols: Vec<&'static str>,
    enabled: bool,
}

fn priority() -> String {
    "priority".to_owned()
}

fn all_protocols() -> Vec<String> {
    vec![
        "chat_completions".into(),
        "responses".into(),
        "messages".into(),
    ]
}

#[derive(Debug, Serialize)]
struct ComboView {
    id: String,
    name: String,
    strategy: String,
    accepted_protocols: Vec<String>,
    enabled: bool,
    targets: Vec<ComboTargetInput>,
}

pub(super) async fn list_routes(State(state): State<AppState>) -> ApiResult {
    list_items(state, false).await
}

pub(super) async fn list_combos(State(state): State<AppState>) -> ApiResult {
    list_items(state, true).await
}

pub(super) async fn list_provider_options(
    State(state): State<AppState>,
    Query(query): Query<ComboProviderOptionsQuery>,
) -> ApiResult {
    if query
        .cursor
        .as_deref()
        .is_some_and(|cursor| cursor.len() > MAX_COMBO_PROVIDER_CURSOR_BYTES)
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider cursor must not exceed 256 bytes",
        ));
    }
    let cursor = query.cursor;
    let (providers, next_cursor) = state
        .db
        .read(move |transaction| {
            let rows = transaction.scan_prefix_after::<Record>(
                Table::Providers,
                "",
                cursor.as_deref(),
                COMBO_PROVIDER_OPTIONS_PAGE_SIZE + 1,
            )?;
            let has_more = rows.len() > COMBO_PROVIDER_OPTIONS_PAGE_SIZE;
            let mut providers =
                Vec::with_capacity(rows.len().min(COMBO_PROVIDER_OPTIONS_PAGE_SIZE));
            for (_, record) in rows.into_iter().take(COMBO_PROVIDER_OPTIONS_PAGE_SIZE) {
                if super::providers::provider_is_deleting(&record)? {
                    continue;
                }
                providers.push(ComboProviderOption {
                    id: record.text("id")?.to_owned(),
                    name: record.text("name")?.to_owned(),
                    adapter_id: record.text("adapter_id")?.to_owned(),
                    supported_upstream_protocols:
                        crate::provider_adapters::supported_upstream_protocols(
                            record.text("adapter_id")?,
                        )
                        .to_vec(),
                    enabled: record.boolean("enabled")?,
                });
            }
            let next_cursor = if has_more {
                providers.last().map(|provider| provider.id.clone())
            } else {
                None
            };
            Ok((providers, next_cursor))
        })
        .await
        .map_err(internal)?;
    Ok(Json(
        json!({"providers":providers,"next_cursor":next_cursor}),
    ))
}

async fn list_items(state: AppState, combos: bool) -> ApiResult {
    let items = state
        .db
        .read(move |transaction| {
            let routes = transaction.scan_prefix::<String>(
                Table::RouteNameIndex,
                ROUTE_NAME_INDEX_PREFIX,
                10_000,
            )?;
            if routes.len() == 10_000 {
                return Err(StorageError::Busy);
            }
            let mut items = Vec::with_capacity(routes.len());
            for (_, id) in routes {
                let record = transaction
                    .get::<Record>(Table::Routes, &id)?
                    .ok_or_else(|| {
                        StorageError::Invalid(
                            "route name index points to a missing route".to_owned(),
                        )
                    })?;
                let target_records = transaction.scan_prefix::<Record>(
                    Table::RouteTargets,
                    &route_target_prefix(&id)?,
                    MAX_ROUTE_TARGETS,
                )?;
                let mut targets = Vec::with_capacity(target_records.len());
                for (_, target) in target_records {
                    targets.push(ComboTargetInput {
                        provider_id: target.text("provider_id")?.to_owned(),
                        model: target.text("model")?.to_owned(),
                        protocol: target.optional_text("protocol")?.map(str::to_owned),
                        priority: u32::try_from(target.integer("priority")?).map_err(|_| {
                            StorageError::Invalid("route target priority is invalid".to_owned())
                        })?,
                        enabled: target.boolean("enabled")?,
                        weight: target
                            .optional_integer("weight")?
                            .map(u32::try_from)
                            .transpose()
                            .map_err(|_| {
                                StorageError::Invalid("route target weight is invalid".to_owned())
                            })?,
                    });
                }
                let protocols: Vec<String> =
                    serde_json::from_str(record.text("accepted_protocols")?)
                        .map_err(|error| StorageError::Codec(error.to_string()))?;
                items.push(ComboView {
                    id,
                    name: record.text("name")?.to_owned(),
                    strategy: record.text("strategy")?.to_owned(),
                    accepted_protocols: protocols,
                    enabled: record.boolean("enabled")?,
                    targets,
                });
            }
            Ok(items)
        })
        .await
        .map_err(internal)?;
    let mut response = serde_json::Map::new();
    response.insert(
        if combos { "combos" } else { "routes" }.to_owned(),
        json!(items),
    );
    Ok(Json(Value::Object(response)))
}

pub(super) async fn create_route(
    State(state): State<AppState>,
    Json(input): Json<ComboInput>,
) -> ApiResult {
    create_item(state, input, None).await
}

pub(super) async fn create_combo(
    State(state): State<AppState>,
    Json(input): Json<ComboInput>,
) -> ApiResult {
    create_item(state, input, Some(MAX_COMBO_TARGETS)).await
}

async fn create_item(
    state: AppState,
    mut input: ComboInput,
    max_targets: Option<usize>,
) -> ApiResult {
    validate_combo(&input, max_targets)?;
    validate_saved_combo_models(&state, &input).await?;
    if input.id.trim().is_empty() {
        input.id = resource_id(&input.name);
    }
    let id = input.id.clone();
    let route = route_record(&input).map_err(internal)?;
    let name_index = route_name_index_key(&input.name, &id).map_err(internal)?;
    let targets = route_target_records(&input).map_err(internal)?;
    state
        .db
        .write(move |transaction| {
            if transaction.get::<Record>(Table::Routes, &id)?.is_some() {
                return Err(StorageError::Conflict);
            }
            save_combo(transaction, &id, &route, &name_index, &targets, false)
        })
        .await
        .map_err(|error| {
            if error.is_conflict() {
                fail(
                    StatusCode::CONFLICT,
                    "A route with this ID already exists or a target provider is being deleted.",
                )
            } else {
                internal(error)
            }
        })?;
    let combo_id = input.id;
    Ok(Json(
        json!({"ok":true,"id":combo_id.clone(),"combo_id":combo_id}),
    ))
}

pub(super) async fn update_route(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ComboInput>,
) -> ApiResult {
    update_item(state, id, input, None).await
}

pub(super) async fn update_combo(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ComboInput>,
) -> ApiResult {
    update_item(state, id, input, Some(MAX_COMBO_TARGETS)).await
}

async fn update_item(
    state: AppState,
    id: String,
    mut input: ComboInput,
    max_targets: Option<usize>,
) -> ApiResult {
    apply_path_id_for_update(&id, &mut input)
        .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    validate_combo(&input, max_targets)?;
    validate_saved_combo_models(&state, &input).await?;
    let route = route_record(&input).map_err(internal)?;
    let name_index = route_name_index_key(&input.name, &id).map_err(internal)?;
    let targets = route_target_records(&input).map_err(internal)?;
    let id_for_write = id.clone();
    let updated = state
        .db
        .write(move |transaction| {
            let Some(old_route) = transaction.get::<Record>(Table::Routes, &id_for_write)? else {
                return Ok(false);
            };
            let old_name_index = route_name_index_key(old_route.text("name")?, &id_for_write)?;
            transaction.delete(Table::RouteNameIndex, &old_name_index)?;
            let mut updated_route = route.clone();
            updated_route.insert(
                "created_at",
                Field::Text(old_route.text("created_at")?.to_owned()),
            );
            save_combo(
                transaction,
                &id_for_write,
                &updated_route,
                &name_index,
                &targets,
                true,
            )?;
            Ok(true)
        })
        .await
        .map_err(internal)?;
    if !updated {
        return Err(fail(StatusCode::NOT_FOUND, "combo not found"));
    }
    state.clear_route_runtime_state(&id).await;
    Ok(Json(json!({"ok":true})))
}

pub(super) async fn delete_route(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult {
    delete_item(state, id).await
}

pub(super) async fn delete_combo(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult {
    delete_item(state, id).await
}

async fn delete_item(state: AppState, id: String) -> ApiResult {
    let route_id = id.clone();
    let deleted = state
        .db
        .write(move |transaction| {
            let Some(route) = transaction.get::<Record>(Table::Routes, &id)? else {
                return Ok(false);
            };
            let name_index = route_name_index_key(route.text("name")?, &id)?;
            transaction.delete(Table::Routes, &id)?;
            transaction.delete(Table::RouteNameIndex, &name_index)?;
            let targets = transaction.scan_prefix::<Record>(
                Table::RouteTargets,
                &route_target_prefix(&id)?,
                MAX_ROUTE_TARGETS,
            )?;
            for (target_key, target) in targets {
                transaction.delete(Table::RouteTargets, &target_key)?;
                let provider_id = target.text("provider_id")?;
                transaction.delete(
                    Table::RouteTargetProviderIndex,
                    &route_target_provider_index_key(provider_id, &target_key)?,
                )?;
            }
            Ok(true)
        })
        .await
        .map_err(internal)?;
    if !deleted {
        return Err(fail(StatusCode::NOT_FOUND, "combo not found"));
    }
    state.clear_route_runtime_state(&route_id).await;
    Ok(Json(json!({"ok":true})))
}

pub(super) async fn list_models(State(state): State<AppState>) -> ApiResult {
    let models = state
        .db
        .read(|transaction| {
            let targets = transaction.scan_prefix::<Record>(Table::RouteTargets, "", 100_001)?;
            if targets.len() > 100_000 {
                return Err(StorageError::Busy);
            }
            let mut routes = std::collections::HashMap::<String, Record>::new();
            let mut providers = std::collections::HashMap::<String, Record>::new();
            let mut models = Vec::with_capacity(targets.len());
            for (target_key, target) in targets {
                let route_id = target.text("route_id")?.to_owned();
                let provider_id = target.text("provider_id")?.to_owned();
                if !routes.contains_key(&route_id)
                    && let Some(route) = transaction.get::<Record>(Table::Routes, &route_id)?
                {
                    routes.insert(route_id.clone(), route);
                }
                if !providers.contains_key(&provider_id)
                    && let Some(provider) =
                        transaction.get::<Record>(Table::Providers, &provider_id)?
                {
                    providers.insert(provider_id.clone(), provider);
                }
                let (Some(route), Some(provider)) =
                    (routes.get(&route_id), providers.get(&provider_id))
                else {
                    continue;
                };
                let route_name = route.text("name")?.to_owned();
                let priority = target.integer("priority")?;
                models.push((
                    route_name.clone(),
                    priority,
                    target_key,
                    json!({
                        "combo_id": route_id,
                        "combo_name": route_name,
                        "route_id": route_id,
                        "route_name": route.text("name")?,
                        "provider_id": provider_id,
                        "provider_name": provider.text("name")?,
                        "model": target.text("model")?,
                        "enabled": target.boolean("enabled")?
                    }),
                ));
            }
            models.sort_by(|left, right| {
                left.0
                    .cmp(&right.0)
                    .then_with(|| left.1.cmp(&right.1))
                    .then_with(|| left.2.cmp(&right.2))
            });
            Ok(models
                .into_iter()
                .map(|(_, _, _, value)| value)
                .collect::<Vec<_>>())
        })
        .await
        .map_err(internal)?;
    Ok(Json(json!({"models":models})))
}

#[cfg(test)]
mod tests;
