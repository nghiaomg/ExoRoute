//! Admin API for egress proxies.
//!
//! A proxy is a named, ordered list of endpoints the gateway may route an
//! upstream provider request through. Endpoints are either proxy servers (with
//! optional credentials) or embedded references to another proxy, which is how
//! one proxy groups several others. Providers bind a proxy by id.

use super::*;
use crate::config::MAX_PROXY_ENDPOINTS;
use crate::infra::storage::{Field, Record, StorageError, Table};

type ProxyApiResult<T> = Result<T, (StatusCode, Json<Value>)>;

mod backup;
mod bulk;
mod check;
mod gateway;
mod references;
mod storage;
mod validation;

pub(crate) use backup::{validate_backup_proxy_entries, validate_proxy_candidate_and_ancestors};
use bulk::BulkProxyValidationStore;
pub(crate) use check::check_proxy;
pub(crate) use gateway::{ProxyEndpointConfig, ProxyEndpointGroup, load_proxy_endpoint_configs};
use references::{
    PendingProxy, ProxyReferences, ensure_reference_graph, referencing_proxy_names,
    validate_proxy_references,
};
pub(crate) use references::{ProxyNode, ProxyStore, validate_stored_proxy_graph};
pub(crate) use storage::{
    proxy_endpoint_key_components, proxy_endpoint_ref_index_key, proxy_name_index_key,
    proxy_provider_index_key, proxy_provider_index_prefix,
};
use storage::{proxy_endpoint_prefix, proxy_endpoint_records, proxy_record, save_proxy};
pub(crate) use validation::normalize_proxy_url;
use validation::{apply_path_id_for_update, validate_proxy};

/// Sanity bound for endpoint scans. The declared bound is
/// `crate::config::MAX_PROXY_ENDPOINTS`; this page size only bounds one read.
pub(super) const MAX_PROXY_ENDPOINTS_PAGE: usize = 4096;
/// How many delete blockers are named in a 409 message.
pub(super) const MAX_DELETE_BLOCKERS_NAMED: usize = 5;
const MAX_BULK_PROXIES: usize = 200;
const PROXY_OPTIONS_PAGE_SIZE: usize = 200;
const MAX_PROXY_OPTIONS_SCAN: usize = 50_000;

fn yes() -> bool {
    true
}

fn priority() -> String {
    "priority".to_owned()
}

/// One endpoint as the API reads and writes it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct ProxyEndpointInput {
    /// Stable identity for secret preservation when the displayed order changes.
    #[serde(default)]
    pub endpoint_id: String,
    /// A proxy server URL. Accepts `host:port`, `http://`, and `https://`.
    #[serde(default)]
    pub url: Option<String>,
    /// Set for an embedded proxy; mutually exclusive with `url`.
    #[serde(default)]
    pub ref_proxy_id: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    /// Plaintext on input, encrypted at rest, and never returned on read.
    #[serde(default, skip_serializing)]
    pub secret: Option<String>,
    /// Ciphertext is populated server-side before storage.
    #[serde(skip)]
    secret_ciphertext: Option<Vec<u8>>,
    /// Whether a stored endpoint has a password, so the editor can show it
    /// without ever receiving the value.
    #[serde(default)]
    pub has_secret: bool,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub priority: u32,
    #[serde(default = "yes")]
    pub enabled: bool,
}

impl ProxyEndpointInput {
    fn normalized_reference(&self) -> Option<&str> {
        self.ref_proxy_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    fn normalized_url(&self) -> Result<String, StorageError> {
        normalize_proxy_url(self.url.as_deref().unwrap_or_default())
            .map_err(|message| StorageError::Invalid(message.to_owned()))
    }

    fn credential_is_set(&self) -> bool {
        self.username
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            || self
                .secret
                .as_deref()
                .is_some_and(|value| !value.is_empty())
    }
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct ProxyInput {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default = "priority")]
    pub strategy: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub endpoints: Vec<ProxyEndpointInput>,
}

