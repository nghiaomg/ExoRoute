use super::auth::cline::{self as cline_oauth};
use super::*;
use crate::security::egress;
use serde_json::Value;
use std::time::Duration;
pub(super) async fn test_cline_oauth_model(
    adapter: &ClineAdapter,
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
    let auth = match cline_oauth::account_for_use(state, credential_id).await {
        Ok(account) => OAuthRequestAuth {
            access_token: account.access_token,
            account_id: account.account_id,
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
        false,
        state.config.connect_timeout.min(Duration::from_secs(3)),
        state.config.request_timeout.min(Duration::from_secs(10)),
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
    let client_headers = http::HeaderMap::new();
    let request = adapter.apply_client_headers(request, &client_headers);
    let request =
        match adapter.apply_request_auth(request, "bearer", None, None, Some(&auth), &request_id) {
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
