//! Agent Client Protocol bridge for the local Devin CLI.
//!
//! ExoRoute spawns `devin acp --agent-type summarizer` per request and speaks
//! newline-delimited JSON-RPC 2.0 over the child's stdio, converting the
//! agent's message updates into Chat Completions wire output. The
//! `summarizer` agent type has no file or shell tools, so a turn can never
//! act on this machine; server-initiated requests (including permission
//! requests) are answered with a method-not-found denial and are never
//! auto-approved. The `session/prompt` result carries the authoritative
//! `stopReason` and is the turn end — treating it as a mere acknowledgement
//! hangs the stream.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};

/// Process-locator "endpoint" carried by the preset. It is never fetched:
/// local transports skip egress validation and the send step spawns the CLI.
pub(super) const DEVIN_ENDPOINT_URL: &str = "devin://acp/stdio";
const ACP_PROTOCOL_VERSION: &str = "0.3";
/// One ACP message; an oversized line means a broken or hostile child.
const MAX_LINE_BYTES: usize = 1024 * 1024;
/// Bound on the agent reply streamed or buffered back.
const MAX_OUTPUT_TEXT_CHARS: usize = 1_000_000;
const CHILD_ARGS: &[&str] = &["acp", "--agent-type", "summarizer"];
/// Environment variables the child must never inherit: an ambient coding-CLI
/// credential visible to the agent would silently change which account —
/// or which provider — serves the turn. `WINDSURF_API_KEY` stays on purpose:
/// it is the CLI's documented alternative sign-in.
const CHILD_ENV_BLOCKLIST: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "OPENAI_API_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "DEEPSEEK_API_KEY",
    "ZHIPUAI_API_KEY",
];

/// Resolves the CLI binary: explicit override, the documented install
/// locations, then a bare name so the OS PATH lookup decides. Returns an
/// error only for an override that does not exist — a missing default
/// install surfaces at spawn time with the OS error text instead.
pub(super) fn discover_devin_binary(
    env_override: Option<&str>,
    home: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(overridden) = env_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let candidate = PathBuf::from(overridden);
        if !candidate.is_file() {
            return Err(format!(
                "EXOROUTE_DEVIN_BIN points at '{}' which is not an existing file",
                candidate.display()
            ));
        }
        return Ok(candidate);
    }
    if let Some(home) = home {
        let candidates: Vec<PathBuf> = if cfg!(windows) {
            vec![
                home.join("AppData")
                    .join("Local")
                    .join("devin")
                    .join("cli")
                    .join("bin")
                    .join("devin.exe"),
            ]
        } else {
            vec![
                home.join(".local/share/devin/bin/devin"),
                home.join(".devin/bin/devin"),
            ]
        };
        for candidate in candidates {
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Ok(PathBuf::from("devin"))
}

pub(super) use super::prompt::flatten_chat_messages;

fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

/// One agent turn event: a text delta, or the prompt result that ends the turn.
pub(super) enum TurnEvent {
    Text(String),
    Ended { stop_reason: String },
}

/// Each CLI session is a local agent process, so the gateway caps concurrent
/// sessions well below the HTTP fan-out; excess attempts fail with a clear
/// message instead of queueing unboundedly.
static ACP_SESSION_PERMITS: std::sync::LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(tokio::sync::Semaphore::new(4)));

pub(super) fn acquire_session_permit() -> Result<tokio::sync::OwnedSemaphorePermit, String> {
    std::sync::Arc::clone(&ACP_SESSION_PERMITS)
        .try_acquire_owned()
        .map_err(|_| {
            "the Devin CLI transport is already running its maximum number of concurrent sessions"
                .to_owned()
        })
}

/// The CLI session over generic byte IO, so tests drive it through duplex
/// pipes instead of a real process. Message sequencing is strict: requests
/// are answered in order, unsolicited server requests are denied, and
/// notifications that are not message chunks are dropped.
pub(super) struct AcpSession<W, R> {
    stdin: W,
    stdout: BufReader<R>,
    next_id: u64,
}

impl<W: AsyncWrite + Unpin, R: AsyncRead + Unpin> AcpSession<W, R> {
    pub(super) fn new(stdin: W, stdout: R) -> Self {
        Self {
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        }
    }