/// Outcome counts of the last process-local live-check, exposed on
/// `ProxyView.last_check` so the existing list endpoint drives the chips.
#[derive(Debug, Serialize)]
pub(super) struct ProxyLastCheck {
    checked_at: u64,
    live_count: usize,
    die_count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct ProxyView {
    id: String,
    name: String,
    strategy: String,
    enabled: bool,
    created_at: String,
    updated_at: String,
    endpoints: Vec<ProxyEndpointInput>,
    /// Providers currently bound to this proxy, so the editor can warn before a
    /// delete is blocked.
    provider_count: usize,
    /// `None` until a live-check has run in this process; a restart or a
    /// proxy update/delete clears it.
    last_check: Option<ProxyLastCheck>,
}

#[derive(Debug, Deserialize)]
pub(super) struct BulkProxyInput {
    pub proxies: Vec<ProxyInput>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct ProxyOptionsQuery {
    pub q: Option<String>,
    pub cursor: Option<String>,
    pub include_id: Option<String>,
}

pub(super) async fn list_proxies(State(state): State<AppState>) -> ApiResult {
    let mut proxies = state
        .db
        .read(|transaction| read_all_proxies(transaction))
        .await
        .map_err(internal)?;
    let ids = proxies
        .iter()
        .map(|proxy| proxy.id.clone())
        .collect::<Vec<_>>();
    let checks = state.proxy_check_snapshot(&ids).await;
    for proxy in &mut proxies {
        if let Some(summary) = checks.get(&proxy.id) {
            proxy.last_check = Some(ProxyLastCheck {
                checked_at: summary.checked_at,
                live_count: summary.live_count,
                die_count: summary.die_count,
            });
        }
    }
    Ok(Json(json!({ "proxies": proxies })))
}

pub(super) async fn proxy_traffic_stats(State(state): State<AppState>) -> ApiResult {
    let proxy_ids = state
        .db
        .read(|transaction| {
            let entries = transaction.scan_prefix_after::<Record>(
                Table::Proxies,
                "",
                None,
                MAX_PROXY_OPTIONS_SCAN.saturating_add(1),
            )?;
            if entries.len() > MAX_PROXY_OPTIONS_SCAN {
                return Err(StorageError::Busy);
            }
            Ok(entries.into_iter().map(|(id, _)| id).collect::<Vec<_>>())
        })
        .await
        .map_err(internal)?;
    let (stats, truncated) = state.proxy_traffic_snapshot(&proxy_ids).await;
    let data = proxy_ids
        .into_iter()
        .map(|id| {
            let totals = stats.get(&id).copied().unwrap_or_default();
            let average_bytes_per_second = if totals.elapsed_millis == 0 {
                None
            } else {
                Some(
                    (u128::from(totals.received_bytes) + u128::from(totals.sent_bytes))
                        .saturating_mul(1000)
                        .checked_div(u128::from(totals.elapsed_millis))
                        .unwrap_or_default()
                        .min(u128::from(u64::MAX)) as u64,
                )
            };
            json!({
                "proxy_id": id,
                "received_bytes": totals.received_bytes,
                "sent_bytes": totals.sent_bytes,
                "total_bytes": totals.received_bytes.saturating_add(totals.sent_bytes),
                "requests": totals.requests,
                "average_bytes_per_second": average_bytes_per_second,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(
        json!({"proxies": data, "scope": "since_startup", "truncated": truncated}),
    ))
}

pub(super) async fn list_proxy_options(
    State(state): State<AppState>,
    Query(query): Query<ProxyOptionsQuery>,
) -> ApiResult {
    if query
        .cursor
        .as_deref()
        .is_some_and(|cursor| cursor.len() > 128)
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "proxy cursor must not exceed 128 bytes",
        ));
    }
    if query.q.as_deref().is_some_and(|q| q.len() > 128) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "proxy search must not exceed 128 bytes",
        ));
    }
    let cursor = query.cursor;
    let search = query
        .q
        .map(|q| q.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let include_id = query.include_id;
    let search_active = !search.is_empty();
    let (options, next_cursor) = state
        .db
        .read(move |transaction| {
            let entries = transaction.scan_prefix_after::<Record>(
                Table::Proxies,
                "",
                cursor.as_deref(),
                MAX_PROXY_OPTIONS_SCAN + 1,
            )?;
            if entries.len() > MAX_PROXY_OPTIONS_SCAN {
                return Err(StorageError::Busy);
            }
            let mut options = Vec::with_capacity(PROXY_OPTIONS_PAGE_SIZE);
            let mut last_scanned: Option<String> = None;
            let mut has_more = false;
            for (id, record) in entries {
                let name = record.text("name")?.to_owned();
                let matches = search.is_empty()
                    || name.to_ascii_lowercase().contains(&search)
                    || id.to_ascii_lowercase().contains(&search);
                if matches && options.len() >= PROXY_OPTIONS_PAGE_SIZE {
                    has_more = true;
                    break;
                }
                last_scanned = Some(id.clone());
                if matches {
                    options.push(json!({"id": id, "name": name, "enabled": record.boolean("enabled")?}));
                }
            }
            let next_cursor = if has_more && !search_active { last_scanned } else { None };
            if let Some(include_id) = include_id.as_deref() {
                if !options.iter().any(|option| option["id"] == include_id) {
                    if let Some(record) = transaction.get::<Record>(Table::Proxies, include_id)? {
                        options.push(json!({"id": include_id, "name": record.text("name")?, "enabled": record.boolean("enabled")?}));
                    }
                }
            }
            Ok((options, next_cursor))
        })
        .await
        .map_err(internal)?;
    Ok(Json(
        json!({ "proxies": options, "next_cursor": next_cursor }),
    ))
}

