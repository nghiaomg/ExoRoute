//! HTTP exchange and response synthesis for the Devin Desktop transport.
//!
//! The Connect-protobuf framing lives in `connect.rs`; this module owns the
//! egress-validated client, the GetUserJwt preflight, the chat exchange, and
//! the OpenAI-shaped response synthesis the gateway pipeline consumes.

use super::connect;
use super::*;
use crate::security::egress;
use std::time::Duration;

// --- Transport --------------------------------------------------------------

/// Runs the unary GetUserJwt preflight and returns when the key is accepted.
pub(crate) async fn auth_preflight(state: &AppState, api_key: &str) -> Result<(), String> {
    let operational = state.operational_settings().settings;
    let connect_timeout = operational.connect_timeout.min(CONNECT_TIMEOUT_CAP);
    let (_url, client) = egress::provider_client(
        connect::AUTH_URL,
        false,
        connect_timeout,
        AUTH_TIMEOUT,
        false,
        user_agent().as_str(),
        operational.upstream,
    )
    .await
    .map_err(|error| format!("the Devin Desktop endpoint was rejected: {error}"))?;
    let session_id = uuid::Uuid::new_v4().to_string();
    let request_bytes = connect::encode_auth_request(&connect::MetadataRefs {
        api_key,
        session_id: &session_id,
        user_jwt: "",
    });
    let response = client
        .post(connect::AUTH_URL)
        .header("Content-Type", "application/proto")
        .header("Accept", "*/*")
        .header("Connect-Protocol-Version", "1")
        .body(request_bytes)
        .timeout(AUTH_TIMEOUT)
        .send()
        .await
        .map_err(|error| format!("the Devin Desktop authentication request failed: {error}"))?;
    let status = response.status();
    let bytes =
        crate::provider_adapters::read_limited_response(response, connect::MAX_AUTH_RESPONSE_BYTES)
            .await
            .map_err(|error| {
                format!("the Devin Desktop authentication response was invalid: {error}")
            })?;
    if !status.is_success() {
        return Err(format!(
            "the Devin Desktop authentication returned HTTP {}",
            status.as_u16()
        ));
    }
    connect::decode_auth_response(&bytes).map(|_| ())
}

fn user_agent() -> String {
    format!("windsurf/{}", connect::resolve_ide_version())
}

struct ExchangeOutput {
    text: String,
    thinking: String,
    tool_calls: Vec<connect::ToolCallDelta>,
    stop_reason: u64,
    usage: Option<connect::Usage>,
    trailer_error: Option<(String, String)>,
}

/// Drains the Connect stream, decoding every frame into one aggregate.
async fn drain_exchange(response: reqwest::Response) -> Result<ExchangeOutput, String> {
    use futures_util::StreamExt;
    let mut parser = connect::FrameParser::new();
    let mut out = ExchangeOutput {
        text: String::new(),
        thinking: String::new(),
        tool_calls: Vec::new(),
        stop_reason: 0,
        usage: None,
        trailer_error: None,
    };
    let mut stream = response.bytes_stream();
    let mut total_bytes = 0usize;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("the Devin Desktop stream failed: {error}"))?;
        total_bytes += chunk.len();
        if total_bytes > connect::MAX_RESPONSE_BYTES {
            return Err("the Devin Desktop response exceeds the safety limit".to_owned());
        }
        for frame in parser.push(&chunk)? {
            let payload = if frame.flags & connect::CONNECT_COMPRESSED_FLAG != 0 {
                connect::decode_gzip(&frame.payload, connect::MAX_FRAME_BYTES)?
            } else {
                frame.payload
            };
            if frame.flags & connect::CONNECT_END_STREAM_FLAG != 0 {
                out.trailer_error = connect::parse_trailer(&payload)?;
                if parser.has_pending() {
                    return Err(
                        "data follows the Devin Desktop Connect end-stream frame".to_owned()
                    );
                }
                return Ok(out);
            }
            let decoded = connect::decode_chat_response(&payload)?;
            out.text.push_str(&decoded.text);
            out.thinking.push_str(&decoded.thinking);
            out.stop_reason = if decoded.stop_reason != 0 {
                decoded.stop_reason
            } else {
                out.stop_reason
            };
            if decoded.usage.is_some() {
                out.usage = decoded.usage;
            }
            out.tool_calls.extend(decoded.tool_calls);
        }
    }
    if parser.has_pending() {
        return Err("truncated Devin Desktop Connect frame".to_owned());
    }
    Err("the Devin Desktop Connect stream ended without trailers".to_owned())
}

