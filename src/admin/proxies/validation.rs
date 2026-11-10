//! Input validation for proxies and their endpoints.
//!
//! Endpoint URLs arrive from operators in several shapes, so normalization
//! happens here rather than at the storage or egress layer: `host:port` gains an
//! `http://` scheme, `http://`/`https://` keep theirs, and anything else
//! (including the SOCKS schemes reqwest only supports behind a build feature) is
//! rejected with a message that names the accepted forms.

use super::*;
use crate::config::{MAX_PROXY_ENDPOINTS, MAX_PROXY_NESTING_DEPTH};

pub(super) const MAX_PROXY_ID_BYTES: usize = 128;
pub(super) const MAX_PROXY_NAME_BYTES: usize = 256;
pub(super) const MAX_PROXY_ENDPOINT_URL_BYTES: usize = 512;
pub(super) const MAX_PROXY_USERNAME_BYTES: usize = 256;
pub(super) const MAX_PROXY_SECRET_BYTES: usize = 512;
pub(super) const MAX_PROXY_LABEL_BYTES: usize = 64;
pub(super) const MAX_PROXY_PRIORITY: u32 = MAX_PROXY_ENDPOINTS as u32;

/// Rejects a body id that disagrees with the path id, then adopts the path id.
pub(super) fn apply_path_id_for_update(
    path_id: &str,
    input: &mut ProxyInput,
) -> Result<(), &'static str> {
    if !input.id.is_empty() && path_id != input.id {
        return Err("path id must match proxy id");
    }
    if path_id.trim() != path_id || path_id.is_empty() {
        return Err("proxy id is not canonical");
    }
    input.id = path_id.to_owned();
    Ok(())
}

/// Validates the accepted proxy shapes and the invariants that span endpoints.
pub(super) fn validate_proxy(input: &mut ProxyInput) -> Result<(), &'static str> {
    if input.name.trim().is_empty() || input.name.len() > MAX_PROXY_NAME_BYTES {
        return Err("proxy name is required and must not exceed 256 bytes");
    }
    if input
        .name
        .chars()
        .any(|character| character.is_ascii_control())
    {
        return Err("proxy name contains unsupported control characters");
    }
    if input.id.len() > MAX_PROXY_ID_BYTES {
        return Err("proxy ID must not exceed 128 bytes");
    }
    if input
        .id
        .chars()
        .any(|character| character.is_ascii_control())
    {
        return Err("proxy ID contains unsupported control characters");
    }
    if input.strategy != "priority" && input.strategy != "round_robin" {
        return Err("proxy strategy must be priority or round_robin");
    }
    if input.endpoints.is_empty() {
        return Err("proxy must have at least one endpoint");
    }
    // An embedded proxy counts as one endpoint that may expand into its own
    // endpoints, so the row bound is the honest limit here; the reference walk
    // enforces the expanded bound.
    if input.endpoints.len() > MAX_PROXY_ENDPOINTS {
        return Err("proxy has more endpoints than the supported limit");
    }
    if !input.endpoints.iter().any(|endpoint| endpoint.enabled) && input.enabled {
        return Err("enabled proxy must have at least one enabled endpoint");
    }
    let mut endpoint_ids = std::collections::HashSet::with_capacity(input.endpoints.len());
    for endpoint in input.endpoints.iter_mut() {
        validate_endpoint(endpoint)?;
        if !endpoint_ids.insert(endpoint.endpoint_id.as_str()) {
            return Err("proxy endpoint IDs must be unique within a proxy");
        }
    }
    Ok(())
}

fn validate_endpoint(endpoint: &mut ProxyEndpointInput) -> Result<(), &'static str> {
    if endpoint.endpoint_id.is_empty() {
        endpoint.endpoint_id = uuid::Uuid::new_v4().to_string();
    }
    if endpoint.endpoint_id.len() > 128
        || endpoint
            .endpoint_id
            .bytes()
            .any(|byte| byte.is_ascii_control())
    {
        return Err("proxy endpoint ID is invalid");
    }
    if endpoint.priority > MAX_PROXY_PRIORITY {
        return Err("proxy endpoint priority is out of range");
    }
    let reference = endpoint
        .ref_proxy_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let server_url = endpoint
        .url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if reference.is_some() && server_url.is_some() {
        return Err("an endpoint is either a proxy server URL or an embedded proxy, not both");
    }
    if let Some(reference) = reference {
        if reference.len() > MAX_PROXY_ID_BYTES {
            return Err("embedded proxy ID must not exceed 128 bytes");
        }
        if reference
            .chars()
            .any(|character| character.is_ascii_control())
        {
            return Err("embedded proxy ID contains unsupported control characters");
        }
        // A reference carries no server fields: credentials on it would be
        // silently dropped at request time.
        if endpoint.credential_is_set() {
            return Err("an embedded proxy cannot carry credentials of its own");
        }
        endpoint.ref_proxy_id = Some(reference.to_owned());
        endpoint.url = None;
        endpoint.secret_ciphertext = None;
        return Ok(());
    }
    if server_url.is_none() {
        return Err("each endpoint needs a proxy URL or an embedded proxy ID");
    }
    validate_endpoint_fields(endpoint)
}

