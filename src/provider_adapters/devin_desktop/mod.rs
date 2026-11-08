//! Devin Desktop (Windsurf backend) adapter — EXPERIMENTAL.
//!
//! Talks the reverse-engineered Connect-protobuf protocol of
//! `server.codeium.com`: a unary `GetUserJwt` preflight puts the raw API key
//! inside the protobuf Metadata (never an Authorization header), then the chat
//! exchange streams `GetChatMessage` frames. The protocol can break whenever
//! Cognition changes their backend; the preset ships labeled experimental.
//! Responses are translated into OpenAI-shaped SSE or JSON so the gateway's
//! classify/stream pipeline stays transport-agnostic.

use super::*;
use std::time::Duration;

pub(crate) use transport::{auth_preflight, send_local_upstream};

mod connect;
#[cfg(test)]
mod tests;
mod transport;

pub(super) struct DevinDesktopAdapter;

pub(super) static DEVIN_DESKTOP_ADAPTER: DevinDesktopAdapter = DevinDesktopAdapter;

const AUTH_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT_CAP: Duration = Duration::from_secs(10);
const MAX_BODY_TEXT_CHARS: usize = 4_000_000;

impl ProviderAdapter for DevinDesktopAdapter {
    fn adapter_id(&self) -> &'static str {
        DEVIN_DESKTOP_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err(
                "the Devin Desktop transport only speaks the Chat Completions protocol".to_owned(),
            );
        }
        if base_url.trim() != connect::BASE_URL {
            return Err(
                "Devin Desktop providers route through server.codeium.com; restore the endpoint to the preset default"
                    .to_owned(),
            );
        }
        reqwest::Url::parse(connect::BASE_URL)
            .map_err(|error| format!("invalid Devin Desktop endpoint: {error}"))
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn validate_config(
        &self,
        preset: &ProviderPreset,
        auth_type: &str,
        base_url: &str,
        preferred_protocol: &str,
        supported_protocols: &[String],
        _api_keys_present: bool,
    ) -> Result<(), &'static str> {
        if !preset.supported_auth_types.contains(&auth_type) {
            return Err(
                "the Devin Desktop transport carries its key inside the protobuf Metadata and takes no other auth style",
            );
        }
        if preferred_protocol != "chat_completions"
            || supported_protocols.len() != 1
            || supported_protocols[0] != "chat_completions"
        {
            return Err("the Devin Desktop transport requires the Chat Completions protocol");
        }
        if base_url.trim() != connect::BASE_URL {
            return Err("Devin Desktop providers keep the server.codeium.com endpoint");
        }
        Ok(())
    }

    fn test_api_key_credential<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, AdapterKeyTestOutcome> {
        Box::pin(async move {
            match auth_preflight(request.state, request.credential).await {
                Ok(()) => AdapterKeyTestOutcome {
                    test_passed: true,
                    status: None,
                    message:
                        "Devin Desktop authentication succeeded; the transport is reverse-engineered and may break when Cognition changes their backend."
                            .to_owned(),
                },
                Err(error) => AdapterKeyTestOutcome {
                    test_passed: false,
                    status: None,
                    message: format!("Devin Desktop probe failed: {error}"),
                },
            }
        })
    }

    fn test_api_key_model<'a>(
        &'a self,
        request: AdapterModelTestRequest<'a>,
    ) -> AdapterFuture<'a, AdapterModelTestOutcome> {
        Box::pin(async move {
            let Some(credential) = request.credentials.first().and_then(Option::as_deref) else {
                return AdapterModelTestOutcome {
                    test_passed: false,
                    status: None,
                    message: "Devin Desktop requires an API key for the model probe.".to_owned(),
                    provider_response_body: None,
                };
            };
            let body = model_probe_body(request.model, request.protocol);
            match send_local_upstream(
                request.state,
                &body,
                request.model,
                "model-probe",
                false,
                Duration::from_secs(90),
                Some(credential),
            )
            .await
            {
                Ok(response) => {
                    let text =
                        crate::provider_adapters::read_limited_response(response, 512 * 1024).await;
                    match text {
                        Ok(bytes) => {
                            let parsed: Result<Value, _> = serde_json::from_slice(&bytes);
                            let answered = parsed
                                .ok()
                                .and_then(|value| {
                                    value["choices"][0]["message"]["content"]
                                        .as_str()
                                        .map(str::to_owned)
                                })
                                .is_some_and(|text| !text.trim().is_empty());
                            AdapterModelTestOutcome {
                                test_passed: answered,
                                status: None,
                                message: if answered {
                                    "Devin Desktop answered the test prompt.".to_owned()
                                } else {
                                    "Devin Desktop returned an empty answer for the test prompt."
                                        .to_owned()
                                },
                                provider_response_body: None,
                            }
                        }
                        Err(error) => AdapterModelTestOutcome {
                            test_passed: false,
                            status: None,
                            message: format!("Devin Desktop model probe failed: {error}"),
                            provider_response_body: None,
                        },
                    }
                }
                Err(error) => AdapterModelTestOutcome {
                    test_passed: false,
                    status: None,
                    message: format!("Devin Desktop model probe failed: {error}"),
                    provider_response_body: None,
                },
            }
        })
    }
}

// --- Message conversion -----------------------------------------------------