fn completion_json(
    output: &ExchangeOutput,
    completion_id: &str,
    model: &str,
    created: i64,
) -> Value {
    let has_tool_calls = !output.tool_calls.is_empty();
    let mut message = json!({
        "role": "assistant",
        "content": output.text,
    });
    if !output.thinking.is_empty() {
        message["reasoning_content"] = Value::String(output.thinking.clone());
    }
    if has_tool_calls {
        // Merge argument fragments per call id, preserving first-seen order.
        let mut merged: Vec<(String, String, String)> = Vec::new();
        for call in &output.tool_calls {
            if let Some(existing) = merged.iter_mut().find(|(id, _, _)| id == &call.id) {
                existing.2.push_str(&call.arguments);
            } else {
                merged.push((call.id.clone(), call.name.clone(), call.arguments.clone()));
            }
        }
        message["tool_calls"] = Value::Array(
            merged
                .into_iter()
                .map(|(id, name, arguments)| {
                    json!({
                        "id": id,
                        "type": "function",
                        "function": {"name": name, "arguments": arguments},
                    })
                })
                .collect(),
        );
    }
    let mut completion = json!({
        "id": completion_id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": connect::finish_reason(output.stop_reason, has_tool_calls),
        }],
    });
    if let Some(usage) = &output.usage {
        completion["usage"] = json!({
            "prompt_tokens": usage.input_tokens,
            "completion_tokens": usage.output_tokens,
            "total_tokens": usage.input_tokens + usage.output_tokens,
            "prompt_tokens_details": {"cached_tokens": usage.cache_read_tokens},
            "cache_write_tokens": usage.cache_write_tokens,
        });
    }
    completion
}

/// Sends one Chat Completions request through the Devin Desktop transport.
/// Streaming clients get a synthesized `text/event-stream`; non-streaming
/// clients get a buffered `chat.completion`.
/// Sends one Chat Completions request through the Devin Desktop transport.
/// Streaming clients get a synthesized `text/event-stream`; non-streaming
/// clients get a buffered `chat.completion`.
pub(crate) async fn send_local_upstream(
    state: &AppState,
    upstream_body: &Value,
    model: &str,
    request_id: &str,
    streaming: bool,
    request_timeout: Duration,
    credential_secret: Option<&str>,
) -> Result<reqwest::Response, String> {
    let api_key = credential_secret
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .ok_or("the Devin Desktop transport requires an API key credential")?;
    let operational = state.operational_settings().settings;
    let connect_timeout = operational.connect_timeout.min(CONNECT_TIMEOUT_CAP);
    let (_url, client) = egress::provider_client(
        connect::CHAT_URL,
        false,
        connect_timeout,
        request_timeout,
        true,
        user_agent().as_str(),
        operational.upstream,
    )
    .await
    .map_err(|error| format!("the Devin Desktop endpoint was rejected: {error}"))?;
    run_exchange(
        &client,
        connect::AUTH_URL,
        connect::CHAT_URL,
        api_key,
        model,
        upstream_body,
        request_id,
        streaming,
        request_timeout,
    )
    .await
}

