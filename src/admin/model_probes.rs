use super::*;
#[cfg(test)]
use crate::protocol::Protocol;
use crate::protocol::UpstreamProtocol;
use crate::provider_adapters::keys::ProviderApiKeyCursor;

pub(super) const MAX_MODEL_TEST_ITEMS: usize = 32;
const MAX_PARALLEL_MODEL_TESTS: usize = 8;

#[derive(Clone, Copy)]
struct SavedModelTestConfig<'a> {
    adapter_id: &'a str,
    base_url: &'a str,
    provider_id: &'a str,
    model: &'a str,
    auth_type: &'a str,
    auth_header: Option<&'a str>,
    custom_headers: &'a std::collections::BTreeMap<String, String>,
    protocol: UpstreamProtocol,
}

#[derive(Clone, Debug)]
struct ModelTestResult {
    provider_id: String,
    model: String,
    test_passed: bool,
    status: Option<u16>,
    latency_ms: u64,
    message: String,
    provider_response_body: Option<String>,
}

impl ModelTestResult {
    fn json(&self) -> Value {
        json!({
            "provider_id": self.provider_id,
            "model": self.model,
            "test_passed": self.test_passed,
            "status": self.status,
            "latency_ms": self.latency_ms,
            "message": self.message,
            "provider_response_body": self.provider_response_body,
        })
    }

    fn invalid(provider_id: String, model: String, message: impl Into<String>) -> Self {
        Self {
            provider_id,
            model,
            test_passed: false,
            status: Some(StatusCode::BAD_REQUEST.as_u16()),
            latency_ms: 0,
            message: message.into(),
            provider_response_body: None,
        }
    }
}

pub(super) async fn test_models(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> ApiResult {
    let entries = input
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| fail(StatusCode::BAD_REQUEST, "models must be an array"))?;
    if entries.is_empty() {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "select at least one model to test",
        ));
    }
    if entries.len() > MAX_MODEL_TEST_ITEMS {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            format!("a single test request can include at most {MAX_MODEL_TEST_ITEMS} models"),
        ));
    }

    // Keep response order aligned with the submitted selections while bounding
    // concurrent upstream inference calls to avoid accidental provider bursts.
    let results = futures_util::stream::iter(entries.iter().cloned())
        .map(|entry| {
            let state = state.clone();
            async move { test_one_model(&state, &entry).await }
        })
        .buffered(MAX_PARALLEL_MODEL_TESTS)
        .collect::<Vec<_>>()
        .await;
    Ok(Json(
        json!({"results":results.iter().map(ModelTestResult::json).collect::<Vec<_>>()}),
    ))
}

