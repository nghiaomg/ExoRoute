use super::*;
use outcome::{CompletedUpstream, DispatchOutcome};
use provider::ProviderCredential;
use send::SendStep;

/// Runs the credential loop for one prepared target and classifies the
/// upstream outcome into a completed result (with its RAII permits) or a
/// failure the orchestrator can act on.
///
/// The attempt context is consumed: failure branches drop its permits, the
/// success branch moves them into [`CompletedUpstream`] so the streaming
/// path keeps the original release semantics.
pub(super) async fn dispatch_target(
    mut attempt: AttemptContext<'_>,
    failures: &mut TargetLoopFailures,
    log: &RequestLogPayload<'_>,
    live: &RequestLiveGuard,
    rotation: &mut CredentialRotation,
) -> DispatchOutcome {
    let provider_id = attempt.provider.id.clone();
    while let Some(current_credential) = rotation.current().cloned() {
        if credential_rotation::quota_cooldown_skip(
            &attempt.scope.state,
            &attempt.provider,
            &current_credential,
            attempt.target_model,
        )
        .await
        {
            failures.last_status = StatusCode::TOO_MANY_REQUESTS;
            failures.last_error = format!(
                "provider '{}' account is in an Antigravity quota cooldown for model '{}'",
                provider_id, attempt.target_model
            );
            if let Some(outcome) = advance_rotation(rotation, &attempt.scope.state).await {
                return outcome;
            }
            continue;
        }
        let (credential_secret, oauth_auth) = match credential_rotation::resolve_credential_auth(
            &attempt.scope.state,
            &attempt.provider,
            &current_credential,
        )
        .await
        {
            credential_rotation::ResolvedAuth::Ready { secret, oauth_auth } => (secret, oauth_auth),
            credential_rotation::ResolvedAuth::Rotate(message) => {
                failures.last_error = message;
                if let Some(outcome) = advance_rotation(rotation, &attempt.scope.state).await {
                    return outcome;
                }
                continue;
            }
        };
        let preparation = match provider_adapters::prepare_upstream_request(
            &attempt.provider.adapter_id,
            provider_adapters::AdapterRequestContext {
                state: &attempt.scope.state,
                base_url: &attempt.provider.base_url,
                adapter_base_url_override: attempt.scope.adapter_base_url_override.as_deref(),
                model: attempt.target_model,
                request_id: &attempt.scope.request_id,
                streaming: attempt.scope.canonical_stream,
                auth: provider_adapters::UpstreamAuthContext {
                    auth_type: &attempt.provider.auth_type,
                    auth_header: attempt.provider.auth_header.as_deref(),
                    secret: credential_secret.as_deref(),
                    oauth_auth: oauth_auth.as_ref(),
                    session_id: &attempt.scope.adapter_session_id,
                    protocol: attempt.upstream_protocol,
                },
            },
            &mut attempt.upstream_body,
        )
        .await
        {
            Ok(preparation) => preparation,
            Err(error)
                if current_credential.id.is_some()
                    && error.status.is_some_and(|status| {
                        provider_adapters::is_key_rejection_status(
                            &attempt.provider.adapter_id,
                            status,
                        )
                    }) =>
            {
                if handle_key_rejection(&attempt, failures, log, live, &current_credential, error)
                    .await
                {
                    if let Some(outcome) = advance_rotation(rotation, &attempt.scope.state).await {
                        return outcome;
                    }
                    continue;
                }
                return DispatchOutcome::Terminal;
            }
            Err(error) => {
                let status = error.status.unwrap_or(StatusCode::BAD_GATEWAY);
                failures.last_status = status;
                failures.last_error = format!(
                    "provider '{}' adapter preflight failed: {}",
                    provider_id, error.message
                );
                attempt.circuit_probe.failed();
                log_request(
                    &attempt.scope.state,
                    Some(live),
                    log.provider_failure(
                        &provider_id,
                        attempt.upstream_protocol,
                        current_credential.id.as_deref(),
                        status,
                        &failures.last_error,
                        attempt.target_model,
                    ),
                )
                .await;
                // Permits drop with the attempt, recording the failure.
                return DispatchOutcome::Terminal;
            }
        };
        match send::send_request(
            &mut attempt,
            failures,
            log,
            live,
            &current_credential,
            credential_secret.as_deref(),
            oauth_auth.as_ref(),
            preparation,
        )
        .await
        {
            SendStep::Rotate => {
                if let Some(outcome) = advance_rotation(rotation, &attempt.scope.state).await {
                    return outcome;
                }
            }
            SendStep::Completed(result) => {
                return DispatchOutcome::Completed(Box::new(CompletedUpstream {
                    result,
                    circuit_probe: attempt.circuit_probe,
                    provider_permit: attempt.provider_permit,
                    adapter: attempt.adapter,
                    adapter_id: attempt.provider.adapter_id.clone(),
                    provider_id: attempt.provider.id.clone(),
                    upstream_protocol: attempt.upstream_protocol,
                    target_model: attempt.target_model.to_owned(),
                }));
            }
            SendStep::Failed(outcome) => {
                return outcome;
            }
        }
    }
    outcome::all_credentials_rejected(&mut attempt, failures, log, live).await
}

/// Advances rotation; `Some` means storage failed and the loop must stop.
async fn advance_rotation(
    rotation: &mut CredentialRotation,
    state: &AppState,
) -> Option<DispatchOutcome> {
    match rotation.advance(state).await {
        Ok(()) => None,
        Err(error) => Some(DispatchOutcome::Storage(error)),
    }
}

/// Records a credential-level key rejection. Returns `true` when the loop
/// should rotate to the next credential, `false` to stop the target loop.
async fn handle_key_rejection(
    attempt: &AttemptContext<'_>,
    failures: &mut TargetLoopFailures,
    log: &RequestLogPayload<'_>,
    live: &RequestLiveGuard,
    credential: &ProviderCredential,
    error: provider_adapters::AdapterRequestError,
) -> bool {
    if let Err(_storage_error) =
        outcome::record_credential_rejection(attempt, failures, log, live, credential, error).await
    {
        failures.last_status = StatusCode::SERVICE_UNAVAILABLE;
        failures.last_error = "could not persist provider key failure state".to_owned();
        return false;
    }
    true
}
