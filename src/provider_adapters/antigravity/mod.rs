use super::auth::antigravity as antigravity_oauth;
use super::*;
use crate::{protocol::UpstreamProtocol, state::AppState};
use http::StatusCode;

const ANTIGRAVITY_MODEL_TEST_MAX_REQUEST_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(30);
const ANTIGRAVITY_MODEL_TEST_MAX_OUTPUT_TOKENS: u64 = 128;

pub(super) struct AntigravityAdapter;

pub(super) static ANTIGRAVITY_ADAPTER: AntigravityAdapter = AntigravityAdapter;

impl ProviderAdapter for AntigravityAdapter {
    fn adapter_id(&self) -> &'static str {
        ANTIGRAVITY_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        _protocol: Protocol,
        adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        self.chat_endpoint(
            adapter_base_url_override.unwrap_or(base_url),
            adapter_base_url_override.is_some(),
        )
    }

    fn upstream_endpoint(
        &self,
        base_url: &str,
        protocol: UpstreamProtocol,
        _model: &str,
        _streaming: bool,
        adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != UpstreamProtocol::GoogleGenerateContent {
            return Err(
                "Antigravity requires Google Generate Content upstream protocol".to_owned(),
            );
        }
        self.chat_endpoint(
            adapter_base_url_override.unwrap_or(base_url),
            adapter_base_url_override.is_some(),
        )
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn validate_config(
        &self,
        preset: &ProviderPreset,
        auth_type: &str,
        base_url: &str,
        preferred_protocol: &str,
        supported_protocols: &[String],
        api_keys_present: bool,
    ) -> Result<(), &'static str> {
        if auth_type != preset.default_auth_type
            || base_url.trim_end_matches('/') != preset.default_base_url.unwrap_or_default()
            || preferred_protocol != preset.default_preferred_protocol
            || supported_protocols.len() != preset.default_supported_protocols.len()
            || !supported_protocols
                .iter()
                .zip(preset.default_supported_protocols)
                .all(|(actual, expected)| actual == expected)
            || api_keys_present
        {
            return Err(
                "Antigravity requires its fixed Cloud Code endpoint, Google Generate Content protocol, and OAuth accounts",
            );
        }
        Ok(())
    }

    fn apply_request_auth(
        &self,
        request: reqwest::RequestBuilder,
        _auth_type: &str,
        _auth_header: Option<&str>,
        _secret: Option<&str>,
        oauth_auth: Option<&OAuthRequestAuth>,
        _session_id: &str,
    ) -> Result<reqwest::RequestBuilder, String> {
        let auth = oauth_auth.ok_or("Antigravity OAuth account is unavailable")?;
        Ok(request
            .bearer_auth(&auth.access_token)
            .header("User-Agent", antigravity_oauth::antigravity_user_agent())
            .header("Accept", "text/event-stream")
            .header("X-Client-Name", "antigravity")
            .header(
                "X-Client-Version",
                antigravity_oauth::antigravity_ide_version(),
            ))
    }

    fn wants_event_stream(&self) -> bool {
        true
    }

    fn allows_missing_event_stream_content_type(&self) -> bool {
        true
    }

    fn is_key_rejection_status(&self, status: StatusCode) -> bool {
        matches!(
            status,
            StatusCode::UNAUTHORIZED
                | StatusCode::FORBIDDEN
                | StatusCode::CONFLICT
                | StatusCode::TOO_MANY_REQUESTS
        )
    }

    fn prepare_upstream_request<'a>(
        &'a self,
        context: AdapterRequestContext<'a>,
        body: &'a mut Value,
    ) -> AdapterFuture<'a, Result<AdapterRequestPreparation, AdapterRequestError>> {
        Box::pin(async move {
            let Some(auth) = context.auth.oauth_auth else {
                return Err(AdapterRequestError::new(
                    Some(StatusCode::UNAUTHORIZED),
                    None,
                    "Antigravity OAuth account is unavailable",
                ));
            };
            let Some(project_id) = auth
                .project_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            else {
                return Err(AdapterRequestError::new(
                    Some(StatusCode::UNPROCESSABLE_ENTITY),
                    None,
                    "Antigravity account has no Cloud Code project; reconnect the account after completing Gemini Code Assist onboarding",
                ));
            };
            let mut request = std::mem::take(body);
            let request_object = request.as_object_mut().ok_or_else(|| {
                AdapterRequestError::new(
                    Some(StatusCode::BAD_REQUEST),
                    None,
                    "Antigravity Google request is invalid",
                )
            })?;
            request_object.insert(
                "sessionId".to_owned(),
                Value::String(context.auth.session_id.to_owned()),
            );
            let model = normalize_model_id(context.model);
            *body = json!({
                "project": project_id,
                "model": model,
                "userAgent": "antigravity",
                "requestType": "agent",
                "requestId": context.request_id,
                "request": request,
            });
            apply_tiered_thinking_config(&mut body["request"], context.model);
            Ok(AdapterRequestPreparation::new(HeaderMap::new(), None, None))
        })
    }

    fn read_event_stream<'a>(
        &'a self,
        response: reqwest::Response,
        max_bytes: usize,
    ) -> AdapterFuture<'a, Result<Value, AdapterSseError>> {
        Box::pin(async move {
            let bytes = read_limited_response(response, max_bytes)
                .await
                .map_err(|error| AdapterSseError::new(error, true))?;
            parse_antigravity_event_stream(&bytes)
        })
    }

    fn discover_oauth_models<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Vec<String>, String>> {
        Box::pin(antigravity_oauth::discover_models(state, credential_id))
    }

    fn test_oauth_model<'a>(
        &'a self,
        state: &'a AppState,
        base_url: &'a str,
        model: &'a str,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, AdapterModelTestOutcome> {
        Box::pin(test_antigravity_model(
            state,
            base_url,
            model,
            credential_id,
        ))
    }

    fn authorization_url(
        &self,
        state: &str,
        challenge: &str,
        redirect_uri: Option<&str>,
    ) -> Result<String, String> {
        antigravity_oauth::authorization_url(state, challenge, redirect_uri)
    }

    fn supports_custom_oauth_redirect_uri(&self) -> bool {
        true
    }

    fn pkce_challenge(&self, verifier: &str) -> Result<String, String> {
        Ok(antigravity_oauth::pkce_challenge(verifier))
    }

    fn exchange_oauth_code<'a>(
        &'a self,
        code: &'a str,
        verifier: &'a str,
        redirect_uri: &'a str,
        connect_timeout: std::time::Duration,
        request_timeout: std::time::Duration,
        upstream: crate::config::UpstreamSettings,
    ) -> AdapterFuture<'a, Result<AdapterOAuthAccount, String>> {
        Box::pin(async move {
            let account = antigravity_oauth::exchange_code(
                code,
                verifier,
                redirect_uri,
                connect_timeout,
                request_timeout,
                upstream,
            )
            .await?;
            let display_name = account.display_name();
            let payload = serde_json::to_value(account)
                .map_err(|_| "could not encode the Antigravity account credentials".to_owned())?;
            Ok(AdapterOAuthAccount {
                payload,
                display_name,
            })
        })
    }

    fn resolve_oauth_request_auth<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Option<OAuthRequestAuth>, String>> {
        Box::pin(async move {
            let account = antigravity_oauth::account_for_use(state, credential_id).await?;
            Ok(Some(OAuthRequestAuth {
                access_token: account.access_token,
                account_id: None,
                project_id: account.project_id,
            }))
        })
    }

    fn fetch_oauth_usage<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<ProviderUsageSnapshot, String>> {
        Box::pin(antigravity_oauth::fetch_usage(state, credential_id))
    }

    fn save_oauth_account<'a>(
        &'a self,
        state: &'a AppState,
        provider_id: &'a str,
        account: AdapterOAuthAccount,
    ) -> AdapterFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let account: antigravity_oauth::AntigravityAccount =
                serde_json::from_value(account.payload).map_err(|_| {
                    "Google returned invalid Antigravity account credentials".to_owned()
                })?;
            antigravity_oauth::save_account(state, provider_id, account).await
        })
    }
}