async fn test_one_model(state: &AppState, entry: &Value) -> ModelTestResult {
    let provider_id = entry
        .get("provider_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    let model = entry
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    if provider_id.is_empty() || model.is_empty() {
        return ModelTestResult::invalid(
            provider_id,
            model,
            "provider_id and model must be non-empty strings",
        );
    }
    if provider_id.len() > 256 || model.len() > 512 {
        return ModelTestResult::invalid(provider_id, model, "provider_id or model is too long");
    }
    let _probe_permit = match state
        .admin
        .admin_provider_probe_in_flight
        .clone()
        .try_acquire_owned()
    {
        Ok(permit) => permit,
        Err(_) => {
            return ModelTestResult {
                provider_id,
                model,
                test_passed: false,
                status: Some(StatusCode::SERVICE_UNAVAILABLE.as_u16()),
                latency_ms: 0,
                message: "Provider probe capacity is busy; retry shortly".to_owned(),
                provider_response_body: None,
            };
        }
    };

    let lookup_provider_id = provider_id.clone();
    let lookup_model = model.clone();
    let (provider, is_saved) = match state
        .db
        .read(move |transaction| {
            let provider = transaction.get::<crate::infra::storage::Record>(
                crate::infra::storage::Table::Providers,
                &lookup_provider_id,
            )?;
            let is_saved = if provider.is_some() {
                let index_key =
                    crate::infra::db::provider_model_index_key(&lookup_provider_id, &lookup_model)?;
                transaction
                    .get::<String>(crate::infra::storage::Table::ProviderModelIndex, &index_key)?
                    .is_some_and(|stored_model| stored_model == lookup_model)
            } else {
                false
            };
            Ok((provider, is_saved))
        })
        .await
    {
        Ok(result) => result,
        Err(error) => return model_test_database_error(provider_id, model, error),
    };
    let Some(provider) = provider else {
        return ModelTestResult {
            provider_id,
            model,
            test_passed: false,
            status: Some(StatusCode::NOT_FOUND.as_u16()),
            latency_ms: 0,
            message: "Provider was not found".to_owned(),
            provider_response_body: None,
        };
    };
    if !is_saved {
        return ModelTestResult {
            provider_id,
            model,
            test_passed: false,
            status: Some(StatusCode::NOT_FOUND.as_u16()),
            latency_ms: 0,
            message: "Model is not saved for this provider; import the provider model list first"
                .to_owned(),
            provider_response_body: None,
        };
    }

    let base_url = match provider.text("base_url") {
        Ok(value) => value.to_owned(),
        Err(error) => return model_test_database_error(provider_id, model, error),
    };
    let adapter_id = match provider.text("adapter_id") {
        Ok(value) => value.to_owned(),
        Err(error) => return model_test_database_error(provider_id, model, error),
    };
    let auth_type = match provider.text("auth_type") {
        Ok(value) => value.to_owned(),
        Err(error) => return model_test_database_error(provider_id, model, error),
    };
    let auth_header: Option<String> = match provider.optional_text("auth_header") {
        Ok(value) => value.map(str::to_owned),
        Err(error) => return model_test_database_error(provider_id, model, error),
    };
    let custom_headers = match crate::admin::providers::stored_custom_headers(
        &provider,
        state.config.master_key.as_ref(),
    ) {
        Ok(headers) => headers,
        Err(error) => return model_test_database_error(provider_id, model, error),
    };
    let protocol_name = match provider.text("preferred_protocol") {
        Ok(value) => value.to_owned(),
        Err(error) => return model_test_database_error(provider_id, model, error),
    };
    let protocol = match parse_model_test_protocol(&protocol_name) {
        Ok(protocol) => protocol,
        Err(_) => {
            return ModelTestResult {
                provider_id,
                model,
                test_passed: false,
                status: Some(StatusCode::BAD_REQUEST.as_u16()),
                latency_ms: 0,
                message: format!("Provider has unsupported preferred protocol '{protocol_name}'"),
                provider_response_body: None,
            };
        }
    };
    let test_config = SavedModelTestConfig {
        adapter_id: &adapter_id,
        base_url: &base_url,
        provider_id: &provider_id,
        model: &model,
        auth_type: &auth_type,
        auth_header: auth_header.as_deref(),
        custom_headers: &custom_headers,
        protocol,
    };
    let started = std::time::Instant::now();
    let capabilities = provider_adapters::capabilities(&adapter_id);
    if capabilities.is_some_and(|capabilities| capabilities.api_keys && capabilities.oauth_accounts)
    {
        return test_mixed_credentials(state, test_config, started).await;
    }

    if provider_adapters::is_oauth_only_adapter(&adapter_id) {
        let (mut cursor, first_account) =
            match ProviderApiKeyCursor::start(&state.db, &provider_id).await {
                Ok(Some(result)) => result,
                Ok(None) => {
                    return ModelTestResult {
                        provider_id,
                        model,
                        test_passed: false,
                        status: Some(StatusCode::SERVICE_UNAVAILABLE.as_u16()),
                        latency_ms: 0,
                        message: "Connect a provider account before testing models".to_owned(),
                        provider_response_body: None,
                    };
                }
                Err(error) => return model_test_database_error(provider_id, model, error),
            };
        let mut current_account = Some(first_account);
        let mut last = provider_adapters::AdapterModelTestOutcome {
            test_passed: false,
            status: Some(StatusCode::SERVICE_UNAVAILABLE.as_u16()),
            message: "No provider account was available".to_owned(),
            provider_response_body: None,
        };
        while let Some(account) = current_account.take() {
            last = provider_adapters::test_oauth_model(
                &adapter_id,
                state,
                &base_url,
                &model,
                &account.id,
            )
            .await;
            if last.test_passed {
                break;
            }
            current_account = match cursor.next(&state.db).await {
                Ok(account) => account,
                Err(error) => return model_test_database_error(provider_id, model, error),
            };
        }
        return model_test_outcome_result(provider_id, model, started, last);
    }

    if auth_type == "none" {
        let outcome = test_saved_model_credential(state, test_config, None).await;
        return model_test_outcome_result(provider_id, model, started, outcome);
    }

    let (mut cursor, first_key) =
        match ProviderApiKeyCursor::start(&state.db, &provider_id).await {
            Ok(result) => result,
            Err(error) => return model_test_database_error(provider_id, model, error),
        }
        .map_or((None, None), |(cursor, key)| (Some(cursor), Some(key)));
    let mut current_key = first_key;
    let mut last_rejection = None;
    let mut had_usable_key = false;
    while let Some(key) = current_key.take() {
        let secret = match decrypt_secret(
            state.config.master_key.as_ref(),
            Some(&key.encrypted_secret),
        ) {
            Ok(secret) => secret,
            Err(error) => {
                return ModelTestResult {
                    provider_id,
                    model,
                    test_passed: false,
                    status: Some(StatusCode::INTERNAL_SERVER_ERROR.as_u16()),
                    latency_ms: 0,
                    message: error,
                    provider_response_body: None,
                };
            }
        };
        if let Some(secret) = secret {
            had_usable_key = true;
            let outcome = test_saved_model_credential(state, test_config, Some(secret)).await;
            let retry = !outcome.test_passed
                && matches!(
                    outcome.status,
                    Some(status)
                        if status == StatusCode::UNAUTHORIZED.as_u16()
                            || status == StatusCode::FORBIDDEN.as_u16()
                );
            if !retry {
                return model_test_outcome_result(provider_id, model, started, outcome);
            }
            last_rejection = Some(outcome);
        }
        current_key = match cursor.as_mut() {
            Some(cursor) => match cursor.next(&state.db).await {
                Ok(key) => key,
                Err(error) => return model_test_database_error(provider_id, model, error),
            },
            None => None,
        };
    }
    if let Some(outcome) = last_rejection {
        return model_test_outcome_result(provider_id, model, started, outcome);
    }
    if !had_usable_key {
        let legacy_secret = match provider.optional_bytes("secret") {
            Ok(value) => value,
            Err(error) => return model_test_database_error(provider_id, model, error),
        };
        let secret = match decrypt_secret(state.config.master_key.as_ref(), legacy_secret) {
            Ok(secret) => secret,
            Err(error) => {
                return ModelTestResult {
                    provider_id,
                    model,
                    test_passed: false,
                    status: Some(StatusCode::INTERNAL_SERVER_ERROR.as_u16()),
                    latency_ms: 0,
                    message: error,
                    provider_response_body: None,
                };
            }
        };
        if let Some(secret) = secret {
            let outcome = test_saved_model_credential(state, test_config, Some(secret)).await;
            return model_test_outcome_result(provider_id, model, started, outcome);
        }
    }
    ModelTestResult {
        provider_id,
        model,
        test_passed: false,
        status: Some(StatusCode::SERVICE_UNAVAILABLE.as_u16()),
        latency_ms: 0,
        message: "Provider has no enabled valid API keys".to_owned(),
        provider_response_body: None,
    }
}

async fn test_mixed_credentials(
    state: &AppState,
    config: SavedModelTestConfig<'_>,
    started: std::time::Instant,
) -> ModelTestResult {
    let (mut cursor, first) = match ProviderApiKeyCursor::start(&state.db, config.provider_id).await
    {
        Ok(Some(result)) => result,
        Ok(None) => {
            return ModelTestResult {
                provider_id: config.provider_id.to_owned(),
                model: config.model.to_owned(),
                test_passed: false,
                status: Some(StatusCode::SERVICE_UNAVAILABLE.as_u16()),
                latency_ms: 0,
                message: "Connect an account or add an API key before testing models".to_owned(),
                provider_response_body: None,
            };
        }
        Err(error) => {
            return model_test_database_error(
                config.provider_id.to_owned(),
                config.model.to_owned(),
                error,
            );
        }
    };
    let mut current = Some(first);
    let mut last = provider_adapters::AdapterModelTestOutcome {
        test_passed: false,
        status: Some(StatusCode::SERVICE_UNAVAILABLE.as_u16()),
        message: "No provider credential was available".to_owned(),
        provider_response_body: None,
    };
    while let Some(credential) = current.take() {
        last = match credential.credential_type {
            crate::provider_adapters::keys::ProviderCredentialType::OAuth => {
                provider_adapters::test_oauth_model(
                    config.adapter_id,
                    state,
                    config.base_url,
                    config.model,
                    &credential.id,
                )
                .await
            }
            crate::provider_adapters::keys::ProviderCredentialType::ApiKey => {
                match decrypt_secret(
                    state.config.master_key.as_ref(),
                    Some(&credential.encrypted_secret),
                ) {
                    Ok(Some(secret)) => {
                        test_saved_model_credential(state, config, Some(secret)).await
                    }
                    Ok(None) => provider_adapters::AdapterModelTestOutcome {
                        test_passed: false,
                        status: None,
                        message: "Provider API key is empty".to_owned(),
                        provider_response_body: None,
                    },
                    Err(error) => {
                        return ModelTestResult {
                            provider_id: config.provider_id.to_owned(),
                            model: config.model.to_owned(),
                            test_passed: false,
                            status: Some(StatusCode::INTERNAL_SERVER_ERROR.as_u16()),
                            latency_ms: 0,
                            message: error,
                            provider_response_body: None,
                        };
                    }
                }
            }
        };
        if last.test_passed
            || !matches!(last.status, Some(status)
                if status == StatusCode::UNAUTHORIZED.as_u16()
                    || status == StatusCode::FORBIDDEN.as_u16()
                    || status == StatusCode::TOO_MANY_REQUESTS.as_u16())
        {
            return model_test_outcome_result(
                config.provider_id.to_owned(),
                config.model.to_owned(),
                started,
                last,
            );
        }
        current = match cursor.next(&state.db).await {
            Ok(next) => next,
            Err(error) => {
                return model_test_database_error(
                    config.provider_id.to_owned(),
                    config.model.to_owned(),
                    error,
                );
            }
        };
    }
    model_test_outcome_result(
        config.provider_id.to_owned(),
        config.model.to_owned(),
        started,
        last,
    )
}

async fn test_saved_model_credential(
    state: &AppState,
    config: SavedModelTestConfig<'_>,
    credential: Option<String>,
) -> provider_adapters::AdapterModelTestOutcome {
    let Some(protocol) = config.protocol.client_protocol() else {
        return provider_adapters::AdapterModelTestOutcome {
            test_passed: false,
            status: Some(StatusCode::BAD_REQUEST.as_u16()),
            message: format!(
                "Provider upstream protocol '{}' cannot be tested with an API-key model probe",
                config.protocol.as_str()
            ),
            provider_response_body: None,
        };
    };
    let credentials = [credential];
    provider_adapters::test_api_key_model(
        config.adapter_id,
        provider_adapters::AdapterModelTestRequest {
            state,
            base_url: config.base_url,
            provider_id: config.provider_id,
            model: config.model,
            auth_type: config.auth_type,
            auth_header: config.auth_header,
            custom_headers: config.custom_headers,
            protocol,
            credentials: &credentials,
        },
    )
    .await
}

fn parse_model_test_protocol(value: &str) -> Result<UpstreamProtocol, String> {
    value.parse::<UpstreamProtocol>()
}

fn model_test_outcome_result(
    provider_id: String,
    model: String,
    started: std::time::Instant,
    outcome: provider_adapters::AdapterModelTestOutcome,
) -> ModelTestResult {
    ModelTestResult {
        provider_id,
        model,
        test_passed: outcome.test_passed,
        status: outcome.status,
        latency_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        message: outcome.message,
        provider_response_body: outcome.provider_response_body,
    }
}

fn model_test_database_error(
    provider_id: String,
    model: String,
    error: impl std::fmt::Display,
) -> ModelTestResult {
    tracing::error!(%error, "could not load model test configuration");
    ModelTestResult {
        provider_id,
        model,
        test_passed: false,
        status: Some(StatusCode::INTERNAL_SERVER_ERROR.as_u16()),
        latency_ms: 0,
        message: "Could not load model test configuration".to_owned(),
        provider_response_body: None,
    }
}

#[cfg(test)]
pub(super) fn model_test_endpoint_url(
    base_url: &str,
    protocol: Protocol,
) -> Result<reqwest::Url, String> {
    provider_adapters::adapter(provider_adapters::GENERIC_ADAPTER_ID)
        .expect("generic adapter is registered")
        .endpoint(base_url, protocol, None)
}

#[cfg(test)]
pub(super) fn model_probe_body(model: &str, protocol: Protocol) -> Value {
    provider_adapters::model_probe_body(model, protocol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_probes_are_non_streaming_and_use_protocol_specific_paths() {
        for (protocol, path) in [
            (Protocol::ChatCompletions, "/v1/chat/completions"),
            (Protocol::Responses, "/v1/responses"),
            (Protocol::Messages, "/v1/messages"),
        ] {
            let body = model_probe_body("sample-model", protocol);
            assert_eq!(body["model"], "sample-model");
            assert_eq!(body["stream"], false);
            assert_eq!(
                model_test_endpoint_url("https://api.example.com/v1", protocol)
                    .expect("valid model test URL")
                    .path(),
                path,
            );
        }
        assert_eq!(
            model_probe_body("sample-model", Protocol::Responses)["max_output_tokens"],
            1
        );
        let responses_body = model_probe_body("sample-model", Protocol::Responses);
        assert!(responses_body["input"].is_array());
        assert_eq!(
            responses_body["input"][0]["content"][0],
            json!({"type":"input_text","text":"Reply with OK."})
        );
    }

    #[test]
    fn model_tests_accept_provider_only_google_protocols() {
        assert_eq!(
            parse_model_test_protocol("google_generate_content").expect("Google protocol"),
            UpstreamProtocol::GoogleGenerateContent
        );
        assert!(
            parse_model_test_protocol("unsupported_protocol").is_err(),
            "unknown protocols must still be rejected"
        );
    }
}