pub(super) async fn create_proxy(
    State(state): State<AppState>,
    Json(input): Json<ProxyInput>,
) -> ApiResult {
    let id = create_item(state, input).await?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

pub(super) async fn bulk_create_proxies(
    State(state): State<AppState>,
    Json(input): Json<BulkProxyInput>,
) -> ApiResult {
    if input.proxies.is_empty() {
        return Err(fail(StatusCode::BAD_REQUEST, "no proxies were provided"));
    }
    if input.proxies.len() > MAX_BULK_PROXIES {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            format!("at most {MAX_BULK_PROXIES} proxies may be created at once"),
        ));
    }
    if input
        .proxies
        .iter()
        .any(|proxy| proxy.endpoints.len() > MAX_PROXY_ENDPOINTS)
        || input
            .proxies
            .iter()
            .flat_map(|proxy| &proxy.endpoints)
            .any(|endpoint| {
                endpoint.secret.as_deref().is_some_and(|secret| {
                    secret.len() > crate::admin::proxies::validation::MAX_PROXY_SECRET_BYTES
                }) || endpoint.url.as_deref().is_some_and(|url| {
                    url.len() > crate::admin::proxies::validation::MAX_PROXY_ENDPOINT_URL_BYTES
                }) || endpoint.username.as_deref().is_some_and(|username| {
                    username.len() > crate::admin::proxies::validation::MAX_PROXY_USERNAME_BYTES
                })
            })
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "proxy endpoint input exceeds request limits",
        ));
    }
    let mut prepared = Vec::with_capacity(input.proxies.len());
    let mut ids = std::collections::HashSet::with_capacity(input.proxies.len());
    for mut proxy in input.proxies {
        if proxy.id.trim().is_empty() {
            proxy.id = resource_id(&proxy.name);
        }
        for endpoint in &mut proxy.endpoints {
            endpoint.priority = endpoint.priority.min(MAX_PROXY_ENDPOINTS as u32);
        }
        validate_proxy(&mut proxy).map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
        if !ids.insert(proxy.id.clone()) {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "bulk proxy IDs must be unique",
            ));
        }
        for endpoint in proxy.endpoints.iter_mut() {
            endpoint.secret_ciphertext = crate::security::encrypt_secret(
                state.config.master_key.as_ref(),
                endpoint.secret.as_deref().unwrap_or_default(),
            )
            .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
            endpoint.has_secret = endpoint.secret_ciphertext.is_some();
        }
        let id = proxy.id.clone();
        let pending = PendingProxy::from_input(&proxy);
        let record = proxy_record(&proxy).map_err(internal)?;
        let name_index = proxy_name_index_key(&proxy.name, &id).map_err(internal)?;
        let endpoints = proxy_endpoint_records(&proxy).map_err(internal)?;
        prepared.push((id, pending, record, name_index, endpoints));
    }

    let state_for_validation = state.clone();
    let mut prior: Vec<(String, PendingProxy)> = Vec::with_capacity(prepared.len());
    for (id, pending, _, _, _) in &prepared {
        let id_for_validation = id.clone();
        let pending_for_validation = pending.clone();
        let prior_for_validation = prior.clone();
        state_for_validation
            .db
            .read(move |transaction| {
                let store = BulkProxyValidationStore::new(
                    transaction,
                    &id_for_validation,
                    &pending_for_validation,
                    &prior_for_validation,
                );
                references::validate_pending_proxy(
                    &store,
                    &id_for_validation,
                    &pending_for_validation,
                )
                .map_err(|error| match error {
                    references::ProxyReferenceError::Storage(error) => error,
                    references::ProxyReferenceError::Issue(issue) => {
                        StorageError::Invalid(references::proxy_issue_message(&issue))
                    }
                })
            })
            .await
            .map_err(|error| match error {
                StorageError::Invalid(message) => fail(StatusCode::BAD_REQUEST, message),
                other => internal(other),
            })?;
        prior.push((id.clone(), pending.clone()));
    }

    let ids: Vec<String> = prepared.iter().map(|item| item.0.clone()).collect();
    let count = prepared.len();

    state
        .db
        .write(move |transaction| {
            let mut prior = Vec::<(String, PendingProxy)>::with_capacity(count);
            for (id, pending, record, name_index, endpoints) in &prepared {
                if transaction.get::<Record>(Table::Proxies, id)?.is_some() {
                    return Err(StorageError::Conflict);
                }
                let store = BulkProxyValidationStore::new(&*transaction, id, pending, &prior);
                ensure_reference_graph(&store, id, pending)?;
                transaction.put_if_absent(Table::Proxies, id, record)?;
                transaction.put_if_absent(Table::ProxyNameIndex, name_index, id)?;
                for endpoint in endpoints {
                    transaction.put_if_absent(
                        Table::ProxyEndpoints,
                        &endpoint.endpoint_key,
                        &endpoint.record,
                    )?;
                    if let Some(reference_index) = endpoint.reference_index.as_deref() {
                        transaction.put(Table::ProxyEndpointRefIndex, reference_index, id)?;
                    }
                }
                prior.push((id.clone(), pending.clone()));
            }
            Ok(())
        })
        .await
        .map_err(|error| match error {
            StorageError::Conflict => fail(
                StatusCode::CONFLICT,
                "a proxy in the bulk request already exists",
            ),
            StorageError::Invalid(message) => fail(StatusCode::BAD_REQUEST, message),
            other => internal(other),
        })?;
    Ok(Json(json!({ "ok": true, "created": count, "ids": ids })))
}

