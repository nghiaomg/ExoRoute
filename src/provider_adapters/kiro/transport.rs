//! Kiro transport: OpenAI body → CodeWhisperer EventStream → OpenAI response.
//!
//! The upstream always answers with the binary AWS EventStream protocol. For
//! streaming clients this module synthesizes `text/event-stream` chunks; for
//! non-streaming clients it aggregates the frames into one `chat.completion`.
//! Frame→chunk mapping follows OmniRoute's `KiroExecutor`.
use super::eventstream::{EventFrame, FrameParser};
use super::transform;
use crate::state::AppState;
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::time::Duration;

const MAX_EVENT_STREAM_BYTES: usize = 32 * 1024 * 1024;
const CONNECT_TIMEOUT_CAP: Duration = Duration::from_secs(10);
const USER_AGENT: &str = "AWS-SDK-JS/3.0.0 kiro-ide/1.0.0";
const EVENT_STREAM_ACCEPT: &str = "application/vnd.amazon.eventstream";
const EVENT_STREAM_TARGET: &str = "AmazonCodeWhispererStreamingService.GenerateAssistantResponse";

pub(super) struct StreamState {
    response_id: String,
    created: u64,
    chunk_index: u64,
    end_detected: bool,
    finish_emitted: bool,
    has_tool_calls: bool,
    tool_call_index: u64,
    seen_tool_ids: std::collections::HashMap<String, u64>,
    total_content_length: u64,
    context_usage_percentage: f64,
    has_context_usage: bool,
    has_metering_event: bool,
    usage: Option<Value>,
}

impl StreamState {
    fn new() -> Self {
        Self {
            response_id: format!("chatcmpl-kiro-{}", uuid::Uuid::new_v4()),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            chunk_index: 0,
            end_detected: false,
            finish_emitted: false,
            has_tool_calls: false,
            tool_call_index: 0,
            seen_tool_ids: std::collections::HashMap::new(),
            total_content_length: 0,
            context_usage_percentage: 0.0,
            has_context_usage: false,
            has_metering_event: false,
            usage: None,
        }
    }
}

/// Sends one Chat Completions request through the Kiro transport.
/// `oauth_auth` carries the refreshed access token plus the packed profile
/// extras (`account_id` JSON) from `resolve_oauth_request_auth`.
pub(crate) async fn send_local_upstream(
    state: &AppState,
    upstream_body: &Value,
    model: &str,
    request_id: &str,
    streaming: bool,
    request_timeout: Duration,
    oauth_auth: Option<&crate::provider_adapters::OAuthRequestAuth>,
) -> Result<reqwest::Response, String> {
    let auth = oauth_auth.ok_or("Kiro requires a connected Kiro account")?;
    let access_token = auth.access_token.trim();
    if access_token.is_empty() {
        return Err("Kiro account has no access token; reconnect the account".to_owned());
    }
    let profile_arn = auth
        .account_id
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|extra| {
            extra
                .get("profile_arn")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|arn| !arn.is_empty());

    let mut body = upstream_body.clone();
    if let Some(arn) = profile_arn {
        body["_kiro_profile_arn"] = json!(arn);
    }
    let payload = transform::build_payload(&body, model)
        .map_err(|error| format!("could not translate the Kiro request: {error}"))?;

    let operational = state.operational_settings().settings;
    let (_url, client) = crate::security::egress::provider_client(
        super::KIRO_CHAT_URL,
        false,
        operational.connect_timeout.min(CONNECT_TIMEOUT_CAP),
        request_timeout,
        true,
        USER_AGENT,
        operational.upstream,
    )
    .await
    .map_err(|error| format!("the Kiro endpoint was rejected: {error}"))?;

    let response = client
        .post(super::KIRO_CHAT_URL)
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::ACCEPT, EVENT_STREAM_ACCEPT)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("X-Amz-Target", EVENT_STREAM_TARGET)
        .header("User-Agent", USER_AGENT)
        .header("X-Amz-User-Agent", "aws-sdk-js/3.0.0 kiro-ide/1.0.0")
        .header("Amz-Sdk-Request", "attempt=1; max=3")
        .header("Amz-Sdk-Invocation-Id", uuid::Uuid::new_v4().to_string())
        .header("x-amzn-bedrock-cache-control", "enable")
        .header("anthropic-beta", "prompt-caching-2024-07-31")
        .header("x-request-id", request_id)
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("could not reach the Kiro upstream: {}", error.without_url()))?;

    let status = response.status();
    if !status.is_success() {
        let bytes = crate::provider_adapters::read_limited_response(response, 256 * 1024)
            .await
            .unwrap_or_default();
        let detail = String::from_utf8_lossy(&bytes);
        let detail: String = detail.chars().take(400).collect();
        return Err(format!(
            "Kiro upstream returned HTTP {}: {}",
            status.as_u16(),
            detail.trim()
        ));
    }

    if streaming {
        stream_response(response, model)
    } else {
        aggregate_response(response, model).await
    }
}

