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
    request = match provider_adapters::apply_custom_headers(request, &provider.custom_headers) {
        Ok(request) => request,
        Err(error) => {
            failures.last_error = error;
            return SendStep::Failed(DispatchOutcome::TargetFailed);
        }
    };
    request = attempt
        .adapter
        .apply_client_headers(request, &attempt.scope.client_headers);
    for (name, value) in &preparation.headers {
        request = request.header(name, value);
    }
    let request = match provider_adapters::apply_upstream_request_auth(
        &provider.adapter_id,
        request,
        provider_adapters::UpstreamAuthContext {
            auth_type: &provider.auth_type,
            auth_header: provider.auth_header.as_deref(),
            secret: credential_secret,
            oauth_auth,
            session_id: &attempt.scope.adapter_session_id,
            protocol: attempt.upstream_protocol,
        },
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
    let response = request.send().await;
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
        },
        response_status,
    )
    .await;
    classify::classify_response(attempt, failures, log, live, credential, response).await
}
