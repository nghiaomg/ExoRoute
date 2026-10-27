use super::*;
use provider::ProviderCredential;
use target_preparation::AttemptContext;

/// One step of the send classification: rotate to the next credential, or
/// end the credential loop with the given outcome.
pub(super) enum SendStep {
    Rotate,
    Completed(ProviderUpstreamResult),
    Failed(DispatchOutcome),
}

fn build_upstream_request(
    attempt: &AttemptContext<'_>,
    credential_secret: Option<&str>,
    oauth_auth: Option<&provider_adapters::OAuthRequestAuth>,
    preparation: &provider_adapters::AdapterRequestPreparation,
    session_id: &str,
) -> Result<reqwest::RequestBuilder, String> {
    let provider = &attempt.provider;
    let mut request = attempt
        .provider_http
        .post(attempt.endpoint.clone())
        .json(&attempt.upstream_body)
        .header("x-request-id", &attempt.scope.request_id);
    if provider_adapters::wants_event_stream(&provider.adapter_id)
        || (attempt.scope.canonical_stream
            && attempt.upstream_protocol == UpstreamProtocol::GoogleGenerateContent)
    {
        request = request.header("Accept", "text/event-stream");
    }
    if attempt.upstream_protocol == UpstreamProtocol::Messages {
        request = request.header("anthropic-version", "2023-06-01");
    }
    request = provider_adapters::apply_custom_headers(request, &provider.custom_headers)?;
    request = attempt
        .adapter
        .apply_client_headers(request, &attempt.scope.client_headers);
    for (name, value) in &preparation.headers {
        request = request.header(name, value);
    }
    provider_adapters::apply_upstream_request_auth(
        &provider.adapter_id,
        request,
        provider_adapters::UpstreamAuthContext {
            auth_type: &provider.auth_type,
            auth_header: provider.auth_header.as_deref(),
            secret: credential_secret,
            oauth_auth,
            session_id,
            protocol: attempt.upstream_protocol,
            model: Some(attempt.target_model),
        },
    )
}

/// Builds and sends the upstream request for one credential. The outcome
/// classification lives in `classify.rs`.
#[allow(clippy::too_many_arguments)]
pub(super) async fn send_request(
    attempt: &mut AttemptContext<'_>,
    failures: &mut TargetLoopFailures,
    log: &RequestLogPayload<'_>,
    live: &RequestLiveGuard,
    credential: &ProviderCredential,
    credential_secret: Option<&str>,
    oauth_auth: Option<&provider_adapters::OAuthRequestAuth>,
    preparation: provider_adapters::AdapterRequestPreparation,
) -> SendStep {
    let provider = &attempt.provider;
    let provider_id = &provider.id;
    let request = match build_upstream_request(
        attempt,
        credential_secret,
        oauth_auth,
        &preparation,
        &attempt.scope.adapter_session_id,
    ) {
        Ok(request) => request,
        Err(error) => {
            failures.last_error = error;
            return SendStep::Failed(DispatchOutcome::TargetFailed);
        }
    };
    if provider_adapters::capabilities(&provider.adapter_id)
        .is_some_and(|capabilities| capabilities.local_quota_tracking)
    {
        attempt
            .scope
            .state
            .record_local_quota_attempt(provider_id)
            .await;
    }
    let mut response = request.send().await;
    let is_auth_rejection = response.as_ref().ok().is_some_and(|r| {
        let status = r.status();
        (status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN)
            && (provider.adapter_id == provider_adapters::OPENCODE_GO_ADAPTER_ID
                || provider.adapter_id == provider_adapters::OPENCODE_ZEN_ADAPTER_ID)
    });
    if is_auth_rejection {
        tracing::debug!(
            provider_id = %provider_id,
            "OpenCode upstream returned 401/403; retrying once with refreshed session"
        );
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let fresh_session_id = uuid::Uuid::new_v4().to_string();
        if let Ok(retry_request) = build_upstream_request(
            attempt,
            credential_secret,
            oauth_auth,
            &preparation,
            &fresh_session_id,
        ) {
            let retried = retry_request.send().await;
            if retried
                .as_ref()
                .ok()
                .is_some_and(|r| r.status().is_success())
            {
                response = retried;
            }
        }
    }
    let response_status = response.as_ref().ok().map(|response| response.status());
    provider_adapters::finish_upstream_request(
        &provider.adapter_id,
        &attempt.scope.state,
        preparation,
        provider_adapters::UpstreamAuthContext {
            auth_type: &provider.auth_type,
            auth_header: provider.auth_header.as_deref(),
            secret: credential_secret,
            oauth_auth,
            session_id: &attempt.scope.adapter_session_id,
            protocol: attempt.upstream_protocol,
            model: Some(attempt.target_model),
        },
        response_status,
    )
    .await;
    classify::classify_response(attempt, failures, log, live, credential, response).await
}