async fn create_item(state: AppState, mut input: ProxyInput) -> ProxyApiResult<String> {
    validate_proxy(&mut input).map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    if input.id.trim().is_empty() {
        input.id = resource_id(&input.name);
    }
    let id = input.id.clone();
    let write_id = id.clone();
    for endpoint in input.endpoints.iter_mut() {
        endpoint.secret_ciphertext = crate::security::encrypt_secret(
            state.config.master_key.as_ref(),
            endpoint.secret.as_deref().unwrap_or_default(),
        )
        .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
        endpoint.has_secret = endpoint.secret_ciphertext.is_some();
    }
    let pending = PendingProxy::from_input(&input);
    validate_proxy_references(&state, id.clone(), pending.clone()).await?;
    let proxy = proxy_record(&input).map_err(internal)?;
    let name_index = proxy_name_index_key(&input.name, &id).map_err(internal)?;
    let endpoints = proxy_endpoint_records(&input).map_err(internal)?;
    state
        .db
        .write(move |transaction| {
            if transaction
                .get::<Record>(Table::Proxies, &write_id)?
                .is_some()
            {
                return Err(StorageError::Conflict);
            }
            ensure_reference_graph(&*transaction, &write_id, &pending)?;
            save_proxy(
                transaction,
                &write_id,
                &proxy,
                &name_index,
                &endpoints,
                false,
            )
        })
        .await
        .map_err(|error| match error {
            StorageError::Conflict => {
                fail(StatusCode::CONFLICT, "a proxy with this ID already exists")
            }
            other => internal(other),
        })?;
    Ok(id)
}

pub(super) async fn update_proxy(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut input): Json<ProxyInput>,
) -> ApiResult {
    apply_path_id_for_update(&id, &mut input)
        .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    validate_proxy(&mut input).map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    for endpoint in input.endpoints.iter_mut() {
        endpoint.secret_ciphertext = crate::security::encrypt_secret(
            state.config.master_key.as_ref(),
            endpoint.secret.as_deref().unwrap_or_default(),
        )
        .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
        endpoint.has_secret |= endpoint.secret_ciphertext.is_some();
    }
    let id = input.id.clone();
    let pending = PendingProxy::from_input(&input);
    validate_proxy_references(&state, id.clone(), pending.clone()).await?;
    let proxy = proxy_record(&input).map_err(internal)?;
    let name_index = proxy_name_index_key(&input.name, &id).map_err(internal)?;
    let endpoints = proxy_endpoint_records(&input).map_err(internal)?;
    let cleared_id = id.clone();
    let found = state
        .db
        .write(move |transaction| {
            let Some(current) = transaction.get::<Record>(Table::Proxies, &id)? else {
                return Ok(false);
            };
            ensure_reference_graph(&*transaction, &id, &pending)?;
            let old_name_index = proxy_name_index_key(current.text("name")?, &id)?;
            transaction.delete(Table::ProxyNameIndex, &old_name_index)?;
            let mut updated = proxy.clone();
            updated.insert(
                "created_at",
                Field::Text(current.text("created_at")?.to_owned()),
            );
            save_proxy(transaction, &id, &updated, &name_index, &endpoints, true)?;
            Ok(true)
        })
        .await
        .map_err(internal)?;
    if !found {
        return Err(fail(StatusCode::NOT_FOUND, "proxy not found"));
    }
    // Edited endpoints invalidate the process-local live-check result.
    state.clear_proxy_check_state(&cleared_id).await;
    Ok(Json(json!({ "ok": true })))
}

