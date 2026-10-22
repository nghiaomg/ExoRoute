//! Qoder OAuth model probe.
//!
//! Verifies that the connected Qoder account's credentials are valid by querying
//! Qoder's OpenAPI userinfo endpoint and validating the model identifier against
//! the supported Qoder catalog.

use super::auth::qoder as qoder_oauth;
use super::catalog::is_supported_qoder_model;
use super::*;
use crate::security::egress;
use std::time::Duration;

const PROBE_CONNECT_TIMEOUT_CAP: Duration = Duration::from_secs(3);
const PROBE_REQUEST_TIMEOUT_CAP: Duration = Duration::from_secs(10);

pub(super) async fn test_qoder_oauth_model(
    adapter: &dyn ProviderAdapter,
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

    let auth = match qoder_oauth::account_for_use(state, credential_id).await {
        Ok(account) => account,
        Err(error) => return failed(None, error),
    };

    let is_direct_upstream = base_url.contains("qoder.sh")
        || base_url.contains("qoder.com.cn")
        || base_url.contains("qoder.com");

    if is_direct_upstream && !is_supported_qoder_model(adapter.adapter_id(), model) {
        return AdapterModelTestOutcome {
            test_passed: false,
            status: Some(StatusCode::NOT_FOUND.as_u16()),
            message: format!("Model '{model}' is not supported by Qoder"),
            provider_response_body: None,
        };
    }

    let operational = state.operational_settings().settings;

    // Custom OpenAI-compatible proxy (e.g. local proxy or mock server)
    if !is_direct_upstream
        && let Ok(endpoint) = adapter.endpoint(base_url, Protocol::ChatCompletions, None)
        && let Ok((endpoint, client)) = egress::provider_client(
            endpoint.as_str(),
            state.config.allow_private_provider_urls,
            operational.connect_timeout.min(PROBE_CONNECT_TIMEOUT_CAP),
            operational.request_timeout.min(PROBE_REQUEST_TIMEOUT_CAP),
            false,
            concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
            operational.upstream,
        )
        .await
    {
        let request_id = uuid::Uuid::new_v4().to_string();
        let mut body = super::model_probe_body(model, Protocol::ChatCompletions);
        adapter.prepare_body(&mut body, &request_id);

        let request = client
            .post(endpoint)
            .header("Accept", "application/json")
            .header("x-request-id", &request_id)
            .bearer_auth(&auth.access_token)
            .json(&body);

        let request = adapter.apply_client_headers(request, &HeaderMap::new());
        if let Ok(response) = request.send().await {
            let status = response.status();
            if status.is_success() {
                return AdapterModelTestOutcome {
                    test_passed: true,
                    status: Some(status.as_u16()),
                    message: "Model test passed".to_owned(),
                    provider_response_body: None,
                };
            }
            if status != StatusCode::NOT_FOUND {
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
        }
    }

    // Direct Qoder upstream or fallback: query Qoder's OpenAPI userinfo endpoint
    let userinfo_url = if base_url.contains("127.0.0.1") || base_url.contains("localhost") {
        if let Ok(mut url) = reqwest::Url::parse(base_url) {
            url.set_path("/api/v1/userinfo");
            url.set_query(None);
            url.to_string()
        } else {
            "https://openapi.qoder.sh/api/v1/userinfo".to_owned()
        }
    } else if adapter.adapter_id() == QODER_CN_ADAPTER_ID || base_url.contains("qoder.com.cn") {
        "https://openapi.qoder.com.cn/api/v1/userinfo".to_owned()
    } else {
        "https://openapi.qoder.sh/api/v1/userinfo".to_owned()
    };

    let (endpoint, client) = match egress::provider_client(
        &userinfo_url,
        state.config.allow_private_provider_urls,
        operational.connect_timeout.min(PROBE_CONNECT_TIMEOUT_CAP),
        operational.request_timeout.min(PROBE_REQUEST_TIMEOUT_CAP),
        false,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        operational.upstream,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => return failed(None, format!("provider egress check failed: {error}")),
    };

    let request = client
        .get(endpoint)
        .header(http::header::ACCEPT, "application/json")
        .header(http::header::USER_AGENT, "Go-http-client/2.0")
        .header(QODER_IDE_VERSION_HEADER, QODER_IDE_VERSION)
        .header(QODER_CLIENT_TYPE_HEADER, QODER_CLIENT_TYPE)
        .header(QODER_LOGIN_VERSION_HEADER, QODER_LOGIN_VERSION)
        .bearer_auth(&auth.access_token);

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
    if status.is_success() {
        return AdapterModelTestOutcome {
            test_passed: true,
            status: Some(StatusCode::OK.as_u16()),
            message: "Model test passed".to_owned(),
            provider_response_body: None,
        };
    }

    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return AdapterModelTestOutcome {
            test_passed: false,
            status: Some(status.as_u16()),
            message: "Qoder access token is invalid or expired".to_owned(),
            provider_response_body: None,
        };
    }

    let provider_response_body =
        match super::read_limited_response(response, super::MAX_MODEL_TEST_RESPONSE_BYTES).await {
            Ok(body) => super::model_test_provider_response_body(&body),
            Err(_) => None,
        };

    AdapterModelTestOutcome {
        test_passed: false,
        status: Some(status.as_u16()),
        message: format!("Provider returned HTTP {}", status.as_u16()),
        provider_response_body,
    }
}
