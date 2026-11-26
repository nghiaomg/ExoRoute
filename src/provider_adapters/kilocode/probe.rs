//! Kilo Code OAuth model probe.
//!
//! Sends the same Chat Completions request the gateway would send, so a model
//! test measures the connected account rather than a public catalog.

use super::auth::kilocode as kilocode_oauth;
use super::*;
use crate::security::egress;
use std::time::Duration;

const PROBE_CONNECT_TIMEOUT_CAP: Duration = Duration::from_secs(3);
const PROBE_REQUEST_TIMEOUT_CAP: Duration = Duration::from_secs(10);

pub(super) async fn test_kilocode_oauth_model(
    adapter: &KilocodeAdapter,
    state: &AppState,
    base_url: &str,
    model: &str,
    credential_id: &str,
) -> AdapterModelTestOutcome {
    let failed = |status, message: String| AdapterModelTestOutcome {
        test_passed: false,
        status,
        message,
        provider_response_body: None,
    };
    let auth = match kilocode_oauth::account_for_use(state, credential_id).await {
        Ok(account) => OAuthRequestAuth {
            access_token: account.access_token,
            account_id: None,
            project_id: None,
        },
        Err(error) => return failed(None, error),
    };
    let endpoint = match adapter.endpoint(base_url, Protocol::ChatCompletions, None) {
        Ok(endpoint) => endpoint,
        Err(error) => return failed(None, error),
    };
    let (endpoint, client) = match egress::provider_client(
        endpoint.as_str(),
        state.config.allow_private_provider_urls,
        state.config.connect_timeout.min(PROBE_CONNECT_TIMEOUT_CAP),
        state.config.request_timeout.min(PROBE_REQUEST_TIMEOUT_CAP),
        false,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        state.operational_settings().settings.upstream,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => return failed(None, format!("provider egress check failed: {error}")),
    };
    let request_id = uuid::Uuid::new_v4().to_string();
    let mut body = super::model_probe_body(model, Protocol::ChatCompletions);
    adapter.prepare_body(&mut body, &request_id);
    let request = client
        .post(endpoint)
        .header("Accept", "application/json")
        .header("x-request-id", &request_id)
        .json(&body);
    let request = adapter.apply_client_headers(request, &HeaderMap::new());
    let request = match adapter.apply_request_auth(
        request,
        KILOCODE_ADAPTER_ID,
        None,
        None,
        Some(&auth),
        &request_id,
    ) {
        Ok(request) => request,
        Err(error) => return failed(None, error),
    };
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            return failed(
                None,
                format!("Could not reach provider: {}", error.without_url()),
            );
        }
    };
    let status = response.status();
    if !status.is_success() {
        let provider_response_body = match super::read_limited_response(
            response,
            super::MAX_MODEL_TEST_RESPONSE_BYTES,
        )
        .await
        {
            Ok(body) => super::model_test_provider_response_body(&body),
            Err(_) => None,
        };
        return AdapterModelTestOutcome {
            test_passed: false,
            status: Some(status.as_u16()),
            message: format!("Provider returned HTTP {}", status.as_u16()),
            provider_response_body,
        };
    }
    let body =
        match super::read_limited_response(response, super::MAX_MODEL_TEST_RESPONSE_BYTES).await {
            Ok(body) => body,
            Err(error) => {
                return failed(
                    Some(http::StatusCode::BAD_GATEWAY.as_u16()),
                    format!("Provider test response could not be read: {error}"),
                );
            }
        };
    let value = match serde_json::from_slice::<Value>(&body) {
        Ok(value) => value,
        Err(_) => {
            return failed(
                Some(http::StatusCode::BAD_GATEWAY.as_u16()),
                "Provider returned a non-JSON inference response".to_owned(),
            );
        }
    };
    if let Err(error) = adapter.normalize_response(value) {
        return failed(Some(http::StatusCode::BAD_GATEWAY.as_u16()), error);
    }
    AdapterModelTestOutcome {
        test_passed: true,
        status: Some(status.as_u16()),
        message: "Model responded successfully".to_owned(),
        provider_response_body: None,
    }
}
