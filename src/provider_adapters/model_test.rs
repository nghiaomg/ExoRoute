use super::*;

pub(super) const MAX_MODEL_TEST_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_MODEL_TEST_PROVIDER_RESPONSE_CHARS: usize = 8 * 1024;

pub(super) fn model_test_provider_response_body(body: &[u8]) -> Option<String> {
    let detail = crate::gateway::sanitize_provider_error_body(body);
    if detail.trim().is_empty() {
        return None;
    }
    let mut truncated = detail
        .chars()
        .take(MAX_MODEL_TEST_PROVIDER_RESPONSE_CHARS)
        .collect::<String>();
    if detail.chars().count() > MAX_MODEL_TEST_PROVIDER_RESPONSE_CHARS {
        truncated.push('…');
    }
    Some(truncated)
}

pub(super) fn model_test_non_sse_response(
    provider_status: http::StatusCode,
    content_type: Option<&str>,
    body: &[u8],
) -> super::AdapterModelTestOutcome {
    let content_type = content_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(128).collect::<String>())
        .unwrap_or_else(|| "missing".to_owned());
    super::AdapterModelTestOutcome {
        test_passed: false,
        status: Some(http::StatusCode::BAD_GATEWAY.as_u16()),
        message: format!(
            "Provider returned HTTP {} with Content-Type {content_type}; expected text/event-stream",
            provider_status.as_u16()
        ),
        provider_response_body: model_test_provider_response_body(body),
    }
}

pub(crate) fn model_probe_body(model: &str, protocol: Protocol) -> Value {
    match protocol {
        Protocol::ChatCompletions => json!({
            "model": model,
            "messages": [{"role":"user","content":"Reply with OK."}],
            "max_tokens": 1,
            "stream": false,
        }),
        Protocol::Responses => json!({
            "model": model,
            "input": [{
                "role": "user",
                "content": [{"type": "input_text", "text": "Reply with OK."}],
            }],
            "max_output_tokens": 1,
            "stream": false,
        }),
        Protocol::Messages => json!({
            "model": model,
            "messages": [{"role":"user","content":"Reply with OK."}],
            "max_tokens": 1,
            "stream": false,
        }),
    }
}

pub(super) async fn test_api_key_credential_impl<A: ProviderAdapter + ?Sized>(
    adapter: &A,
    options: AdapterApiKeyRequest<'_>,
) -> AdapterKeyTestOutcome {
    let failed = |status, message: String| AdapterKeyTestOutcome {
        test_passed: false,
        status,
        message,
    };
    let endpoint = match adapter.model_list_endpoint(options.base_url) {
        Ok(endpoint) => endpoint,
        Err(error) => return failed(None, error),
    };
    let (endpoint, client) = match egress::provider_client(
        endpoint.as_str(),
        options.state.config.allow_private_provider_urls,
        options
            .state
            .config
            .connect_timeout
            .min(std::time::Duration::from_secs(3)),
        options
            .state
            .config
            .request_timeout
            .min(std::time::Duration::from_secs(10)),
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        options.state.operational_settings().settings.upstream,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => return failed(None, format!("Provider egress check failed: {error}")),
    };
    let request = match crate::provider_adapters::apply_custom_headers(
        client.get(endpoint),
        options.custom_headers,
    ) {
        Ok(request) => request,
        Err(error) => return failed(None, error),
    };
    let request = if options.preferred_protocol == "messages" {
        request.header("anthropic-version", "2023-06-01")
    } else {
        request
    };
    let request = match adapter.apply_request_auth(
        request,
        options.auth_type,
        options.auth_header,
        Some(options.credential),
        None,
        "credential-test",
    ) {
        Ok(request) => request,
        Err(error) => return failed(None, error),
    };
    match request.send().await {
        Ok(response) if response.status().is_success() => AdapterKeyTestOutcome {
            test_passed: true,
            status: Some(response.status().as_u16()),
            message: "API key test passed".to_owned(),
        },
        Ok(response) => failed(
            Some(response.status().as_u16()),
            format!(
                "Provider returned HTTP {} while testing this key",
                response.status().as_u16()
            ),
        ),
        Err(error) => failed(
            None,
            format!("Could not reach provider: {}", error.without_url()),
        ),
    }
}