    async fn write_message(&mut self, value: &Value) -> Result<(), String> {
        let mut line = serde_json::to_string(value)
            .map_err(|error| format!("could not encode the ACP message: {error}"))?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|error| format!("the Devin CLI closed its input: {error}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|error| format!("the Devin CLI input could not be flushed: {error}"))
    }

    /// Reads one ndjson message; `None` marks end of output.
    async fn read_message(&mut self) -> Result<Option<Value>, String> {
        loop {
            let mut line = Vec::new();
            let read = self
                .stdout
                .read_until(b'\n', &mut line)
                .await
                .map_err(|error| format!("the Devin CLI output could not be read: {error}"))?;
            if read == 0 {
                return Ok(None);
            }
            if line.len() > MAX_LINE_BYTES {
                return Err("the Devin CLI sent an oversized ACP message".to_owned());
            }
            while line
                .last()
                .is_some_and(|byte| *byte == b'\n' || *byte == b'\r')
            {
                line.pop();
            }
            if line.is_empty() {
                continue;
            }
            return serde_json::from_slice(&line)
                .map(Some)
                .map_err(|error| format!("the Devin CLI sent a malformed ACP message: {error}"));
        }
    }

    async fn send_request(&mut self, method: &str, params: Value) -> Result<u64, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.write_message(
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )
        .await?;
        Ok(id)
    }

    /// Sends a request and waits for its own result, denying any
    /// server-initiated request on the way and dropping notifications.
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.send_request(method, params).await?;
        loop {
            let Some(message) = self.read_message().await? else {
                return Err(format!("the Devin CLI exited before answering '{method}'"));
            };
            if message.get("id").and_then(Value::as_u64) == Some(id)
                && (message.get("result").is_some() || message.get("error").is_some())
            {
                if let Some(error) = message.get("error") {
                    return Err(format!(
                        "the Devin CLI rejected '{method}': {}",
                        compact_error_text(error)
                    ));
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
            self.handle_unsolicited(message).await?;
        }
    }

    /// Answers a server-initiated request so the child never blocks on us.
    async fn handle_unsolicited(&mut self, message: Value) -> Result<(), String> {
        let Some(id) = message.get("id").and_then(Value::as_u64) else {
            return Ok(()); // notification or unrelated frame: nothing to answer
        };
        if message.get("method").is_none() {
            return Ok(()); // a response to a request we did not track
        }
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32601,
                "message": "ExoRoute runs the Devin CLI without tool or permission access"
            }
        }))
        .await
    }

    /// Runs the handshake and opens a session; `model` may be omitted to let
    /// the CLI use its own default (credential probes do that).
    pub(super) async fn start_session(&mut self, model: Option<&str>) -> Result<String, String> {
        self.request(
            "initialize",
            json!({"protocolVersion": ACP_PROTOCOL_VERSION, "clientCapabilities": {}}),
        )
        .await?;
        // Devin 3000.2.x rejects session/new without the mcpServers field.
        let mut params = json!({"cwd": std::env::temp_dir(), "mcpServers": []});
        if let Some(model) = model {
            params["model"] = json!(model);
        }
        let result = self.request("session/new", params).await?;
        let session_id = result
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or("the Devin CLI did not return a session id")?
            .to_owned();
        Ok(session_id)
    }

    /// Submits the turn prompt; the returned id identifies its result.
    pub(super) async fn send_prompt(
        &mut self,
        session_id: &str,
        text: &str,
    ) -> Result<u64, String> {
        self.send_request(
            "session/prompt",
            json!({
                "sessionId": session_id,
                // Devin expects the prompt as a content block array, not a
                // bare string.
                "prompt": [{"type": "text", "text": text}],
            }),
        )
        .await
    }

    /// Reads the next turn event. `None` marks end of output before the turn
    /// result arrived, which is a failure for the caller to report.
    pub(super) async fn next_turn_event(
        &mut self,
        prompt_id: u64,
    ) -> Result<Option<TurnEvent>, String> {
        loop {
            let Some(message) = self.read_message().await? else {
                return Ok(None);
            };
            if message.get("id").and_then(Value::as_u64) == Some(prompt_id)
                && (message.get("result").is_some() || message.get("error").is_some())
            {
                if let Some(error) = message.get("error") {
                    return Err(format!(
                        "the Devin CLI turn failed: {}",
                        compact_error_text(error)
                    ));
                }
                return Ok(Some(TurnEvent::Ended {
                    stop_reason: message
                        .pointer("/result/stopReason")
                        .and_then(Value::as_str)
                        .unwrap_or("end_turn")
                        .to_owned(),
                }));
            }
            if let Some(chunk) = extract_message_chunk(&message) {
                return Ok(Some(TurnEvent::Text(chunk)));
            }
            self.handle_unsolicited(message).await?;
        }
    }
}