/// The full auth-preflight-then-chat exchange against explicit endpoints.
/// Tests inject a local client and URLs; production uses the egress-validated
/// client and the canonical `server.codeium.com` endpoints.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_exchange(
    client: &reqwest::Client,
    auth_url: &str,
    chat_url: &str,
    api_key: &str,
    model: &str,
    upstream_body: &Value,
    request_id: &str,
    streaming: bool,
    request_timeout: Duration,
) -> Result<reqwest::Response, String> {
    let (system_prompt, owned_prompts) = convert_messages(upstream_body)?;
    let owned_tools = convert_tools(upstream_body)?;
    let tool_choice = convert_tool_choice(upstream_body)?;
    let session_id = uuid::Uuid::new_v4().to_string();
    let cascade_id = upstream_body
        .get("conversation_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // History tool calls ride on their prompt; build per-prompt references.
    let prompt_tool_calls: Vec<Vec<connect::ToolCallRef<'_>>> = owned_prompts
        .iter()
        .map(|prompt| {
            prompt
                .tool_calls
                .iter()
                .map(|(id, name, arguments)| connect::ToolCallRef {
                    id,
                    name,
                    arguments,
                })
                .collect()
        })
        .collect();
    let prompts: Vec<connect::PromptRef<'_>> = owned_prompts
        .iter()
        .zip(&prompt_tool_calls)
        .map(|(prompt, calls)| connect::PromptRef {
            message_id: &prompt.message_id,
            source: prompt.source,
            prompt: &prompt.prompt,
            tool_calls: calls,
            tool_call_id: &prompt.tool_call_id,
        })
        .collect();
    let tools: Vec<connect::ToolDefRef<'_>> = owned_tools
        .iter()
        .map(|tool| connect::ToolDefRef {
            name: &tool.name,
            description: &tool.description,
            schema_json: &tool.schema_json,
            strict: tool.strict,
        })
        .collect();

    // Auth preflight through the same client; the JWT is scoped to this
    // exchange and never logged.
    let auth_session = uuid::Uuid::new_v4().to_string();
    let auth_bytes = connect::encode_auth_request(&connect::MetadataRefs {
        api_key,
        session_id: &auth_session,
        user_jwt: "",
    });
    let auth_response = client
        .post(auth_url)
        .header("Content-Type", "application/proto")
        .header("Accept", "*/*")
        .header("Connect-Protocol-Version", "1")
        .body(auth_bytes)
        .timeout(AUTH_TIMEOUT)
        .send()
        .await
        .map_err(|error| format!("the Devin Desktop authentication request failed: {error}"))?;
    let auth_status = auth_response.status();
    let auth_payload = crate::provider_adapters::read_limited_response(
        auth_response,
        connect::MAX_AUTH_RESPONSE_BYTES,
    )
    .await
    .map_err(|error| format!("the Devin Desktop authentication response was invalid: {error}"))?;
    if !auth_status.is_success() {
        return Err(format!(
            "the Devin Desktop authentication returned HTTP {}",
            auth_status.as_u16()
        ));
    }
    let user_jwt = connect::decode_auth_response(&auth_payload)?;

    let request_bytes = connect::encode_chat_request(&connect::ChatRequestRefs {
        metadata: connect::MetadataRefs {
            api_key,
            session_id: &session_id,
            user_jwt: &user_jwt,
        },
        model,
        system_prompt: &system_prompt,
        prompts: &prompts,
        cascade_id: &cascade_id,
        tools: &tools,
        disable_parallel_tool_calls: upstream_body.get("parallel_tool_calls")
            == Some(&Value::Bool(false)),
        tool_choice,
    });
    let framed = connect::encode_frame(&request_bytes, 0);
    let response = client
        .post(chat_url)
        .header("Content-Type", "application/connect+proto")
        .header("Accept", "application/connect+proto")
        .header("Connect-Protocol-Version", "1")
        .header("Connect-Accept-Encoding", "gzip")
        .header("User-Agent", user_agent())
        .body(framed)
        .timeout(request_timeout)
        .send()
        .await
        .map_err(|error| format!("the Devin Desktop chat request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "the Devin Desktop chat request returned HTTP {}",
            status.as_u16()
        ));
    }

    let completion_id = format!("chatcmpl-devin-desktop-{request_id}");
    let created = connect::unix_timestamp();
    if streaming {
        let body = streaming_response(response, completion_id, model.to_owned(), created);
        sse_response(body)
    } else {
        let output = drain_exchange(response).await?;
        if let Some((code, message)) = &output.trailer_error {
            return Err(format!(
                "the Devin Desktop stream failed: {code}: {message}"
            ));
        }
        let completion = completion_json(&output, &completion_id, model, created);
        json_response(&completion)
    }
}

// --- Synthesized response frames ---------------------------------------------

