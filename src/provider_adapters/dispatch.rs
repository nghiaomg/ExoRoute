use super::{
    AdapterRequestContext, AdapterRequestError, AdapterRequestPreparation, AppState,
    UpstreamAuthContext, adapter,
};
use http::{HeaderValue, StatusCode};
use serde_json::Value;
use std::collections::BTreeMap;

pub fn prepare_request_body(
    adapter_id: &str,
    body: &mut Value,
    request_id: &str,
) -> Result<(), String> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered".to_owned());
    };
    adapter.prepare_body(body, request_id);
    Ok(())
}

pub async fn prepare_provider_images(
    adapter_id: &str,
    state: &AppState,
    body: &mut Value,
) -> Result<(), String> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered".to_owned());
    };
    adapter.prepare_images(state, body).await
}

pub async fn prepare_upstream_request<'a>(
    adapter_id: &str,
    context: AdapterRequestContext<'a>,
    body: &'a mut Value,
) -> Result<AdapterRequestPreparation, AdapterRequestError> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err(AdapterRequestError::new(
            None,
            None,
            "provider adapter is not registered",
        ));
    };
    adapter.prepare_upstream_request(context, body).await
}

pub fn apply_custom_headers(
    mut request: reqwest::RequestBuilder,
    headers: &BTreeMap<String, String>,
) -> Result<reqwest::RequestBuilder, String> {
    for (name, value) in headers {
        let name = http::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| "stored provider custom header name is invalid".to_owned())?;
        let value = HeaderValue::from_str(value)
            .map_err(|_| "stored provider custom header value is invalid".to_owned())?;
        request = request.header(name, value);
    }
    Ok(request)
}

pub fn retry_upstream_response_as_key_rejection(adapter_id: &str) -> bool {
    adapter(adapter_id)
        .map(|adapter| adapter.retry_upstream_response_as_key_rejection())
        .unwrap_or(true)
}

pub async fn finish_upstream_request<'a>(
    adapter_id: &str,
    state: &'a AppState,
    preparation: AdapterRequestPreparation,
    auth: UpstreamAuthContext<'a>,
    status: Option<StatusCode>,
) {
    if let Some(adapter) = adapter(adapter_id) {
        adapter
            .finish_upstream_request(state, preparation, auth, status)
            .await;
    }
}
