use super::{ProviderPreset, UpstreamAuthContext};
use crate::protocol::UpstreamProtocol;
use http::HeaderMap;
use serde_json::{Value, json};
pub(super) fn validate_opencode_config(
    preset: &ProviderPreset,
    auth_type: &str,
    preferred_protocol: &str,
    supported_protocols: &[String],
    zen: bool,
) -> Result<(), &'static str> {
    if !preset.supported_auth_types.contains(&auth_type) || auth_type != "bearer" {
        return Err("OpenCode Go and Zen require Bearer API-key authentication");
    }
    if supported_protocols.is_empty()
        || !supported_protocols
            .iter()
            .any(|protocol| protocol == preferred_protocol)
        || supported_protocols.iter().any(|protocol| {
            !(matches!(
                protocol.as_str(),
                "chat_completions" | "responses" | "messages"
            ) || zen && protocol == "google_generate_content")
        })
        || (!zen
            && supported_protocols
                .iter()
                .any(|p| p == "google_generate_content"))
    {
        return Err("OpenCode provider has an unsupported upstream protocol configuration");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReasoningContentRequirement {
    EveryAssistantTurn,
    ToolCallAssistantTurns,
}

fn opencode_go_reasoning_content_requirement(model: &str) -> Option<ReasoningContentRequirement> {
    let model = model.to_ascii_lowercase();
    if model.contains("deepseek")
        || model.contains("mimo")
        || model.contains("minimax")
        || model.contains("big-pickle")
        || model.contains("qwq")
        || (model.contains("qwen") && model.contains("think"))
        || (model.contains("glm") && model.contains("think"))
    {
        Some(ReasoningContentRequirement::EveryAssistantTurn)
    } else if model.contains("kimi-k") || model.contains("kimi/k") {
        Some(ReasoningContentRequirement::ToolCallAssistantTurns)
    } else {
        None
    }
}

pub(super) fn inject_opencode_go_reasoning_content(model: &str, body: &mut Value) {
    let Some(requirement) = opencode_go_reasoning_content_requirement(model) else {
        return;
    };
    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let has_reasoning_content = message
            .get("reasoning_content")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty());
        if has_reasoning_content {
            continue;
        }
        let has_tool_calls = message
            .get("tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|calls| !calls.is_empty());
        if requirement == ReasoningContentRequirement::EveryAssistantTurn || has_tool_calls {
            message["reasoning_content"] = json!(" ");
        }
    }
}

pub(super) fn apply_opencode_session_header(
    request: reqwest::RequestBuilder,
    headers: &HeaderMap,
) -> reqwest::RequestBuilder {
    let Some(session) = headers
        .get("x-opencode-session")
        .and_then(|value| value.to_str().ok())
    else {
        return request;
    };
    apply_opencode_session_value(request, session)
}

fn apply_opencode_session_value(
    request: reqwest::RequestBuilder,
    session: &str,
) -> reqwest::RequestBuilder {
    let Some(session) = valid_opencode_session_value(session) else {
        return request;
    };
    request.header("x-opencode-session", session)
}

fn valid_opencode_session_value(value: &str) -> Option<&str> {
    let session = value.trim();
    if session.is_empty()
        || session.len() > 128
        || !session
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return None;
    }
    Some(session)
}

pub(super) fn apply_opencode_auth(
    mut request: reqwest::RequestBuilder,
    auth: UpstreamAuthContext<'_>,
    zen: bool,
) -> Result<reqwest::RequestBuilder, String> {
    if auth.oauth_auth.is_some() {
        return Err(
            "OpenCode providers use API keys and do not support OAuth credentials".to_owned(),
        );
    }
    let Some(secret) = auth.secret else {
        return Err("OpenCode provider API key is missing".to_owned());
    };
    if auth.auth_type != "bearer" {
        return Err("OpenCode provider requires Bearer API-key authentication".to_owned());
    }
    if !zen {
        let session = valid_opencode_session_value(auth.session_id)
            .ok_or("OpenCode Go provider session ID is invalid")?;
        request = request.header("x-opencode-session", session);
    }
    match auth.protocol {
        UpstreamProtocol::Messages => {
            request = request.header("x-api-key", secret);
        }
        UpstreamProtocol::Responses if zen => {
            request = request.header("x-api-key", secret);
        }
        UpstreamProtocol::ChatCompletions
        | UpstreamProtocol::Responses
        | UpstreamProtocol::GoogleGenerateContent => {
            request = request.bearer_auth(secret);
        }
    }
    Ok(request)
}