fn sse_response<S>(body: S) -> Result<reqwest::Response, String>
where
    S: futures_util::Stream<Item = Result<Vec<u8>, std::io::Error>> + Send + 'static,
{
    http::Response::builder()
        .status(http::StatusCode::OK)
        .header(http::header::CONTENT_TYPE, "text/event-stream")
        .body(reqwest::Body::wrap_stream(body))
        .map(reqwest::Response::from)
        .map_err(|error| format!("could not build the Kiro response: {error}"))
}

fn json_response(completion: &Value) -> Result<reqwest::Response, String> {
    let bytes = serde_json::to_vec(completion)
        .map_err(|error| format!("could not encode the Kiro completion: {error}"))?;
    http::Response::builder()
        .status(http::StatusCode::OK)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(reqwest::Body::from(bytes))
        .map(reqwest::Response::from)
        .map_err(|error| format!("could not build the Kiro completion: {error}"))
}

fn frame_to_chunks(frame: &EventFrame, state: &mut StreamState, model: &str) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let push = |out: &mut Vec<Vec<u8>>, value: Value| {
        out.push(format!("data: {value}\n\n").into_bytes());
    };
    match frame.event_type.as_str() {
        "assistantResponseEvent" => {
            let content = frame
                .payload
                .as_ref()
                .and_then(|p| p.get("content"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if content.is_empty() {
                return out;
            }
            state.total_content_length += content.len() as u64;
            let delta = if state.chunk_index == 0 {
                json!({ "role": "assistant", "content": content })
            } else {
                json!({ "content": content })
            };
            state.chunk_index += 1;
            push(&mut out, chunk_json(state, model, delta, None));
        }
        "codeEvent" => {
            if let Some(content) = frame
                .payload
                .as_ref()
                .and_then(|p| p.get("content"))
                .and_then(Value::as_str)
                .filter(|c| !c.is_empty())
            {
                state.total_content_length += content.len() as u64;
                state.chunk_index += 1;
                push(
                    &mut out,
                    chunk_json(state, model, json!({ "content": content }), None),
                );
            }
        }
        "toolUseEvent" => {
            let tools = frame.payload.as_ref().map(|payload| {
                if let Some(list) = payload.as_array() {
                    list.clone()
                } else if payload.get("toolUses").is_some() {
                    payload
                        .get("toolUses")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default()
                } else {
                    vec![payload.clone()]
                }
            });
            if let Some(tools) = tools {
                state.has_tool_calls = true;
                for tool in tools {
                    let tool_use_id = tool
                        .get("toolUseId")
                        .or_else(|| tool.get("tool_use_id"))
                        .or_else(|| tool.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4()));
                    let name = tool
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    let is_new = !state.seen_tool_ids.contains_key(&tool_use_id);
                    let tool_index = if is_new {
                        let index = state.tool_call_index;
                        state.tool_call_index += 1;
                        state.seen_tool_ids.insert(tool_use_id.clone(), index);
                        let mut delta = json!({
                            "tool_calls": [{
                                "index": index,
                                "id": tool_use_id,
                                "type": "function",
                                "function": { "name": name, "arguments": "" },
                            }]
                        });
                        if state.chunk_index == 0 {
                            delta["tool_calls"][0]["role"] = json!("assistant");
                        }
                        state.chunk_index += 1;
                        push(&mut out, chunk_json(state, model, delta, None));
                        index
                    } else {
                        *state.seen_tool_ids.get(&tool_use_id).unwrap_or(&0)
                    };
                    let _ = tool_index;
                    if let Some(input) = tool.get("input") {
                        let arguments = match input {
                            Value::String(text) => text.clone(),
                            other => serde_json::to_string(other).unwrap_or_default(),
                        };
                        if !arguments.is_empty() {
                            state.chunk_index += 1;
                            push(
                                &mut out,
                                chunk_json(
                                    state,
                                    model,
                                    json!({
                                        "tool_calls": [{
                                            "index": *state.seen_tool_ids.get(&tool_use_id).unwrap_or(&0),
                                            "function": { "arguments": arguments },
                                        }]
                                    }),
                                    None,
                                ),
                            );
                        }
                    }
                }
            }
        }
        "messageStopEvent" => {
            state.end_detected = true;
            if !state.finish_emitted {
                state.finish_emitted = true;
                let reason = if state.has_tool_calls {
                    "tool_calls"
                } else {
                    "stop"
                };
                push(&mut out, chunk_json(state, model, json!({}), Some(reason)));
            }
        }
        "contextUsageEvent" => {
            if let Some(percent) = frame
                .payload
                .as_ref()
                .and_then(|p| p.get("contextUsagePercentage"))
                .and_then(Value::as_f64)
                .filter(|percent| *percent > 0.0)
            {
                state.context_usage_percentage = percent;
                state.has_context_usage = true;
            }
        }
        "meteringEvent" => {
            state.has_metering_event = true;
        }
        "metricsEvent" => {
            let metrics = frame
                .payload
                .as_ref()
                .map(|p| p.get("metricsEvent").cloned().unwrap_or_else(|| p.clone()))
                .unwrap_or(Value::Null);
            let input = metrics
                .get("inputTokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let output = metrics
                .get("outputTokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if input > 0 || output > 0 {
                let mut usage = json!({
                    "prompt_tokens": input,
                    "completion_tokens": output,
                    "total_tokens": input + output,
                });
                let cache_read = metrics
                    .get("cacheReadTokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let cache_write = metrics
                    .get("cacheCreationTokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if cache_read > 0 {
                    usage["cache_read_input_tokens"] = json!(cache_read);
                }
                if cache_write > 0 {
                    usage["cache_creation_input_tokens"] = json!(cache_write);
                }
                state.usage = Some(usage);
            }
        }
        _ => {}
    }
    // Emit the usage-carrying finish chunk once both terminal events arrived.
    if state.has_metering_event && state.has_context_usage && !state.finish_emitted {
        state.finish_emitted = true;
        let mut chunk = chunk_json(
            state,
            model,
            json!({}),
            Some(if state.has_tool_calls {
                "tool_calls"
            } else {
                "stop"
            }),
        );
        if let Some(usage) = state.usage.clone() {
            chunk["usage"] = usage;
        }
        push(&mut out, chunk);
    }
    out
}

fn chunk_json(
    state: &StreamState,
    model: &str,
    delta: Value,
    finish_reason: Option<&str>,
) -> Value {
    let mut choice = json!({ "index": 0, "delta": delta });
    choice["finish_reason"] = finish_reason
        .map(|reason| json!(reason))
        .unwrap_or(Value::Null);
    json!({
        "id": state.response_id,
        "object": "chat.completion.chunk",
        "created": state.created,
        "model": model,
        "choices": [choice],
    })
}

fn stream_response(response: reqwest::Response, model: &str) -> Result<reqwest::Response, String> {
    let model = model.to_owned();
    let stream = async_stream::stream! {
        let mut byte_stream = response.bytes_stream();
        let mut parser = FrameParser::new();
        let mut state = StreamState::new();
        let mut total = 0usize;
        while let Some(chunk) = byte_stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) => break,
            };
            total += chunk.len();
            if total > MAX_EVENT_STREAM_BYTES {
                break;
            }
            parser.push(&chunk);
            for frame in parser.drain() {
                for bytes in frame_to_chunks(&frame, &mut state, &model) {
                    yield Ok(bytes);
                }
            }
        }
        if !state.finish_emitted {
            yield Ok(format!(
                "data: {}\n\n",
                chunk_json(&state, &model, json!({}), Some(if state.has_tool_calls { "tool_calls" } else { "stop" }))
            )
            .into_bytes());
        }
        yield Ok(b"data: [DONE]\n\n".to_vec());
    };
    sse_response(stream)
}