fn validate_endpoint_fields(endpoint: &mut ProxyEndpointInput) -> Result<(), &'static str> {
    let normalized = normalize_proxy_url(endpoint.url.as_deref().unwrap_or_default())?;
    if normalized.len() > MAX_PROXY_ENDPOINT_URL_BYTES {
        return Err("proxy URL must not exceed 512 bytes");
    }
    if let Some(username) = endpoint.username.as_deref() {
        if username.len() > MAX_PROXY_USERNAME_BYTES {
            return Err("proxy username must not exceed 256 bytes");
        }
        if username
            .chars()
            .any(|character| character.is_ascii_control())
        {
            return Err("proxy username contains unsupported control characters");
        }
    }
    if let Some(secret) = endpoint.secret.as_deref() {
        if secret.len() > MAX_PROXY_SECRET_BYTES {
            return Err("proxy password must not exceed 512 bytes");
        }
    }
    if let Some(label) = endpoint.label.as_deref() {
        if label.len() > MAX_PROXY_LABEL_BYTES {
            return Err("proxy endpoint label must not exceed 64 bytes");
        }
        if label.chars().any(|character| character.is_ascii_control()) {
            return Err("proxy endpoint label contains unsupported control characters");
        }
    }
    endpoint.url = Some(normalized);
    endpoint.secret_ciphertext = None;
    Ok(())
}

/// Accepts `host:port`, `http://host:port`, and `https://host:port`.
///
/// `host:port` without a scheme is the form operators paste from proxy lists and
/// means an HTTP proxy, matching how curl and reqwest read a bare `host:port`.
/// Every other scheme is rejected explicitly so a SOCKS URL fails at save time
/// with a clear message instead of at connect time with a transport error.
pub(crate) fn normalize_proxy_url(value: &str) -> Result<String, &'static str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("proxy URL is required");
    }
    if trimmed
        .chars()
        .any(|character| character.is_ascii_control())
    {
        return Err("proxy URL contains unsupported control characters");
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        // A bare `host:port` is unambiguous once it has a colon; anything else
        // is rejected rather than guessed at.
        if !trimmed.contains(':') {
            return Err("proxy URL must include a port, for example 10.0.0.1:8080");
        }
        format!("http://{trimmed}")
    };
    let scheme = with_scheme
        .split_once("://")
        .map(|(scheme, _)| scheme.to_ascii_lowercase())
        .unwrap_or_default();
    if scheme != "http" && scheme != "https" {
        return Err("proxy URL must use http:// or https://; SOCKS proxies are not supported");
    }
    let parsed = reqwest::Url::parse(&with_scheme)
        .map_err(|_| "proxy URL is not a valid URL, for example 10.0.0.1:8080")?;
    if parsed.host_str().is_none() {
        return Err("proxy URL must include a host");
    }
    if parsed.port_or_known_default().is_none() {
        return Err("proxy URL must include a port, for example 10.0.0.1:8080");
    }
    // Credentials belong in the dedicated fields, where they are encrypted; a
    // userinfo component in stored text would leak the password into logs.
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("put proxy credentials in the username and password fields, not in the URL");
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("proxy URL must not include a query or fragment");
    }
    let path = parsed.path();
    if !(path.is_empty() || path == "/") {
        return Err("proxy URL must not include a path");
    }
    Ok(parsed.to_string())
}

/// Rejects an expanded reference graph that would nest deeper than the bound.
pub(super) fn validate_nesting_depth(depth: usize) -> Result<(), &'static str> {
    if depth > MAX_PROXY_NESTING_DEPTH {
        return Err("embedded proxies are nested deeper than the supported limit");
    }
    Ok(())
}