/// Extracts the text of one Chat Completions message. Text-bearing array parts
/// concatenate; any media part is rejected loudly instead of dropped.
fn message_text(content: &Value) -> Result<String, String> {
    match content {
        Value::String(text) => Ok(text.clone()),
        Value::Array(parts) => {
            let mut out = String::new();
            for part in parts {
                let kind = part.get("type").and_then(Value::as_str).unwrap_or("text");
                if kind == "text" {
                    out.push_str(part.get("text").and_then(Value::as_str).unwrap_or_default());
                } else {
                    return Err(format!(
                        "the Devin Desktop transport does not support '{kind}' message parts"
                    ));
                }
            }
            Ok(out)
        }
        Value::Null => Ok(String::new()),
        _ => Err("unsupported message content shape".to_owned()),
    }
}

fn bounded(text: &str, what: &str) -> Result<String, String> {
    if text.chars().count() > MAX_BODY_TEXT_CHARS {
        return Err(format!(
            "{what} exceeds the {MAX_BODY_TEXT_CHARS}-character transport limit"
        ));
    }
    Ok(text.to_owned())
}

#[derive(Debug)]
struct OwnedPrompt {
    message_id: String,
    source: u64,
    prompt: String,
    tool_calls: Vec<(String, String, String)>,
    tool_call_id: String,
}

#[derive(Debug)]
struct OwnedTool {
    name: String,
    description: String,
    schema_json: String,
    strict: bool,
}

fn convert_messages(body: &Value) -> Result<(String, Vec<OwnedPrompt>), String> {
    let mut system_parts: Vec<String> = Vec::new();
    let mut prompts: Vec<OwnedPrompt> = Vec::new();
    let Some(messages) = body.get("messages").and_then(Value::as_array) else {
        return Err("the request has no messages".to_owned());
    };
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        let text = message_text(message.get("content").unwrap_or(&Value::Null))?;
        match role {
            "system" | "developer" => {
                if !text.is_empty() {
                    system_parts.push(bounded(&text, "the system prompt")?);
                }
            }
            "user" | "assistant" | "tool" => {
                let source = match role {
                    "assistant" => 2,
                    "tool" => 4,
                    _ => 1,
                };
                let mut tool_calls = Vec::new();
                let mut tool_call_id = String::new();
                if source == 2
                    && let Some(calls) = message.get("tool_calls").and_then(Value::as_array)
                {
                    for call in calls {
                        let id = call.get("id").and_then(Value::as_str).unwrap_or_default();
                        let name = call["function"]["name"].as_str().unwrap_or_default();
                        let arguments = call["function"]["arguments"].as_str().unwrap_or_default();
                        if id.is_empty() || name.is_empty() {
                            return Err(
                                "the Devin Desktop transport requires complete tool calls in history"
                                    .to_owned(),
                            );
                        }
                        tool_calls.push((id.to_owned(), name.to_owned(), arguments.to_owned()));
                    }
                }
                if source == 4 {
                    tool_call_id = message
                        .get("tool_call_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                }
                prompts.push(OwnedPrompt {
                    message_id: uuid::Uuid::new_v4().to_string(),
                    source,
                    prompt: bounded(&text, "a message")?,
                    tool_calls,
                    tool_call_id,
                });
            }
            other => {
                return Err(format!(
                    "the Devin Desktop transport does not support the '{other}' role"
                ));
            }
        }
    }
    if prompts.is_empty() {
        prompts.push(OwnedPrompt {
            message_id: uuid::Uuid::new_v4().to_string(),
            source: 1,
            prompt: String::new(),
            tool_calls: Vec::new(),
            tool_call_id: String::new(),
        });
    }
    Ok((system_parts.join("\n\n"), prompts))
}

fn convert_tools(body: &Value) -> Result<Vec<OwnedTool>, String> {
    let Some(tools) = body.get("tools").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut converted = Vec::new();
    for tool in tools {
        if tool.get("type").and_then(Value::as_str) != Some("function") {
            return Err("the Devin Desktop transport only supports function tools".to_owned());
        }
        let function = tool
            .get("function")
            .ok_or("a function tool has no definition")?;
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or("a function tool has no name")?;
        converted.push(OwnedTool {
            name: name.to_owned(),
            description: function
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            schema_json: function
                .get("parameters")
                .map(|parameters| parameters.to_string())
                .unwrap_or_else(|| "{}".to_owned()),
            strict: function.get("strict").and_then(Value::as_bool) == Some(true),
        });
    }
    Ok(converted)
}

fn convert_tool_choice(body: &Value) -> Result<Option<connect::ToolChoiceRef<'static>>, String> {
    let choice = match body.get("tool_choice") {
        None | Some(Value::Null) => return Ok(None),
        Some(choice) => choice,
    };
    if let Some(name) = choice.as_str() {
        if matches!(name, "auto" | "none" | "required") {
            // Leak is bounded to the three fixed option names.
            let owned = match name {
                "auto" => "auto",
                "none" => "none",
                _ => "required",
            };
            return Ok(Some(connect::ToolChoiceRef::OptionName(owned)));
        }
        return Err(
            "the Devin Desktop transport only supports the auto/none/required tool choices by name"
                .to_owned(),
        );
    }
    Err("the Devin Desktop transport does not support named tool choices".to_owned())
}