/// A spawned CLI with its stdio wired into a session. Holding the `Child`
/// (spawned with `kill_on_drop`) is what makes cancellation safe: when the
/// response future is dropped — client disconnect, timeout, panic — the
/// agent process is killed and reaped.
pub(super) struct SpawnedCli {
    pub session: AcpSession<ChildStdin, ChildStdout>,
    pub child: Child,
}

pub(super) async fn spawn_session() -> Result<SpawnedCli, String> {
    let binary = discover_devin_binary(
        std::env::var("EXOROUTE_DEVIN_BIN").ok().as_deref(),
        home_dir().as_deref(),
    )?;
    let mut command = tokio::process::Command::new(&binary);
    command.args(CHILD_ARGS);
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    for name in CHILD_ENV_BLOCKLIST {
        command.env_remove(name);
    }
    let mut child = command.spawn().map_err(|error| {
        format!(
            "could not start the Devin CLI '{}': {error}",
            binary.display()
        )
    })?;
    let stdin = child
        .stdin
        .take()
        .ok_or("the Devin CLI process has no stdin pipe")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("the Devin CLI process has no stdout pipe")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("the Devin CLI process has no stderr pipe")?;
    // The CLI's own logs stay out of the gateway, but the pipe must keep
    // draining or a verbose child blocks on a full stderr buffer.
    tokio::spawn(drain_stderr(stderr));
    Ok(SpawnedCli {
        session: AcpSession::new(stdin, stdout),
        child,
    })
}

fn home_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    } else {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

async fn drain_stderr(mut stderr: ChildStderr) {
    let mut buffer = [0u8; 4096];
    loop {
        match stderr.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
}

/// Extracts assistant output from ACP `agent_message_chunk` updates. ACP v1
/// chunks carry one content block; implementations can encode that block as
/// either a JSON object or an array of blocks. Ignore thoughts and non-text.
pub(super) fn extract_message_chunk(message: &Value) -> Option<String> {
    let update = message
        .pointer("/params/update")
        .or_else(|| message.get("params"))?;
    let update_kind = update.get("sessionUpdate").and_then(Value::as_str)?;
    if update_kind != "agent_message_chunk" {
        return None;
    }
    let content = update.get("content")?;
    let mut text = String::new();
    match content {
        Value::Array(blocks) => {
            for block in blocks {
                append_text_block(block, &mut text);
            }
        }
        Value::Object(_) => append_text_block(content, &mut text),
        _ => return None,
    }
    (!text.is_empty()).then_some(text)
}

fn append_text_block(block: &Value, text: &mut String) {
    if block.get("type").and_then(Value::as_str) == Some("text")
        && let Some(part) = block.get("text").and_then(Value::as_str)
    {
        text.push_str(part);
    }
}

pub(super) fn has_assistant_output(text: &str) -> bool {
    !text.trim().is_empty()
}

fn compact_error_text(error: &Value) -> String {
    let text = error
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| error.to_string());
    truncate_chars(&text, 300)
}

fn estimated_tokens(chars: usize) -> u64 {
    // Rough character/4 estimate, mirroring the CLI bridges in 9Router and
    // OmniRoute; the gateway marks these as estimates by contract.
    (chars as u64).div_ceil(4)
}

pub(super) fn unix_timestamp() -> u64 {
    // unix_seconds() already maps a pre-epoch clock to 0.
    crate::support::time::unix_seconds() as u64
}

fn frame(value: &Value) -> Vec<u8> {
    // json!-built values are acyclic with string keys, so serialization
    // cannot fail; the guard keeps that invariant explicit.
    let text = serde_json::to_string(value).expect("static JSON values always serialize");
    let mut bytes = b"data: ".to_vec();
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(b"\n\n");
    bytes
}

fn done_frame() -> Vec<u8> {
    b"data: [DONE]\n\n".to_vec()
}

fn chat_chunk_frame(
    completion_id: &str,
    model: &str,
    created: u64,
    delta: Value,
    finish_reason: Option<&str>,
    usage: Option<Value>,
) -> Vec<u8> {
    let mut body = json!({
        "id": completion_id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{"index": 0, "delta": delta, "finish_reason": finish_reason}],
    });
    if let Some(usage) = usage {
        body["usage"] = usage;
    }
    frame(&body)
}