pub(super) async fn test_api_key_model_impl<A: ProviderAdapter + ?Sized>(
    adapter: &A,
    request: AdapterModelTestRequest<'_>,
) -> AdapterModelTestOutcome {
    let failed = |status, message: String| AdapterModelTestOutcome {
        test_passed: false,
        status,
        message,
        provider_response_body: None,
    };
    let endpoint = match adapter.endpoint(request.base_url, request.protocol, None) {
        Ok(endpoint) => endpoint,
        Err(error) => return failed(None, error),
    };
    let (endpoint, client) = match egress::provider_client(
        endpoint.as_str(),
        request.state.config.allow_private_provider_urls,
        request
            .state
            .config
            .connect_timeout
            .min(std::time::Duration::from_secs(3)),
        request
            .state
            .config
            .request_timeout
            .min(std::time::Duration::from_secs(10)),
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        request.state.operational_settings().settings.upstream,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => return failed(None, format!("Provider egress check failed: {error}")),
    };
    let mut last_rejection = None;
    for (index, credential) in request.credentials.iter().enumerate() {
        let request_id = uuid::Uuid::new_v4().to_string();
        let mut body = model_probe_body(request.model, request.protocol);
        adapter.prepare_body(&mut body, &request_id);
        let secret = credential.as_deref();
        if secret.is_none() && request.auth_type != "none" {
            continue;
        }
        let preparation = match prepare_upstream_request(
            adapter.adapter_id(),
            AdapterRequestContext {
                state: request.state,
                base_url: request.base_url,
                adapter_base_url_override: None,
                model: request.model,
                request_id: &request_id,
                streaming: false,
                auth: UpstreamAuthContext {
                    auth_type: request.auth_type,
                    auth_header: request.auth_header,
                    secret,
                    oauth_auth: None,
                    session_id: &request_id,
                    protocol: request.protocol.into(),
                },
            },
            &mut body,
        )
        .await
        {
            Ok(preparation) => preparation,
            Err(error)
                if request.credentials.len() > 1
                    && index + 1 < request.credentials.len()
                    && matches!(
                        error.status,
                        Some(
                            http::StatusCode::UNAUTHORIZED
                                | http::StatusCode::FORBIDDEN
                                | http::StatusCode::TOO_MANY_REQUESTS
                        )
                    ) =>
            {
                last_rejection = error.status.map(|status| (status.as_u16(), None));
                continue;
            }
            Err(error) => {
                return AdapterModelTestOutcome {
                    test_passed: false,
                    status: error.status.map(|status| status.as_u16()),
                    message: error.message,
                    provider_response_body: error.provider_response_body,
                };
            }
        };
        let mut provider_request = client
            .post(endpoint.clone())
            .json(&body)
            .header("x-request-id", &request_id);
        provider_request = match crate::provider_adapters::apply_custom_headers(
            provider_request,
            request.custom_headers,
        ) {
            Ok(request) => request,
            Err(error) => return failed(None, error),
        };
        if request.protocol == Protocol::Messages {
            provider_request = provider_request.header("anthropic-version", "2023-06-01");
        }
        let empty_client_headers = HeaderMap::new();
        provider_request = adapter.apply_client_headers(provider_request, &empty_client_headers);
        for (name, value) in &preparation.headers {
            provider_request = provider_request.header(name, value);
        }
        provider_request = match adapter.apply_upstream_request_auth(
            provider_request,
            UpstreamAuthContext {
                auth_type: request.auth_type,
                auth_header: request.auth_header,
                secret,
                oauth_auth: None,
                session_id: &request_id,
                protocol: request.protocol.into(),
            },
        ) {
            Ok(request) => request,
            Err(error) => return failed(None, error),
        };
        if capabilities(adapter.adapter_id())
            .is_some_and(|capabilities| capabilities.local_quota_tracking)
        {
            request
                .state
                .record_local_quota_attempt(request.provider_id)
                .await;
        }
        let response = provider_request.send().await;
        let response_status = response.as_ref().ok().map(reqwest::Response::status);
        adapter
            .finish_upstream_request(
                request.state,
                preparation,
                UpstreamAuthContext {
                    auth_type: request.auth_type,
                    auth_header: request.auth_header,
                    secret,
                    oauth_auth: None,
                    session_id: &request_id,
                    protocol: request.protocol.into(),
                },
                response_status,
            )
            .await;
        match response {
            Ok(response)
                if request.credentials.len() > 1
                    && index + 1 < request.credentials.len()
                    && adapter.retry_upstream_response_as_key_rejection()
                    && matches!(
                        response.status(),
                        http::StatusCode::UNAUTHORIZED
                            | http::StatusCode::FORBIDDEN
                            | http::StatusCode::TOO_MANY_REQUESTS
                    ) =>
            {
                let status = response.status().as_u16();
                let outcome = model_probe_response(adapter, response).await;
                last_rejection = Some((status, outcome.provider_response_body));
            }
            Ok(response) => return model_probe_response(adapter, response).await,
            Err(error) => {
                return failed(
                    None,
                    format!("Could not reach provider: {}", error.without_url()),
                );
            }
        }
    }
    if let Some((status, provider_response_body)) = last_rejection {
        AdapterModelTestOutcome {
            test_passed: false,
            status: Some(status),
            message: format!("Provider rejected all enabled API keys (HTTP {status})"),
            provider_response_body,
        }
    } else {
        failed(
            Some(http::StatusCode::SERVICE_UNAVAILABLE.as_u16()),
            format!(
                "Provider '{}' has no usable API key for this model test",
                request.provider_id
            ),
        )
    }
}

async fn model_probe_response<A: ProviderAdapter + ?Sized>(
    adapter: &A,
    response: reqwest::Response,
) -> AdapterModelTestOutcome {
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
            return AdapterModelTestOutcome {
                test_passed: false,
                status: Some(http::StatusCode::BAD_GATEWAY.as_u16()),
                message: format!("Provider test response could not be read: {error}"),
                provider_response_body: None,
            };
        }
    };
    let provider_response_body = model_test_provider_response_body(&body);
    let value = match serde_json::from_slice::<Value>(&body) {
        Ok(value) => value,
        Err(_) => {
            return AdapterModelTestOutcome {
                test_passed: false,
                status: Some(http::StatusCode::BAD_GATEWAY.as_u16()),
                message: "Provider returned a non-JSON inference response".to_owned(),
                provider_response_body: provider_response_body.clone(),
            };
        }
    };
    if let Err(error) = adapter.normalize_response(value) {
        return AdapterModelTestOutcome {
            test_passed: false,
            status: Some(http::StatusCode::BAD_GATEWAY.as_u16()),
            message: error,
            provider_response_body,
        };
    }
    AdapterModelTestOutcome {
        test_passed: true,
        status: Some(status.as_u16()),
        message: "Model responded successfully".to_owned(),
        provider_response_body: None,
    }
}