pub(super) async fn delete_proxy(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult {
    let proxy_id = id.clone();
    let outcome = state
        .db
        .write(move |transaction| {
            let Some(proxy) = transaction.get::<Record>(Table::Proxies, &id)? else {
                return Ok(DeleteOutcome::Missing);
            };
            if !transaction.referencing_proxies(&id)?.is_empty() {
                return Ok(DeleteOutcome::Referenced);
            }
            let provider_bindings = transaction.scan_prefix::<String>(
                Table::ProxyProviderIndex,
                &proxy_provider_index_prefix(&id)?,
                MAX_PROXY_ENDPOINTS_PAGE,
            )?;
            if !provider_bindings.is_empty() {
                return Ok(DeleteOutcome::BoundToProvider);
            }
            let name_index = proxy_name_index_key(proxy.text("name")?, &id)?;
            transaction.delete(Table::Proxies, &id)?;
            transaction.delete(Table::ProxyNameIndex, &name_index)?;
            let endpoints = transaction.scan_prefix::<Record>(
                Table::ProxyEndpoints,
                &proxy_endpoint_prefix(&id)?,
                MAX_PROXY_ENDPOINTS_PAGE,
            )?;
            for (endpoint_key, endpoint) in endpoints {
                transaction.delete(Table::ProxyEndpoints, &endpoint_key)?;
                if let Some(reference) = endpoint.optional_text("ref_proxy_id")? {
                    transaction.delete(
                        Table::ProxyEndpointRefIndex,
                        &proxy_endpoint_ref_index_key(reference, &endpoint_key)?,
                    )?;
                }
            }
            Ok(DeleteOutcome::Deleted)
        })
        .await
        .map_err(internal)?;
    match outcome {
        DeleteOutcome::Missing => Err(fail(StatusCode::NOT_FOUND, "proxy not found")),
        DeleteOutcome::Referenced => Err(delete_blocked_error(&state, &proxy_id).await),
        DeleteOutcome::BoundToProvider => Err(fail(
            StatusCode::CONFLICT,
            "This proxy is bound to a provider. Unbind it before deleting the proxy.",
        )),
        DeleteOutcome::Deleted => {
            state.clear_proxy_runtime_state(&proxy_id).await;
            Ok(Json(json!({"ok": true})))
        }
    }
}

/// Why a proxy deletion did or did not happen.
enum DeleteOutcome {
    Deleted,
    Missing,
    Referenced,
    BoundToProvider,
}

async fn delete_blocked_error(state: &AppState, proxy_id: &str) -> (StatusCode, Json<Value>) {
    let proxy_id = proxy_id.to_owned();
    let names = state
        .db
        .read(move |transaction| referencing_proxy_names(transaction, &proxy_id))
        .await
        .unwrap_or_default();
    let message = match names.len() {
        0 => "This proxy is embedded by another proxy. Remove the embedded references before deleting it.".to_owned(),
        1 => format!(
            "This proxy is embedded by {}. Remove the embedded reference before deleting it.",
            names[0]
        ),
        _ => format!(
            "This proxy is embedded by {}. Remove the embedded references before deleting it.",
            names.join(", ")
        ),
    };
    fail(StatusCode::CONFLICT, message)
}

/// Rebuilds every proxy with its endpoint list in name order.
fn read_all_proxies(
    transaction: &crate::infra::storage::ReadTxn<'_, '_>,
) -> Result<Vec<ProxyView>, StorageError> {
    let entries = transaction.scan_prefix_after::<Record>(
        Table::Proxies,
        "",
        None,
        MAX_PROXY_OPTIONS_SCAN.saturating_add(1),
    )?;
    if entries.len() > MAX_PROXY_OPTIONS_SCAN {
        return Err(StorageError::Busy);
    }
    let mut proxies = Vec::with_capacity(entries.len());
    for (id, record) in entries {
        let Some(record) = transaction.get::<Record>(Table::Proxies, &id)? else {
            return Err(StorageError::Invalid(
                "proxy name index points to a missing proxy".to_owned(),
            ));
        };
        let endpoint_records = transaction.scan_prefix::<Record>(
            Table::ProxyEndpoints,
            &proxy_endpoint_prefix(&id)?,
            MAX_PROXY_ENDPOINTS_PAGE,
        )?;
        let mut endpoints = Vec::with_capacity(endpoint_records.len());
        for (_, endpoint) in endpoint_records {
            endpoints.push(ProxyEndpointInput {
                endpoint_id: endpoint.text("endpoint_id")?.to_owned(),
                url: endpoint.optional_text("url")?.map(str::to_owned),
                ref_proxy_id: endpoint.optional_text("ref_proxy_id")?.map(str::to_owned),
                username: endpoint.optional_text("username")?.map(str::to_owned),
                // The stored value is ciphertext: only its presence is exposed.
                secret: None,
                secret_ciphertext: None,
                has_secret: endpoint.optional_bytes("secret")?.is_some(),
                label: endpoint.optional_text("label")?.map(str::to_owned),
                priority: u32::try_from(endpoint.integer("priority")?).map_err(|_| {
                    StorageError::Invalid("proxy endpoint priority is invalid".to_owned())
                })?,
                enabled: endpoint.boolean("enabled")?,
            });
        }
        let provider_count = transaction
            .scan_prefix::<String>(
                Table::ProxyProviderIndex,
                &proxy_provider_index_prefix(&id)?,
                MAX_PROXY_ENDPOINTS_PAGE,
            )?
            .len();
        proxies.push(ProxyView {
            id,
            name: record.text("name")?.to_owned(),
            strategy: record.text("strategy")?.to_owned(),
            enabled: record.boolean("enabled")?,
            created_at: record.text("created_at")?.to_owned(),
            updated_at: record.text("updated_at")?.to_owned(),
            endpoints,
            provider_count,
            last_check: None,
        });
    }
    Ok(proxies)
}

/// Whether a stored proxy record is mid-deletion. Reserved for a future delete
/// marker; the current delete removes the record in one transaction, so this is
/// always false and exists so `save_proxy` has one guard to extend.
pub(super) fn ensure_proxy_not_deleting(_proxy: &Record) -> Result<(), StorageError> {
    Ok(())
}

/// Provider-side helper: validates that a proxy id exists and may be bound.
pub(crate) fn ensure_proxy_bindable(
    transaction: &crate::infra::storage::ReadTxn<'_, '_>,
    proxy_id: &str,
) -> Result<(), StorageError> {
    if transaction
        .get::<Record>(Table::Proxies, proxy_id)?
        .is_none()
    {
        return Err(StorageError::Invalid(
            "the selected proxy does not exist".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_proxy_bindable_write(
    transaction: &crate::infra::storage::WriteTxn<'_, '_>,
    proxy_id: &str,
) -> Result<(), StorageError> {
    let Some(proxy) = transaction.get::<Record>(Table::Proxies, proxy_id)? else {
        return Err(StorageError::NotFound);
    };
    ensure_proxy_not_deleting(&proxy)
}

/// Provider-side helper: records or clears one provider's proxy binding.
pub(crate) fn set_provider_proxy_binding(
    transaction: &mut crate::infra::storage::WriteTxn<'_, '_>,
    provider_id: &str,
    previous_proxy_id: Option<&str>,
    next_proxy_id: Option<&str>,
) -> Result<(), StorageError> {
    if let Some(previous) = previous_proxy_id {
        if Some(previous) != next_proxy_id {
            transaction.delete(
                Table::ProxyProviderIndex,
                &proxy_provider_index_key(previous, provider_id)?,
            )?;
        }
    }
    if let Some(next) = next_proxy_id {
        if Some(next) != previous_proxy_id {
            transaction.put(
                Table::ProxyProviderIndex,
                &proxy_provider_index_key(next, provider_id)?,
                &provider_id.to_owned(),
            )?;
        }
    }
    Ok(())
}

/// Provider-side helper: validates every proxy a bulk import would bind.
pub(super) fn proxy_endpoint_priority_bound() -> u32 {
    MAX_PROXY_ENDPOINTS as u32
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