impl AntigravityAdapter {
    fn chat_endpoint(&self, base_url: &str, allow_override: bool) -> Result<reqwest::Url, String> {
        if !allow_override && base_url.trim_end_matches('/') != antigravity_oauth::RUNTIME_BASE_URL
        {
            return Err(
                "Antigravity endpoint must be https://daily-cloudcode-pa.googleapis.com".to_owned(),
            );
        }
        let mut url = reqwest::Url::parse(base_url)
            .map_err(|_| "Antigravity endpoint is invalid".to_owned())?;
        if url.scheme() != "https" {
            return Err("Antigravity endpoint must use HTTPS".to_owned());
        }
        url.set_path("/v1internal:streamGenerateContent");
        url.query_pairs_mut().append_pair("alt", "sse");
        Ok(url)
    }
}

fn normalize_model_id(model: &str) -> String {
    antigravity_oauth::normalize_model_id(model)
}

fn model_thinking_level(model: &str) -> Option<&'static str> {
    let model = model.trim();
    let model = model
        .strip_prefix("antigravity/")
        .or_else(|| model.strip_prefix("ag/"))
        .unwrap_or(model);
    match model {
        "gemini-3.6-flash-high" | "gemini-3.7-flash-high" | "gemini-3.8-flash-high" => Some("high"),
        "gemini-3.6-flash-medium" | "gemini-3.7-flash-medium" | "gemini-3.8-flash-medium" => {
            Some("medium")
        }
        "gemini-3.6-flash-low" | "gemini-3.7-flash-low" | "gemini-3.8-flash-low" => Some("low"),
        _ => None,
    }
}

