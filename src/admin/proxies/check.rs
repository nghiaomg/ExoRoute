//! Diagnostic live-check of proxy endpoints.
//!
//! The dashboard asks the gateway to probe every enabled endpoint of one proxy
//! once. Each probe sends a single HEAD request to a fixed public target
//! *through* the endpoint's proxy: any HTTP response counts as live, and every
//! transport failure counts as die with a short classified message. Results are
//! kept in process memory only (`state::ProxyCheckRegistry`), never persisted,
//! and never contain credentials or response bodies.

use super::*;
use crate::state::{ProxyCheckPermitOutcome, ProxyEndpointCheckOutcome};
use futures_util::StreamExt;
use std::time::Instant;

/// The fixed target every check probes. Operators cannot choose it, so the
/// check never turns an admin request into a request-forger against an
/// arbitrary destination.
pub(super) const DEFAULT_CHECK_TARGET: &str = "https://www.gstatic.com/generate_204";
/// Connect deadline for one endpoint probe. The overall timeout below spans
/// connect plus request, so one endpoint check never exceeds it.
pub(super) const CHECK_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// Overall deadline for one endpoint probe.
pub(super) const CHECK_TOTAL_TIMEOUT: Duration = Duration::from_secs(5);
/// Endpoint probes of one check run with bounded concurrency.
pub(super) const CHECK_MAX_CONCURRENCY: usize = 8;

/// One concrete endpoint prepared for a check, with its credential decrypted.
/// The secret lives only in this request-scoped value.
pub(super) struct CheckableEndpoint {
    pub(super) endpoint_id: String,
    pub(super) label: Option<String>,
    pub(super) url: String,
    pub(super) username: Option<String>,
    pub(super) secret: Option<String>,
}

/// One endpoint outcome of a check.
#[derive(Debug)]
pub(super) struct EndpointCheckResult {
    endpoint_id: String,
    label: Option<String>,
    url: String,
    pub live: bool,
    pub latency_millis: u64,
    /// Classified failure text, or `None` for a live endpoint.
    pub error: Option<String>,
}

pub(crate) async fn check_proxy(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult {
    let master_key = state.config.master_key.clone();
    let proxy_id = id.clone();
    let expanded = state
        .db
        .read(
            move |transaction| match load_proxy_endpoint_configs(transaction, &proxy_id) {
                Ok(group) => Ok(Some(prepare_endpoints(&group, master_key.as_ref()))),
                Err(StorageError::NotFound) => Ok(None),
                Err(error) => Err(error),
            },
        )
        .await
        .map_err(internal)?;
    let Some((checkable, mut prepared)) = expanded else {
        return Err(fail(StatusCode::NOT_FOUND, "proxy not found"));
    };

    // The permit is acquired after the 404 check and before any network work,
    // so a second concurrent check of the same proxy fails fast and sanitized.
    let _permit = match state.try_proxy_check_permit(&id).await {
        ProxyCheckPermitOutcome::Acquired(permit) => permit,
        ProxyCheckPermitOutcome::Busy => {
            return Err(fail(
                StatusCode::CONFLICT,
                "a live check of this proxy is already running; try again once it finishes",
            ));
        }
        ProxyCheckPermitOutcome::AtCapacity => {
            return Err(fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "too many proxy live-checks are running; try again shortly",
            ));
        }
    };

    let checked_at = check_timestamp()?;
    let checked = check_endpoints(
        checkable,
        DEFAULT_CHECK_TARGET,
        CHECK_CONNECT_TIMEOUT,
        CHECK_TOTAL_TIMEOUT,
    )
    .await;

    // Merge checked endpoints back into expansion order: `prepared` holds
    // pre-computed die results for endpoints that could not be prepared.
    let mut results = Vec::with_capacity(prepared.len());
    let mut checked = checked.into_iter();
    for slot in prepared.drain(..) {
        let Some(result) = slot.or_else(|| checked.next()) else {
            return Err(internal_message(
                "proxy check produced fewer results than endpoints",
            ));
        };
        results.push(result);
    }

    let live_count = results.iter().filter(|result| result.live).count();
    let die_count = results.len() - live_count;
    state
        .record_proxy_check(
            &id,
            checked_at,
            results
                .iter()
                .map(|result| ProxyEndpointCheckOutcome {
                    endpoint_id: result.endpoint_id.clone(),
                    live: result.live,
                    latency_millis: result.latency_millis,
                    error: result.error.clone(),
                })
                .collect(),
        )
        .await;

    Ok(Json(json!({
        "proxy_id": id,
        "checked_at": checked_at,
        "results": results
            .iter()
            .map(|result| {
                json!({
                    "endpoint_id": result.endpoint_id,
                    "label": result.label,
                    "url": result.url,
                    "status": if result.live { "live" } else { "die" },
                    "latency_ms": result.latency_millis,
                    "error": result.error,
                })
            })
            .collect::<Vec<Value>>(),
        "live_count": live_count,
        "die_count": die_count,
    })))
}

