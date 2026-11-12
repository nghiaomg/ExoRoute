use super::*;
use std::time::Duration;

mod acp;
mod prompt;
#[cfg(test)]
mod tests;
mod tool_envelope;

pub(super) struct DevinCliAdapter;

pub(super) static DEVIN_CLI_ADAPTER: DevinCliAdapter = DevinCliAdapter;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
const PROBE_PROMPT: &str = "Reply with the single word: OK";

impl ProviderAdapter for DevinCliAdapter {
    fn adapter_id(&self) -> &'static str {
        DEVIN_CLI_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err(
                "the Devin CLI transport only speaks the Chat Completions protocol".to_owned(),
            );
        }
        if base_url.trim() != acp::DEVIN_ENDPOINT_URL {
            return Err(
                "Devin CLI providers route through the local process transport; restore the endpoint to devin://acp/stdio"
                    .to_owned(),
            );
        }
        reqwest::Url::parse(acp::DEVIN_ENDPOINT_URL)
            .map_err(|error| format!("invalid Devin CLI endpoint: {error}"))
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
                "the Devin CLI transport authenticates through `devin auth login` and takes no API key",
            );
        }
        if preferred_protocol != "chat_completions"
            || supported_protocols.len() != 1
            || supported_protocols[0] != "chat_completions"
        {
            return Err("the Devin CLI transport requires the Chat Completions protocol");
        }
        if base_url.trim() != acp::DEVIN_ENDPOINT_URL {
            return Err("Devin CLI providers keep the devin://acp/stdio endpoint");
        }
        Ok(())
    }

    fn test_api_key_credential<'a>(
        &'a self,
        _request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, AdapterKeyTestOutcome> {
        Box::pin(async move {
            // Credentials are decorative here (auth is `none`); the probe
            // validates that the local CLI exists, is signed in, and speaks
            // the ACP handshake.
            match acp::handshake_probe(HANDSHAKE_TIMEOUT).await {
                Ok(()) => AdapterKeyTestOutcome {
                    test_passed: true,
                    status: None,
                    message: "Devin CLI session started; requests run locally via `devin acp` with no file or shell tools.".to_owned(),
                },
                Err(error) => AdapterKeyTestOutcome {
                    test_passed: false,
                    status: None,
                    message: format!("Devin CLI probe failed: {error}"),
                },
            }
        })
    }

    fn test_api_key_model<'a>(
        &'a self,
        request: AdapterModelTestRequest<'a>,
    ) -> AdapterFuture<'a, AdapterModelTestOutcome> {
        Box::pin(async move {
            match acp::model_probe(request.model, PROBE_PROMPT, PROBE_TIMEOUT).await {
                Ok(_) => AdapterModelTestOutcome {
                    test_passed: true,
                    status: None,
                    message: "Devin CLI answered the test prompt.".to_owned(),
                    provider_response_body: None,
                },
                Err(error) => AdapterModelTestOutcome {
                    test_passed: false,
                    status: None,
                    message: format!("Devin CLI model probe failed: {error}"),
                    provider_response_body: None,
                },
            }
        })
    }
}

/// Sends one Chat Completions request through a freshly spawned CLI session.
/// Streaming clients get a synthesized `text/event-stream` response; the
/// gateway's existing classify and stream-translation pipeline treats it
/// exactly like any OpenAI-compatible upstream. Non-streaming clients get a
/// buffered `chat.completion` JSON response.
pub(super) async fn send_local_upstream(
    _state: &AppState,
    upstream_body: &Value,
    model: &str,
    request_id: &str,
    streaming: bool,
    request_timeout: Duration,
    _credential_secret: Option<&str>,
) -> Result<reqwest::Response, String> {
    let prompt = acp::flatten_chat_messages(upstream_body)?;
    let permit = acp::acquire_session_permit()?;
    let spawned = acp::spawn_session().await?;
    let mut session = spawned.session;
    let handshake =
        tokio::time::timeout(HANDSHAKE_TIMEOUT, session.start_session(Some(model))).await;
    let session_id = match handshake {
        Ok(result) => result?,
        Err(_) => {
            return Err(
                "the Devin CLI did not complete its ACP handshake within 30 seconds".to_owned(),
            );
        }
    };
    let prompt_id = session.send_prompt(&session_id, &prompt).await?;
    let completion_id = format!("chatcmpl-devin-{request_id}");
    let created = acp::unix_timestamp();
    let prompt_chars = prompt.chars().count();
    let caller_tools = prompt::extract_caller_tool_names(upstream_body);
    // Tool-bearing requests are buffered so the router can resolve a tool
    // envelope before anything reaches the client (see tool_envelope.rs).
    let has_caller_tools = !caller_tools.is_empty();
    let render = acp::TurnRender {
        completion_id,
        caller_tools,
        tool_call_id: format!("call_{request_id}"),
        model: model.to_owned(),
        created,
        prompt_chars,
        request_timeout,
    };
    if streaming && !has_caller_tools {
        let body = acp::streaming_turn(session, (spawned.child, permit), prompt_id, render);
        sse_response(body)
    } else {
        let completion =
            acp::buffered_turn(session, (spawned.child, permit), prompt_id, render).await?;
        if streaming {
            sse_response(completion_to_sse(completion))
        } else {
            json_response(&completion)
        }
    }
}

/// Converts a buffered completion into OpenAI SSE frames for streaming
/// clients whose tool-bearing turns were resolved after collection.
fn completion_to_sse(
    completion: Value,
) -> impl futures_util::Stream<Item = Result<Vec<u8>, std::io::Error>> + Send + 'static {
    let id = completion["id"].as_str().unwrap_or_default().to_owned();
    let created = completion["created"].as_u64().unwrap_or_default();
    let model = completion["model"].as_str().unwrap_or_default().to_owned();
    let choice = &completion["choices"][0];
    let message = &choice["message"];
    let finish_reason = choice["finish_reason"].as_str();
    let usage = completion.get("usage").cloned();
    let mut frames = Vec::new();
    let delta = if message.get("tool_calls").is_some() {
        json!({"role": "assistant", "tool_calls": message["tool_calls"].clone()})
    } else {
        json!({"role": "assistant", "content": message["content"].clone()})
    };
    frames.push(acp::chat_chunk_frame(
        &id, &model, created, delta, None, None,
    ));
    frames.push(acp::chat_chunk_frame(
        &id,
        &model,
        created,
        json!({}),
        finish_reason,
        usage,
    ));
    frames.push(acp::done_frame());
    futures_util::stream::iter(frames.into_iter().map(Ok))
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
        .map_err(|error| format!("could not build the Devin CLI response: {error}"))
}

fn json_response(completion: &Value) -> Result<reqwest::Response, String> {
    let bytes = serde_json::to_vec(completion)
        .map_err(|error| format!("could not encode the Devin CLI completion: {error}"))?;
    http::Response::builder()
        .status(http::StatusCode::OK)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(reqwest::Body::from(bytes))
        .map(reqwest::Response::from)
        .map_err(|error| format!("could not build the Devin CLI response: {error}"))
}