async fn aggregate_response(
    response: reqwest::Response,
    model: &str,
) -> Result<reqwest::Response, String> {
    let bytes = crate::provider_adapters::read_limited_response(response, MAX_EVENT_STREAM_BYTES)
        .await
        .map_err(|error| format!("Kiro upstream stream failed: {error}"))?;
    let mut parser = FrameParser::new();
    parser.push(&bytes);
    let mut state = StreamState::new();
    let mut content = String::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    for frame in parser.drain() {
        match frame.event_type.as_str() {
            "assistantResponseEvent" | "codeEvent" => {
                if let Some(text) = frame
                    .payload
                    .as_ref()
                    .and_then(|p| p.get("content"))
                    .and_then(Value::as_str)
                {
                    content.push_str(text);
                }
            }
            "toolUseEvent" => {
                collect_tool_calls(frame.payload.as_ref(), &mut tool_calls);
            }
            "metricsEvent" => {
                let metrics = frame
                    .payload
                    .as_ref()
                    .map(|p| p.get("metricsEvent").cloned().unwrap_or_else(|| p.clone()))
                    .unwrap_or(Value::Null);
                let input = metrics
                    .get("inputTokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let output = metrics
                    .get("outputTokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if input > 0 || output > 0 {
                    state.usage = Some(json!({
                        "prompt_tokens": input,
                        "completion_tokens": output,
                        "total_tokens": input + output,
                    }));
                }
            }
            _ => {}
        }
    }
    let mut message = json!({ "role": "assistant", "content": content });
    if !tool_calls.is_empty() {
        message["tool_calls"] = json!(tool_calls);
    }
    let mut completion = json!({
        "id": state.response_id,
        "object": "chat.completion",
        "created": state.created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": if tool_calls.is_empty() { "stop" } else { "tool_calls" },
        }],
    });
    if let Some(usage) = state.usage {
        completion["usage"] = usage;
    }
    json_response(&completion)
}

fn collect_tool_calls(payload: Option<&Value>, out: &mut Vec<Value>) {
    let Some(payload) = payload else { return };
    let tools: Vec<Value> = if let Some(list) = payload.as_array() {
        list.clone()
    } else if let Some(list) = payload.get("toolUses").and_then(Value::as_array) {
        list.clone()
    } else {
        vec![payload.clone()]
    };
    for (index, tool) in tools.iter().enumerate() {
        let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
        let id = tool
            .get("toolUseId")
            .or_else(|| tool.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("call_{index}"));
        let input = tool.get("input").cloned().unwrap_or_else(|| json!({}));
        let arguments = match input {
            Value::String(text) => text,
            other => serde_json::to_string(&other).unwrap_or_default(),
        };
        out.push(json!({
            "id": id,
            "type": "function",
            "function": { "name": name, "arguments": arguments },
        }));
    }
}
