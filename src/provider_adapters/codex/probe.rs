use super::*;
use crate::security::egress;
pub(crate) async fn test_oauth_model_impl(
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
    let auth = match adapter
        .resolve_oauth_request_auth(state, credential_id)
        .await
    {
        Ok(Some(auth)) => auth,
        Ok(None) => return failed(None, "provider OAuth account is unavailable".to_owned()),
        Err(error) => return failed(None, error),
    };
    let probe_protocol = Protocol::Responses;
    let endpoint = match adapter.endpoint(base_url, probe_protocol, None) {
        Ok(endpoint) => endpoint,
        Err(error) => return failed(None, error),
    };
    let (endpoint, client) = match egress::provider_client(
        endpoint.as_str(),
        false,
        state
            .config
            .connect_timeout
            .min(std::time::Duration::from_secs(3)),
        state
            .config
            .request_timeout
            .min(std::time::Duration::from_secs(10)),
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        state.operational_settings().settings.upstream,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => return failed(None, format!("provider egress check failed: {error}")),
    };
    let session_id = uuid::Uuid::new_v4().to_string();
    let mut body = model_probe_body(model, probe_protocol);
    adapter.prepare_body(&mut body, &session_id);
    let request = client
        .post(endpoint)
        .header("Accept", "text/event-stream")
        .json(&body);
    let request = match adapter.apply_request_auth(
        request,
        default_auth_type(adapter.adapter_id()).unwrap_or("none"),
        None,
        None,
        Some(&auth),
        &session_id,
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
            message: format!("provider returned HTTP {}", status.as_u16()),
            provider_response_body,
        };
    }
    let missing_content_type = response.headers().get(http::header::CONTENT_TYPE).is_none();
    if !super::sse::accepts_event_stream_response(
        &response,
        adapter.allows_missing_event_stream_content_type(),
    ) {
        let content_type = response
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = match read_limited_response(response, MAX_MODEL_TEST_RESPONSE_BYTES).await {
            Ok(body) => body,
            Err(error) => {
                let mut outcome = model_test_non_sse_response(status, content_type.as_deref(), &[]);
                outcome.message.push_str(&format!(
                    "; provider response body could not be read: {error}"
                ));
                return outcome;
            }
        };
        return model_test_non_sse_response(status, content_type.as_deref(), &body);
    }
    const MAX_TEST_RESPONSE_BYTES: usize = 256 * 1024;
    let body = if missing_content_type {
        match read_limited_response(response, MAX_TEST_RESPONSE_BYTES).await {
            Ok(body) => body,
            Err(error) => return failed(Some(http::StatusCode::BAD_GATEWAY.as_u16()), error),
        }
    } else {
        return match adapter
            .read_event_stream(response, MAX_TEST_RESPONSE_BYTES)
            .await
        {
            Ok(_) => AdapterModelTestOutcome {
                test_passed: true,
                status: Some(status.as_u16()),
                message: "Model responded successfully".to_owned(),
                provider_response_body: None,
            },
            Err(error) => AdapterModelTestOutcome {
                test_passed: false,
                status: Some(http::StatusCode::BAD_GATEWAY.as_u16()),
                message: error.message,
                provider_response_body: None,
            },
        };
    };
    match parse_adapter_event_stream(&body) {
        Ok(_) => AdapterModelTestOutcome {
            test_passed: true,
            status: Some(status.as_u16()),
            message: "Model responded successfully".to_owned(),
            provider_response_body: None,
        },
        Err(error) => AdapterModelTestOutcome {
            test_passed: false,
            status: Some(http::StatusCode::BAD_GATEWAY.as_u16()),
            message: error.message,
            provider_response_body: model_test_provider_response_body(&body),
        },
    }
}
