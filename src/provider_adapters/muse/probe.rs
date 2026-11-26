//! Muse connected-account model probe.
//!
//! Sends the same Responses request the gateway would send, so a model test
//! measures the connected account rather than a public catalog.

use super::auth::muse as muse_oauth;
use super::*;
use crate::security::egress;
use std::time::Duration;

const PROBE_CONNECT_TIMEOUT_CAP: Duration = Duration::from_secs(3);
const PROBE_REQUEST_TIMEOUT_CAP: Duration = Duration::from_secs(10);
/// The shared probe body asks for a single output token. Muse Spark spends its
/// whole budget on hidden reasoning before emitting anything, so a one-token
/// probe would be reported as a successful model test without the model ever
/// producing an answer. Asking for a real answer keeps the test meaningful.
const PROBE_MAX_OUTPUT_TOKENS: u64 = 512;

pub(super) async fn test_muse_oauth_model(
    adapter: &MuseAdapter,
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
    let loaded = match muse_oauth::load_muse_account(state, credential_id).await {
        Ok(loaded) => loaded,
        Err(error) => return failed(None, error),
    };
    let account = match loaded.needs_remint {
        true => match remint_and_store(state, credential_id, &loaded.account).await {
            Ok(account) => account,
            Err(error) => return failed(None, error),
        },
        false => loaded.account,
    };
    let Some(api_key) = account.inference_key() else {
        return failed(None, "Muse account has no inference key".to_owned());
    };
    let auth = OAuthRequestAuth {
        access_token: api_key.to_owned(),
        account_id: None,
        project_id: None,
    };
    // Muse Spark reasoning only round-trips on Responses, so the probe uses the
    // same default protocol the preset installs.
    let endpoint = match adapter.endpoint(base_url, Protocol::Responses, None) {
        Ok(endpoint) => endpoint,
        Err(error) => return failed(None, error),
    };
    let (endpoint, client) = match egress::provider_client(
        endpoint.as_str(),
        state.config.allow_private_provider_urls,
        state.config.connect_timeout.min(PROBE_CONNECT_TIMEOUT_CAP),
        state.config.request_timeout.min(PROBE_REQUEST_TIMEOUT_CAP),
        false,
        INFERENCE_USER_AGENT,
        state.operational_settings().settings.upstream,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => return failed(None, format!("provider egress check failed: {error}")),
    };
    let request_id = uuid::Uuid::new_v4().to_string();
    let mut body = model_probe_body(model, Protocol::Responses);
    body["max_output_tokens"] = serde_json::json!(PROBE_MAX_OUTPUT_TOKENS);
    adapter.prepare_body(&mut body, &request_id);
    let request = client
        .post(endpoint)
        .header("Accept", "application/json")
        .header("x-request-id", &request_id)
        .json(&body);
    let request = adapter.apply_client_headers(request, &HeaderMap::new());
    let request = match adapter.apply_request_auth(
        request,
        MUSE_ADAPTER_ID,
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
        let provider_response_body =
            match read_limited_response(response, MAX_MODEL_TEST_RESPONSE_BYTES).await {
                Ok(body) => model_test_provider_response_body(&body),
                Err(_) => None,
            };
        return AdapterModelTestOutcome {
            test_passed: false,
            status: Some(status.as_u16()),
            message: format!("Provider returned HTTP {}", status.as_u16()),
            provider_response_body,
        };
    }
    let body = match read_limited_response(response, MAX_MODEL_TEST_RESPONSE_BYTES).await {
        Ok(body) => body,
        Err(error) => {
            return failed(
                Some(http::StatusCode::BAD_GATEWAY.as_u16()),
                format!("Provider test response could not be read: {error}"),
            );
        }
    };
    if serde_json::from_slice::<Value>(&body).is_err() {
        return failed(
            Some(http::StatusCode::BAD_GATEWAY.as_u16()),
            "Provider returned a non-JSON inference response".to_owned(),
        );
    }
    AdapterModelTestOutcome {
        test_passed: true,
        status: Some(status.as_u16()),
        message: "Model responded successfully".to_owned(),
        provider_response_body: None,
    }
}
