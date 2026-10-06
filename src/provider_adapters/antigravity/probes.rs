//! Model-probe request building and SSE parsing for Antigravity.
//! Probe entry points live with the adapter trait impl; this module
//! owns the probe-specific thinking configuration and event decoding.

use super::*;

pub(super) fn normalize_model_id(model: &str) -> String {
    antigravity_oauth::normalize_model_id(model)
}

pub(super) fn model_thinking_level(model: &str) -> Option<&'static str> {
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

pub(super) fn set_tiered_thinking_config(request: &mut Value, level: &str) {
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

pub(super) fn apply_tiered_thinking_config(request: &mut Value, model: &str) {
    let Some(level) = model_thinking_level(model) else {
        return;
    };
    set_tiered_thinking_config(request, level);
}

pub(super) fn apply_model_test_thinking_config(request: &mut Value, model: &str) {
    if !normalize_model_id(model).ends_with("-tiered") {
        return;
    }
    let level = model_thinking_level(model).unwrap_or("low");
    set_tiered_thinking_config(request, level);
}

pub(super) async fn test_antigravity_model(
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
        false,
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

pub(super) fn parse_antigravity_event_stream(bytes: &[u8]) -> Result<Value, AdapterSseError> {
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
