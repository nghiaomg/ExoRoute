use super::*;
use std::time::Duration;

mod acp;
#[cfg(test)]
mod tests;

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
    let render = acp::TurnRender {
        completion_id,
        model: model.to_owned(),
        created,
        prompt_chars,
        request_timeout,
    };
    if streaming {
        let body = acp::streaming_turn(session, (spawned.child, permit), prompt_id, render);
        sse_response(body)
    } else {
        let completion =
            acp::buffered_turn(session, (spawned.child, permit), prompt_id, render).await?;
        json_response(&completion)
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