struct StreamState {
    completion_id: String,
    model: String,
    created: i64,
    role_emitted: bool,
    stop_reason: u64,
    tool_indexes: std::collections::HashMap<String, usize>,
    last_tool_index: Option<usize>,
    orphan_arguments: String,
    usage: Option<connect::Usage>,
}

impl StreamState {
    fn chunk(&self, delta: Value, finish_reason: Option<&str>, usage: Option<Value>) -> Value {
        let mut chunk = json!({
            "id": self.completion_id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [{
                "index": 0,
                "delta": delta,
                "finish_reason": finish_reason.map(Value::from).unwrap_or(Value::Null),
            }],
        });
        if let Some(usage) = usage {
            chunk["usage"] = usage;
        }
        chunk
    }

    fn usage_json(&self) -> Option<Value> {
        self.usage.as_ref().map(|usage| {
            json!({
                "prompt_tokens": usage.input_tokens,
                "completion_tokens": usage.output_tokens,
                "total_tokens": usage.input_tokens + usage.output_tokens,
                "prompt_tokens_details": {"cached_tokens": usage.cache_read_tokens},
                "cache_write_tokens": usage.cache_write_tokens,
            })
        })
    }

    /// Emits tool-call deltas with stable indexes, holding id-less argument
    /// fragments until the call they continue is known.
    fn emit_tool_calls(&mut self, calls: &[connect::ToolCallDelta], chunks: &mut Vec<Value>) {
        for call in calls {
            if call.id.is_empty() && self.last_tool_index.is_none() {
                self.orphan_arguments.push_str(&call.arguments);
                continue;
            }
            let existing_index = if call.id.is_empty() {
                self.last_tool_index
            } else {
                self.tool_indexes.get(&call.id).copied()
            };
            let index = existing_index.unwrap_or(self.tool_indexes.len());
            let first_delta = !call.id.is_empty() && existing_index.is_none();
            if first_delta {
                self.tool_indexes.insert(call.id.clone(), index);
            }
            self.last_tool_index = Some(index);
            let mut function = serde_json::Map::new();
            if !call.name.is_empty() {
                function.insert("name".to_owned(), Value::String(call.name.clone()));
            }
            let arguments = if first_delta {
                let combined = format!("{}{}", self.orphan_arguments, call.arguments);
                self.orphan_arguments.clear();
                combined
            } else {
                call.arguments.clone()
            };
            if !arguments.is_empty() {
                function.insert("arguments".to_owned(), Value::String(arguments));
            }
            let mut delta = serde_json::Map::new();
            let mut call_json = serde_json::Map::new();
            call_json.insert("index".to_owned(), Value::from(index as u64));
            if first_delta {
                call_json.insert("id".to_owned(), Value::String(call.id.clone()));
                call_json.insert("type".to_owned(), Value::String("function".to_owned()));
            }
            call_json.insert("function".to_owned(), Value::Object(function));
            delta.insert(
                "tool_calls".to_owned(),
                Value::Array(vec![Value::Object(call_json)]),
            );
            chunks.push(self.chunk(Value::Object(delta), None, None));
        }
    }
}