fn set_tiered_thinking_config(request: &mut Value, level: &str) {
    let Some(request) = request.as_object_mut() else {
        return;
    };
    let generation_config = request
        .entry("generationConfig".to_owned())
        .or_insert_with(|| json!({}));
    let Some(generation_config) = generation_config.as_object_mut() else {
        return;
    };
    let thinking_config = generation_config
        .entry("thinkingConfig".to_owned())
        .or_insert_with(|| json!({}));
    let Some(thinking_config) = thinking_config.as_object_mut() else {
        return;
    };
    thinking_config.insert("thinkingLevel".to_owned(), json!(level));
    thinking_config.insert("includeThoughts".to_owned(), json!(false));
}

fn apply_tiered_thinking_config(request: &mut Value, model: &str) {
    let Some(level) = model_thinking_level(model) else {
        return;
    };
    set_tiered_thinking_config(request, level);
}

fn apply_model_test_thinking_config(request: &mut Value, model: &str) {
    if !normalize_model_id(model).ends_with("-tiered") {
        return;
    }
    let level = model_thinking_level(model).unwrap_or("low");
    set_tiered_thinking_config(request, level);
}

async fn test_antigravity_model(
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
    let account = match antigravity_oauth::account_for_use(state, credential_id).await {
        Ok(account) => account,
        Err(error) => return failed(None, error),
    };
    let Some(project_id) = account.project_id.as_deref() else {
        return failed(
            Some(StatusCode::UNPROCESSABLE_ENTITY.as_u16()),
            "Antigravity account has no Cloud Code project".to_owned(),
        );
    };
    let endpoint = match ANTIGRAVITY_ADAPTER.chat_endpoint(base_url, false) {
        Ok(endpoint) => endpoint,
        Err(error) => return failed(None, error),
    };
    let connect_timeout = state
        .config
        .connect_timeout
        .min(std::time::Duration::from_secs(3));
    let request_timeout = state
        .config
        .request_timeout
        .min(ANTIGRAVITY_MODEL_TEST_MAX_REQUEST_TIMEOUT);
    let (endpoint, client) = match egress::provider_client(
        endpoint.as_str(),
        false,
        connect_timeout,
        request_timeout,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        state.operational_settings().settings.upstream,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => return failed(None, error),
    };
    let mut body = json!({
        "project": project_id,
        "model": normalize_model_id(model),
        "userAgent": "antigravity",
        "requestType": "agent",
        "requestId": uuid::Uuid::new_v4().to_string(),
            "request": {
                "contents": [{"role":"user","parts":[{"text":"Reply with OK."}]}],
                "sessionId": uuid::Uuid::new_v4().to_string(),
                "generationConfig": {"maxOutputTokens": ANTIGRAVITY_MODEL_TEST_MAX_OUTPUT_TOKENS}
            }
    });
    apply_model_test_thinking_config(&mut body["request"], model);
    let response = match client
        .post(endpoint)
        .bearer_auth(&account.access_token)
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .header("User-Agent", antigravity_oauth::antigravity_user_agent())
        .header("X-Client-Name", "antigravity")
        .header(
            "X-Client-Version",
            antigravity_oauth::antigravity_ide_version(),
        )
        .json(&body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            if error.is_timeout() {
                return failed(
                    None,
                    format!(
                        "Antigravity request timed out (connect timeout <= {} ms; request timeout <= {} ms)",
                        connect_timeout.as_millis(),
                        request_timeout.as_millis()
                    ),
                );
            }
            return failed(
                None,
                format!("Could not reach Antigravity: {}", error.without_url()),
            );
        }
    };
    let status = response.status();
    if !status.is_success() {
        return failed(
            Some(status.as_u16()),
            format!("Antigravity returned HTTP {}", status.as_u16()),
        );
    }
    let response_body = match read_limited_response(response, MAX_MODEL_TEST_RESPONSE_BYTES).await {
        Ok(body) => body,
        Err(error) => {
            return failed(
                None,
                format!("Antigravity response could not be read: {error}"),
            );
        }
    };
    match parse_antigravity_event_stream(&response_body) {
        Ok(_) => AdapterModelTestOutcome {
            test_passed: true,
            status: Some(status.as_u16()),
            message: "Model responded successfully".to_owned(),
            provider_response_body: None,
        },
        Err(error) => AdapterModelTestOutcome {
            test_passed: false,
            status: None,
            message: error.message,
            provider_response_body: model_test_provider_response_body(&response_body),
        },
    }
}