/// Expands stored endpoint records into checkable endpoints plus pre-computed
/// die results, in expansion order, for endpoints that cannot be prepared.
/// Corrupt records become die results instead of silent skips.
fn prepare_endpoints(
    group: &ProxyEndpointGroup,
    master_key: Option<&[u8; 32]>,
) -> (Vec<CheckableEndpoint>, Vec<Option<EndpointCheckResult>>) {
    let mut checkable = Vec::with_capacity(group.endpoints.len());
    let mut prepared = Vec::with_capacity(group.endpoints.len());
    for config in &group.endpoints {
        let endpoint_id = config.endpoint_id.clone();
        let label = config
            .record
            .optional_text("label")
            .ok()
            .flatten()
            .map(str::to_owned);
        let url = config
            .record
            .optional_text("url")
            .ok()
            .flatten()
            .unwrap_or_default()
            .to_owned();
        let username = config
            .record
            .optional_text("username")
            .ok()
            .flatten()
            .map(str::to_owned);
        let secret = match config.record.optional_bytes("secret") {
            Ok(None) => Ok(None),
            Ok(Some(stored)) => decrypt_secret(master_key, Some(stored)),
            Err(_) => Err("stored proxy endpoint credential is invalid".to_owned()),
        };
        let die_now = |error: String| EndpointCheckResult {
            endpoint_id: endpoint_id.clone(),
            label: label.clone(),
            url: url.clone(),
            live: false,
            latency_millis: 0,
            error: Some(error),
        };
        if url.is_empty() {
            prepared.push(Some(die_now("proxy endpoint URL is invalid".to_owned())));
            continue;
        }
        let secret = match secret {
            Ok(secret) => secret,
            Err(message) => {
                prepared.push(Some(die_now(message)));
                continue;
            }
        };
        checkable.push(CheckableEndpoint {
            endpoint_id: endpoint_id.clone(),
            label: label.clone(),
            url: url.clone(),
            username: username.clone(),
            secret,
        });
        prepared.push(None);
    }
    (checkable, prepared)
}

/// Checks each endpoint once, preserving input order with bounded concurrency.
pub(super) async fn check_endpoints(
    endpoints: Vec<CheckableEndpoint>,
    target: &str,
    connect_timeout: Duration,
    total_timeout: Duration,
) -> Vec<EndpointCheckResult> {
    futures_util::stream::iter(endpoints.into_iter().map(|endpoint| async move {
        check_endpoint(endpoint, target, connect_timeout, total_timeout).await
    }))
    .buffered(CHECK_MAX_CONCURRENCY)
    .collect()
    .await
}

async fn check_endpoint(
    endpoint: CheckableEndpoint,
    target: &str,
    connect_timeout: Duration,
    total_timeout: Duration,
) -> EndpointCheckResult {
    let started = Instant::now();
    let outcome = probe_endpoint(&endpoint, target, connect_timeout, total_timeout).await;
    let latency_millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    EndpointCheckResult {
        endpoint_id: endpoint.endpoint_id,
        label: endpoint.label,
        url: endpoint.url,
        live: outcome.is_ok(),
        latency_millis,
        error: outcome.err(),
    }
}

/// One probe: any HTTP response through the proxy counts as live. Transport
/// failures map to short classified messages that cannot carry the proxy URL,
/// credentials, or a response body.
async fn probe_endpoint(
    endpoint: &CheckableEndpoint,
    target: &str,
    connect_timeout: Duration,
    total_timeout: Duration,
) -> Result<(), String> {
    let proxy_url = reqwest::Url::parse(&endpoint.url)
        .map_err(|_| "proxy endpoint URL is invalid".to_owned())?;
    if proxy_url.scheme() != "http" && proxy_url.scheme() != "https" {
        return Err("proxy endpoint URL must use http:// or https://".to_owned());
    }
    let mut proxy = reqwest::Proxy::all(proxy_url)
        .map_err(|_| "proxy endpoint URL is not usable as a proxy".to_owned())?;
    if let (Some(username), Some(secret)) =
        (endpoint.username.as_deref(), endpoint.secret.as_deref())
    {
        proxy = proxy.basic_auth(username, secret);
    }
    let client = reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(total_timeout)
        .redirect(reqwest::redirect::Policy::none())
        .proxy(proxy)
        .build()
        .map_err(|_| "could not build the proxy check client".to_owned())?;
    let response = client
        .head(target)
        .send()
        .await
        .map_err(|error| classify_check_error(&error))?;
    // Any response status means the endpoint accepted the connection and
    // answered; the body of a HEAD probe is empty and is dropped unread.
    drop(response);
    Ok(())
}

/// Classifies a transport failure without propagating reqwest's error text,
/// which embeds URLs and is not sanitized for display.
fn classify_check_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "the check did not complete within its deadline".to_owned()
    } else if error.is_connect() {
        "could not connect through the proxy endpoint".to_owned()
    } else {
        "the check request failed".to_owned()
    }
}

fn check_timestamp() -> Result<u64, (StatusCode, Json<Value>)> {
    let seconds = super::super::sessions::db_now_unix_seconds().map_err(internal_message)?;
    u64::try_from(seconds).map_err(|_| {
        fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the system clock is out of range",
        )
    })
}