/// Mid-stream failure: an OpenAI-style error event the gateway's stream
/// error extraction understands, then a clean `[DONE]` terminator.
fn error_turn_frame(message: &str) -> Vec<u8> {
    let mut bytes = frame(&json!({
        "error": {"message": message, "type": "upstream_error", "code": "devin_cli_failure"},
    }));
    bytes.extend_from_slice(b"data: [DONE]\n\n");
    bytes
}

/// Render parameters for a CLI turn, shared by the streaming and buffered
/// turn loops.
pub(super) struct TurnRender {
    pub completion_id: String,
    pub model: String,
    pub created: u64,
    pub prompt_chars: usize,
    pub request_timeout: Duration,
}

/// Streams one CLI turn as Chat Completions SSE frames. `guard` is opaque
/// state that must outlive the stream — the spawned `Child` and the session
/// permit — so cancelling the response future (client disconnect, upstream
/// failure) drops and kills the agent process.
pub(super) fn streaming_turn<W, R, G>(
    mut session: AcpSession<W, R>,
    guard: G,
    prompt_id: u64,
    render: TurnRender,
) -> impl futures_util::Stream<Item = Result<Vec<u8>, std::io::Error>> + Send + 'static
where
    W: AsyncWrite + Unpin + Send + 'static,
    R: AsyncRead + Unpin + Send + 'static,
    G: Send + 'static,
{
    async_stream::stream! {
        let _keep_alive = guard;
        let TurnRender { completion_id, model, created, prompt_chars, request_timeout } = render;
        let mut output_chars: usize = 0;
        let mut has_non_whitespace_output = false;
        yield Ok(chat_chunk_frame(
            &completion_id,
            &model,
            created,
            json!({"role": "assistant", "content": ""}),
            None,
            None,
        ));
        let deadline = tokio::time::Instant::now() + request_timeout;
        loop {
            match tokio::time::timeout_at(deadline, session.next_turn_event(prompt_id)).await {
                Err(_) => {
                    yield Ok(error_turn_frame("the Devin CLI session exceeded the request timeout"));
                    break;
                }
                Ok(Err(error)) => {
                    yield Ok(error_turn_frame(&error));
                    break;
                }
                Ok(Ok(None)) => {
                    yield Ok(error_turn_frame("the Devin CLI exited before finishing the turn"));
                    break;
                }
                Ok(Ok(Some(TurnEvent::Text(delta)))) => {
                    output_chars += delta.chars().count();
                    has_non_whitespace_output |= !delta.trim().is_empty();
                    if output_chars > MAX_OUTPUT_TEXT_CHARS {
                        yield Ok(error_turn_frame("the Devin CLI reply exceeded the output limit"));
                        break;
                    }
                    yield Ok(chat_chunk_frame(
                        &completion_id,
                        &model,
                        created,
                        json!({"content": delta}),
                        None,
                        None,
                    ));
                }
                Ok(Ok(Some(TurnEvent::Ended { stop_reason }))) => {
                    if !has_non_whitespace_output {
                        yield Ok(error_turn_frame("the Devin CLI completed the turn without assistant text"));
                        break;
                    }
                    let finish_reason = if stop_reason == "max_tokens" { "length" } else { "stop" };
                    let usage = json!({
                        "prompt_tokens": estimated_tokens(prompt_chars),
                        "completion_tokens": estimated_tokens(output_chars),
                        "total_tokens": estimated_tokens(prompt_chars + output_chars),
                    });
                    yield Ok(chat_chunk_frame(
                        &completion_id,
                        &model,
                        created,
                        json!({}),
                        Some(finish_reason),
                        Some(usage),
                    ));
                    yield Ok(done_frame());
                    break;
                }
            }
        }
    }
}