fn parse_antigravity_event_stream(bytes: &[u8]) -> Result<Value, AdapterSseError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| AdapterSseError::new("Antigravity returned an invalid event stream", true))?;
    let mut text_output = String::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    let mut finish_reason = None;
    let mut usage = None;
    let mut response_id = None;
    let mut model_version = None;
    let mut saw_event = false;
    let normalized = text.replace("\r\n", "\n");
    for block in normalized.split("\n\n").chain(std::iter::once("")) {
        let data = block
            .lines()
            .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let value: Value = serde_json::from_str(&data)
            .map_err(|_| AdapterSseError::new("Antigravity returned an invalid event", true))?;
        saw_event = true;
        if value.get("error").is_some() {
            return Err(AdapterSseError::new(
                "Antigravity reported a failed response",
                true,
            ));
        }
        let value = value.get("response").unwrap_or(&value);
        if response_id.is_none() {
            response_id = value
                .get("responseId")
                .or_else(|| value.get("response_id"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }
        if model_version.is_none() {
            model_version = value
                .get("modelVersion")
                .or_else(|| value.get("model_version"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }
        if let Some(value_usage) = value.get("usageMetadata") {
            usage = Some(value_usage.clone());
        }
        let Some(candidate) = value
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|candidates| candidates.first())
        else {
            continue;
        };
        if let Some(reason) = candidate
            .get("finishReason")
            .or_else(|| candidate.get("finish_reason"))
            .and_then(Value::as_str)
            .filter(|reason| !reason.is_empty())
        {
            finish_reason = Some(reason.to_owned());
        }
        if let Some(parts) = candidate
            .get("content")
            .and_then(|content| content.get("parts"))
            .and_then(Value::as_array)
        {
            for part in parts {
                if part.get("thought").and_then(Value::as_bool) == Some(true) {
                    continue;
                }
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    if text_output.len().saturating_add(text.len()) > 16 * 1024 * 1024 {
                        return Err(AdapterSseError::new(
                            "Antigravity response exceeded its size limit",
                            false,
                        ));
                    }
                    text_output.push_str(text);
                }
                if let Some(call) = part.get("functionCall") {
                    if let Some(existing) = tool_calls.iter_mut().find(|existing| {
                        existing.get("name") == call.get("name")
                            && existing.get("id") == call.get("id")
                    }) {
                        if let (Some(existing_args), Some(new_args)) =
                            (existing.get_mut("args"), call.get("args"))
                            && let (Some(existing_args), Some(new_args)) =
                                (existing_args.as_object_mut(), new_args.as_object())
                        {
                            for (key, value) in new_args {
                                existing_args.insert(key.clone(), value.clone());
                            }
                        }
                    } else {
                        tool_calls.push(call.clone());
                    }
                }
            }
        }
    }
    if !saw_event {
        return Err(AdapterSseError::new(
            "Antigravity event stream was empty",
            true,
        ));
    }
    let finish_reason = finish_reason.ok_or_else(|| {
        AdapterSseError::new(
            "Antigravity event stream ended without a completed response",
            true,
        )
    })?;
    let mut parts = Vec::new();
    if !text_output.is_empty() {
        parts.push(json!({"text": text_output}));
    }
    parts.extend(
        tool_calls
            .into_iter()
            .map(|call| json!({"functionCall": call})),
    );
    if parts.is_empty() {
        return Err(AdapterSseError::new(
            "Antigravity completed response has no visible text or tool call",
            true,
        ));
    }
    Ok(json!({
        "responseId": response_id.unwrap_or_else(|| "antigravity-response".to_owned()),
        "modelVersion": model_version.unwrap_or_else(|| "antigravity".to_owned()),
        "candidates": [{
            "content": {"role":"model", "parts": parts},
            "finishReason": finish_reason
        }],
        "usageMetadata": usage.unwrap_or_else(|| json!({}))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_prefix_and_aliases_are_normalized() {
        assert_eq!(
            normalize_model_id("ag/gemini-3.7-flash"),
            "gemini-3.7-flash-tiered"
        );
        assert_eq!(
            normalize_model_id("antigravity/gpt-oss-120b"),
            "gpt-oss-120b-medium"
        );
        assert_eq!(normalize_model_id("claude-sonnet-4-6"), "claude-sonnet-4-6");
    }

    #[test]
    fn tiered_model_alias_sets_native_thinking_level() {
        let mut request = json!({"generationConfig":{"maxOutputTokens":128}});
        apply_tiered_thinking_config(&mut request, "ag/gemini-3.7-flash-low");
        assert_eq!(
            request["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "low"
        );
        assert_eq!(
            request["generationConfig"]["thinkingConfig"]["includeThoughts"],
            false
        );
    }

    #[test]
    fn google_sse_is_accumulated_into_a_terminal_response() {
        let bytes = b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hel\"}]}}]}\n\ndata: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"lo\"}]},\"finishReason\":\"STOP\"}]}\n\n";
        let response = parse_antigravity_event_stream(bytes).expect("Google SSE response");
        assert_eq!(
            response["candidates"][0]["content"]["parts"][0]["text"],
            "hello"
        );
        assert_eq!(response["candidates"][0]["finishReason"], "STOP");
    }

    #[test]
    fn google_sse_rejects_missing_terminal_event() {
        let bytes =
            b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hello\"}]}}]}\n\n";
        assert!(parse_antigravity_event_stream(bytes).is_err());
    }

    #[test]
    fn google_sse_rejects_thought_only_terminal_response_without_http_status() {
        let bytes = br#"data: {"candidates":[{"content":{"parts":[{"text":"internal","thought":true}]},"finishReason":"STOP"}]}

"#;
        let error = parse_antigravity_event_stream(bytes).expect_err("visible content required");
        assert_eq!(
            error.message,
            "Antigravity completed response has no visible text or tool call"
        );
    }

    #[tokio::test]
    async fn wraps_google_request_with_cloud_code_envelope() {
        let database = crate::support::test_support::TestDatabase::open().await;
        let state = AppState::new(database.config(), database.db.clone());
        let auth = OAuthRequestAuth {
            access_token: "oauth-secret".to_owned(),
            account_id: None,
            project_id: Some("cloud-project".to_owned()),
        };
        let mut body = json!({
            "contents": [{"role":"user","parts":[{"text":"hello"}] }]
        });
        let preparation = ANTIGRAVITY_ADAPTER
            .prepare_upstream_request(
                AdapterRequestContext {
                    state: &state,
                    base_url: antigravity_oauth::RUNTIME_BASE_URL,
                    adapter_base_url_override: None,
                    model: "ag/gemini-3.7-flash",
                    request_id: "request-1",
                    streaming: true,
                    auth: UpstreamAuthContext {
                        auth_type: "antigravity_oauth",
                        auth_header: None,
                        secret: None,
                        oauth_auth: Some(&auth),
                        session_id: "session-1",
                        protocol: UpstreamProtocol::GoogleGenerateContent,
                    },
                },
                &mut body,
            )
            .await
            .expect("Cloud Code request envelope");

        assert!(preparation.headers.is_empty());
        assert_eq!(body["project"], "cloud-project");
        assert_eq!(body["model"], "gemini-3.7-flash-tiered");
        assert_eq!(body["userAgent"], "antigravity");
        assert_eq!(body["requestType"], "agent");
        assert_eq!(body["requestId"], "request-1");
        assert_eq!(body["request"]["sessionId"], "session-1");
        assert_eq!(
            ANTIGRAVITY_ADAPTER
                .upstream_endpoint(
                    antigravity_oauth::RUNTIME_BASE_URL,
                    UpstreamProtocol::GoogleGenerateContent,
                    "gemini-3.7-flash",
                    true,
                    None,
                )
                .expect("Antigravity endpoint")
                .as_str(),
            "https://daily-cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse"
        );
        assert!(ANTIGRAVITY_ADAPTER.is_key_rejection_status(StatusCode::CONFLICT));
        assert!(ANTIGRAVITY_ADAPTER.is_key_rejection_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(!ANTIGRAVITY_ADAPTER.is_key_rejection_status(StatusCode::INTERNAL_SERVER_ERROR));
    }
}