fn streaming_response(
    response: reqwest::Response,
    completion_id: String,
    model: String,
    created: i64,
) -> impl futures_util::Stream<Item = Result<Vec<u8>, std::io::Error>> + Send + 'static {
    use futures_util::StreamExt;
    async_stream::stream! {
        let mut state = StreamState {
            completion_id,
            model,
            created,
            role_emitted: false,
            stop_reason: 0,
            tool_indexes: std::collections::HashMap::new(),
            last_tool_index: None,
            orphan_arguments: String::new(),
            usage: None,
        };
        let mut parser = connect::FrameParser::new();
        let mut stream = response.bytes_stream();
        let mut total_bytes = 0usize;
        let failure = |message: String| -> Vec<u8> {
            let error = json!({
                "error": {
                    "message": message,
                    "type": "devin_desktop_error",
                    "code": "upstream_error",
                }
            });
            let mut bytes = format!("data: {error}\n\n").into_bytes();
            bytes.extend_from_slice(b"data: [DONE]\n\n");
            bytes
        };
        loop {
            // Read the next network chunk, mapping transport failures in-band.
            let chunk = match stream.next().await {
                Some(Ok(chunk)) => chunk,
                Some(Err(error)) => {
                    yield Ok(failure(format!("the Devin Desktop stream failed: {error}")));
                    return;
                }
                None => break,
            };
            total_bytes += chunk.len();
            if total_bytes > connect::MAX_RESPONSE_BYTES {
                yield Ok(failure(
                    "the Devin Desktop response exceeds the safety limit".to_owned(),
                ));
                return;
            }
            let frames = match parser.push(&chunk) {
                Ok(frames) => frames,
                Err(message) => {
                    yield Ok(failure(message));
                    return;
                }
            };
            for frame in frames {
                let payload = if frame.flags & connect::CONNECT_COMPRESSED_FLAG != 0 {
                    match connect::decode_gzip(&frame.payload, connect::MAX_FRAME_BYTES) {
                        Ok(payload) => payload,
                        Err(message) => {
                            yield Ok(failure(message));
                            return;
                        }
                    }
                } else {
                    frame.payload
                };
                if frame.flags & connect::CONNECT_END_STREAM_FLAG != 0 {
                    match connect::parse_trailer(&payload) {
                        Ok(Some((code, message))) => {
                            yield Ok(failure(format!(
                                "the Devin Desktop stream failed: {code}: {message}"
                            )));
                            return;
                        }
                        Ok(None) => {
                            let finish = connect::finish_reason(
                                state.stop_reason,
                                !state.tool_indexes.is_empty(),
                            );
                            let usage = state.usage_json();
                            let final_chunk = state.chunk(json!({}), Some(finish), usage);
                            yield Ok(sse_frame(&final_chunk));
                            yield Ok(b"data: [DONE]\n\n".to_vec());
                            return;
                        }
                        Err(message) => {
                            yield Ok(failure(message));
                            return;
                        }
                    }
                }
                let decoded = match connect::decode_chat_response(&payload) {
                    Ok(decoded) => decoded,
                    Err(message) => {
                        yield Ok(failure(message));
                        return;
                    }
                };
                if decoded.stop_reason != 0 {
                    state.stop_reason = decoded.stop_reason;
                }
                let has_usage = decoded.usage.is_some();
                let has_payload = !decoded.thinking.is_empty()
                    || !decoded.text.is_empty()
                    || !decoded.tool_calls.is_empty();
                if has_usage {
                    state.usage = decoded.usage;
                }
                if has_payload && !state.role_emitted {
                    state.role_emitted = true;
                    yield Ok(sse_frame(&state.chunk(json!({"role": "assistant", "content": ""}), None, None)));
                }
                if !decoded.thinking.is_empty() {
                    yield Ok(sse_frame(&state.chunk(json!({"reasoning_content": decoded.thinking}), None, None)));
                }
                if !decoded.text.is_empty() {
                    yield Ok(sse_frame(&state.chunk(json!({"content": decoded.text}), None, None)));
                }
                if !decoded.tool_calls.is_empty() {
                    let mut chunks = Vec::new();
                    state.emit_tool_calls(&decoded.tool_calls, &mut chunks);
                    for chunk in chunks {
                        yield Ok(sse_frame(&chunk));
                    }
                }
            }
        }
        if parser.has_pending() {
            yield Ok(failure("truncated Devin Desktop Connect frame".to_owned()));
        } else {
            yield Ok(failure(
                "the Devin Desktop Connect stream ended without trailers".to_owned(),
            ));
        }
    }
}

fn sse_frame(value: &Value) -> Vec<u8> {
    format!("data: {value}\n\n").into_bytes()
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
        .map_err(|error| format!("could not build the Devin Desktop response: {error}"))
}

fn json_response(completion: &Value) -> Result<reqwest::Response, String> {
    let bytes = serde_json::to_vec(completion)
        .map_err(|error| format!("could not encode the Devin Desktop completion: {error}"))?;
    http::Response::builder()
        .status(http::StatusCode::OK)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(reqwest::Body::from(bytes))
        .map(reqwest::Response::from)
        .map_err(|error| format!("could not build the Devin Desktop completion: {error}"))
}