/// Buffers one CLI turn into a complete `chat.completion` response for
/// non-streaming clients. Fails instead of emitting a partial answer: the
/// client has received nothing yet, so a clean error beats a truncated body.
pub(super) async fn buffered_turn<W, R, G>(
    mut session: AcpSession<W, R>,
    guard: G,
    prompt_id: u64,
    render: TurnRender,
) -> Result<Value, String>
where
    W: AsyncWrite + Unpin + Send + 'static,
    R: AsyncRead + Unpin + Send + 'static,
    G: Send + 'static,
{
    let _keep_alive = guard;
    let TurnRender {
        completion_id,
        model,
        created,
        prompt_chars,
        request_timeout,
    } = render;
    let deadline = tokio::time::Instant::now() + request_timeout;
    let mut text = String::new();
    loop {
        let event = tokio::time::timeout_at(deadline, session.next_turn_event(prompt_id))
            .await
            .map_err(|_| "the Devin CLI session exceeded the request timeout".to_owned())?;
        match event? {
            Some(TurnEvent::Text(delta)) => {
                text.push_str(&delta);
                if text.chars().count() > MAX_OUTPUT_TEXT_CHARS {
                    return Err("the Devin CLI reply exceeded the output limit".to_owned());
                }
            }
            Some(TurnEvent::Ended { stop_reason }) => {
                if !has_assistant_output(&text) {
                    return Err(
                        "the Devin CLI completed the turn without assistant text".to_owned()
                    );
                }
                let completion_tokens = estimated_tokens(text.chars().count());
                return Ok(json!({
                    "id": completion_id,
                    "object": "chat.completion",
                    "created": created,
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "message": {"role": "assistant", "content": text},
                        "finish_reason": if stop_reason == "max_tokens" { "length" } else { "stop" },
                    }],
                    "usage": {
                        "prompt_tokens": estimated_tokens(prompt_chars),
                        "completion_tokens": completion_tokens,
                        "total_tokens": estimated_tokens(prompt_chars) + completion_tokens,
                    },
                }));
            }
            None => return Err("the Devin CLI exited before finishing the turn".to_owned()),
        }
    }
}

/// Credential probe: spawn plus a full ACP handshake, then drop the session.
pub(super) async fn handshake_probe(timeout: Duration) -> Result<(), String> {
    let _permit = acquire_session_permit()?;
    let SpawnedCli { mut session, child } = spawn_session().await?;
    let _keep_alive = child;
    tokio::time::timeout(timeout, session.start_session(None))
        .await
        .map_err(|_| "the Devin CLI did not complete its ACP handshake in time".to_owned())??;
    Ok(())
}

/// Model probe: one minimal prompt turn against the requested model.
pub(super) async fn model_probe(
    model: &str,
    prompt: &str,
    timeout: Duration,
) -> Result<String, String> {
    let _permit = acquire_session_permit()?;
    let SpawnedCli { mut session, child } = spawn_session().await?;
    let _keep_alive = child;
    let deadline = tokio::time::Instant::now() + timeout;
    let session_id = tokio::time::timeout_at(deadline, session.start_session(Some(model)))
        .await
        .map_err(|_| "the Devin CLI did not complete its ACP handshake in time".to_owned())??;
    let prompt_id = session.send_prompt(&session_id, prompt).await?;
    let mut text = String::new();
    loop {
        let event = tokio::time::timeout_at(deadline, session.next_turn_event(prompt_id))
            .await
            .map_err(|_| "the Devin CLI model probe exceeded its time budget".to_owned())?;
        match event? {
            Some(TurnEvent::Text(delta)) => {
                text.push_str(&delta);
                if text.chars().count() > MAX_OUTPUT_TEXT_CHARS {
                    return Err("the Devin CLI reply exceeded the output limit".to_owned());
                }
            }
            Some(TurnEvent::Ended { .. }) => {
                if !has_assistant_output(&text) {
                    return Err(
                        "the Devin CLI completed the probe without assistant text".to_owned()
                    );
                }
                return Ok(text);
            }
            None => return Err("the Devin CLI exited before finishing the probe".to_owned()),
        }
    }
}
