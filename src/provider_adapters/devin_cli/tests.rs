use super::acp::{
    AcpSession, DEVIN_ENDPOINT_URL, TurnEvent, discover_devin_binary, flatten_chat_messages,
    streaming_turn,
};
use super::*;
use futures_util::StreamExt;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex};

fn json_line(value: &serde_json::Value) -> String {
    format!("{}\n", serde_json::to_string(value).unwrap())
}

/// Spawns a session whose "child" is a duplex pair: the test scripts the
/// child's stdout through `child_out` and reads the requests the engine
/// writes from `child_in`.
fn scripted_session() -> (
    AcpSession<impl tokio::io::AsyncWrite + Unpin, impl tokio::io::AsyncRead + Unpin>,
    BufReader<impl tokio::io::AsyncRead + Unpin>,
    impl tokio::io::AsyncWrite + Unpin,
) {
    let (engine_to_child, child_in) = duplex(64 * 1024);
    let (child_out, engine_reads) = duplex(64 * 1024);
    let session = AcpSession::new(engine_to_child, engine_reads);
    let requests = BufReader::new(child_in);
    (session, requests, child_out)
}

#[tokio::test]
async fn start_session_sends_initialize_then_session_new_with_empty_mcp_servers() {
    let (mut session, mut requests, mut child_out) = scripted_session();

    let pending = tokio::spawn(async move { session.start_session(Some("swe-1-7")).await });

    let mut line = String::new();
    requests.read_line(&mut line).await.unwrap();
    let initialize: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(initialize["method"], "initialize");
    assert_eq!(initialize["params"]["protocolVersion"], "0.3");

    child_out
        .write_all(
            json_line(&serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": {}})).as_bytes(),
        )
        .await
        .unwrap();

    line.clear();
    requests.read_line(&mut line).await.unwrap();
    let new_session: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(new_session["method"], "session/new");
    assert_eq!(new_session["params"]["model"], "swe-1-7");
    assert_eq!(new_session["params"]["mcpServers"], serde_json::json!([]));

    child_out
        .write_all(
            json_line(&serde_json::json!({
                "jsonrpc": "2.0", "id": 2,
                "result": {"sessionId": "s-1"},
            }))
            .as_bytes(),
        )
        .await
        .unwrap();

    assert_eq!(pending.await.unwrap().unwrap(), "s-1");
}

#[tokio::test]
async fn prompt_turn_flattens_updates_and_ends_on_the_prompt_result() {
    let (mut session, mut requests, mut child_out) = scripted_session();

    child_out
        .write_all(
            json_line(&serde_json::json!({
                "jsonrpc": "2.0", "method": "session/update",
                "params": {"update": {"sessionUpdate": "agent_message_chunk",
                    "content": [{"type": "text", "text": "Hello"}]}},
            }))
            .as_bytes(),
        )
        .await
        .unwrap();
    // Flat ACP variant: sessionUpdate directly on params.
    child_out
        .write_all(
            json_line(&serde_json::json!({
                "jsonrpc": "2.0", "method": "session/update",
                "params": {"sessionUpdate": "agent_message_chunk",
                    "content": [{"type": "text", "text": " world"}]},
            }))
            .as_bytes(),
        )
        .await
        .unwrap();
    // Devin ACP can encode a text chunk as one content object rather than an array.
    child_out
        .write_all(
            json_line(&serde_json::json!({
                "jsonrpc": "2.0", "method": "session/update",
                "params": {"update": {"sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": "!"}}},
            }))
            .as_bytes(),
        )
        .await
        .unwrap();
    child_out
        .write_all(
            json_line(&serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "result": {"stopReason": "end_turn"},
            }))
            .as_bytes(),
        )
        .await
        .unwrap();

    let prompt_id = session.send_prompt("s-1", "hi").await.unwrap();
    assert_eq!(prompt_id, 1);

    let mut texts = Vec::new();
    loop {
        match session.next_turn_event(prompt_id).await.unwrap().unwrap() {
            TurnEvent::Text(delta) => texts.push(delta),
            TurnEvent::Ended { stop_reason } => {
                assert_eq!(stop_reason, "end_turn");
                break;
            }
        }
    }
    assert_eq!(
        texts,
        vec!["Hello".to_owned(), " world".to_owned(), "!".to_owned()]
    );

    // The engine only wrote the prompt request: no other stdin traffic.
    let mut line = String::new();
    requests.read_line(&mut line).await.unwrap();
    let prompt: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(prompt["method"], "session/prompt");
    assert_eq!(prompt["params"]["prompt"][0]["type"], "text");
    assert_eq!(prompt["params"]["prompt"][0]["text"], "hi");
}

#[tokio::test]
async fn a_server_request_is_denied_and_the_turn_continues() {
    let (mut session, mut requests, mut child_out) = scripted_session();

    // The agent asks for permission mid-turn; the engine must answer so the
    // child never blocks, and must never approve.
    child_out
        .write_all(
            json_line(&serde_json::json!({
                "jsonrpc": "2.0", "id": 50, "method": "session/request_permission",
                "params": {},
            }))
            .as_bytes(),
        )
        .await
        .unwrap();
    child_out
        .write_all(
            json_line(&serde_json::json!({
                "jsonrpc": "2.0", "method": "session/update",
                "params": {"update": {"sessionUpdate": "agent_message_chunk",
                    "content": [{"type": "text", "text": "still here"}]}},
            }))
            .as_bytes(),
        )
        .await
        .unwrap();
    child_out
        .write_all(
            json_line(&serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": {"stopReason": "end_turn"}}))
                .as_bytes(),
        )
        .await
        .unwrap();

    let prompt_id = session.send_prompt("s-1", "hi").await.unwrap();
    let mut saw_text = false;
    while let TurnEvent::Text(_) = session.next_turn_event(prompt_id).await.unwrap().unwrap() {
        saw_text = true;
    }
    assert!(saw_text, "the turn survived the denied request");

    // The engine wrote the prompt request first, then the denial.
    let mut prompt_line = String::new();
    requests.read_line(&mut prompt_line).await.unwrap();
    let mut denial = String::new();
    requests.read_line(&mut denial).await.unwrap();
    let denial: serde_json::Value = serde_json::from_str(&denial).unwrap();
    assert_eq!(denial["id"], 50);
    assert_eq!(denial["error"]["code"], -32601);
}

#[test]
fn acp_assistant_chunk_accepts_object_or_array_and_ignores_other_updates() {
    assert!(super::acp::has_assistant_output("usable answer"));
    let whitespace = super::acp::extract_message_chunk(&serde_json::json!({
        "params": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "  \n"}},
    }))
    .expect("a text chunk is extracted");
    assert!(!super::acp::has_assistant_output(&whitespace));
    // Single-object chunks are the format returned by Devin CLI.
    let nested = serde_json::json!({
        "params": {"update": {"sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "nested"}}},
    });
    assert_eq!(
        super::acp::extract_message_chunk(&nested).as_deref(),
        Some("nested")
    );

    let flat_complete_message = serde_json::json!({
        "params": {"sessionUpdate": "agent_message",
            "content": [{"type": "text", "text": "complete"}]},
    });
    assert_eq!(
        super::acp::extract_message_chunk(&flat_complete_message),
        None
    );

    let non_assistant_message = serde_json::json!({
        "params": {"sessionUpdate": "agent_thought_chunk",
            "content": {"type": "text", "text": "private reasoning"}},
    });
    assert_eq!(
        super::acp::extract_message_chunk(&non_assistant_message),
        None
    );
}

#[tokio::test]
async fn empty_successful_devin_turn_is_an_error_not_an_empty_completion() {
    let (engine_to_child, _child_in) = duplex(64 * 1024);
    let (mut child_out, engine_reads) = duplex(64 * 1024);
    child_out.write_all(
        json_line(&serde_json::json!({"jsonrpc": "2.0", "id": 7, "result": {"stopReason": "end_turn"}})).as_bytes(),
    ).await.unwrap();
    drop(child_out);

    let stream = streaming_turn(
        AcpSession::new(engine_to_child, engine_reads),
        (),
        7,
        super::acp::TurnRender {
            completion_id: "chatcmpl-empty".to_owned(),
            model: "swe-2-flash".to_owned(),
            created: 0,
            prompt_chars: 8,
            request_timeout: Duration::from_secs(5),
        },
    );
    let mut output = String::new();
    {
        futures_util::pin_mut!(stream);
        while let Some(frame) = stream.next().await {
            output.push_str(&String::from_utf8(frame.unwrap()).unwrap());
        }
    }
    assert!(
        output.contains("the Devin CLI completed the turn without assistant text"),
        "{output}"
    );
    assert!(
        !output.contains("\"finish_reason\":\"stop\""),
        "empty completion must not look successful: {output}"
    );
    assert!(output.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn streaming_turn_emits_role_content_and_finish_frames() {
    let (engine_to_child, _child_in) = duplex(64 * 1024);
    let (mut child_out, engine_reads) = duplex(64 * 1024);
    child_out
        .write_all(
            json_line(&serde_json::json!({
                "jsonrpc": "2.0", "method": "session/update",
                "params": {"update": {"sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": "Hi"}}},
            }))
            .as_bytes(),
        )
        .await
        .unwrap();
    child_out
        .write_all(
            json_line(&serde_json::json!({"jsonrpc": "2.0", "id": 7, "result": {"stopReason": "end_turn"}}))
                .as_bytes(),
        )
        .await
        .unwrap();
    drop(child_out);

    let stream = streaming_turn(
        AcpSession::new(engine_to_child, engine_reads),
        (),
        7,
        super::acp::TurnRender {
            completion_id: "chatcmpl-1".to_owned(),
            model: "swe-1-7".to_owned(),
            created: 0,
            prompt_chars: 8,
            request_timeout: Duration::from_secs(5),
        },
    );
    let mut output = String::new();
    {
        futures_util::pin_mut!(stream);
        while let Some(frame) = stream.next().await {
            output.push_str(&String::from_utf8(frame.unwrap()).unwrap());
        }
    }

    let frames: Vec<&str> = output
        .split("\n\n")
        .filter(|part| !part.is_empty())
        .collect();
    assert_eq!(frames.len(), 4, "role, content, finish, [DONE]: {output}");
    assert!(frames[0].starts_with("data: "));
    let role: serde_json::Value =
        serde_json::from_str(frames[0].trim_start_matches("data: ")).unwrap();
    assert_eq!(role["choices"][0]["delta"]["role"], "assistant");
    let content: serde_json::Value =
        serde_json::from_str(frames[1].trim_start_matches("data: ")).unwrap();
    assert_eq!(content["choices"][0]["delta"]["content"], "Hi");
    let finish: serde_json::Value =
        serde_json::from_str(frames[2].trim_start_matches("data: ")).unwrap();
    assert_eq!(finish["choices"][0]["finish_reason"], "stop");
    assert_eq!(finish["usage"]["completion_tokens"], 1);
    assert_eq!(frames[3], "data: [DONE]");
}

#[tokio::test]
async fn an_exited_child_becomes_a_stream_error_frame() {
    let (engine_to_child, _child_in) = duplex(64 * 1024);
    let (child_out, engine_reads) = duplex(64 * 1024);
    drop(child_out); // the CLI died before answering

    let stream = streaming_turn(
        AcpSession::new(engine_to_child, engine_reads),
        (),
        7,
        super::acp::TurnRender {
            completion_id: "chatcmpl-1".to_owned(),
            model: "swe-1-7".to_owned(),
            created: 0,
            prompt_chars: 8,
            request_timeout: Duration::from_secs(5),
        },
    );
    let mut output = String::new();
    {
        futures_util::pin_mut!(stream);
        while let Some(frame) = stream.next().await {
            output.push_str(&String::from_utf8(frame.unwrap()).unwrap());
        }
    }

    assert!(output.contains("\"error\""), "got: {output}");
    assert!(output.ends_with("data: [DONE]\n\n"));
}

#[test]
fn flatten_formats_caller_tools_response_format_and_tool_results() {
    let text = flatten_chat_messages(&serde_json::json!({
        "messages": [
            {"role": "system", "content": "be brief"},
            {"role": "user", "content": "hi"},
            {"role": "assistant", "content": "", "tool_calls": [
                {"function": {"name": "lookup", "arguments": "{\"q\":\"x\"}"}}
            ]},
            {"role": "tool", "content": "result"},
        ],
    }))
    .unwrap();
    assert!(text.contains("[System]\nbe brief"));
    assert!(text.contains("[User]\nhi"));
    assert!(text.contains("[Tool Call] lookup"));
    assert!(text.contains("[Tool]\nresult"));

    let image = flatten_chat_messages(&serde_json::json!({
        "messages": [{"role": "user", "content": [
            {"type": "text", "text": "look"},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,x"}},
        ]}],
    }))
    .unwrap_err();
    assert!(image.contains("image"), "got: {image}");

    let with_tools = flatten_chat_messages(&serde_json::json!({
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [
            {"type": "function", "function": {"name": "lookup_weather"}},
            {"type": "function", "function": {"name": ""}},
            {"name": "anthropic_tool"}
        ],
    }))
    .unwrap();
    assert!(with_tools.contains(
        "[System]\nCaller-side tools are available externally: lookup_weather, anthropic_tool."
    ));
    assert!(
        with_tools.contains("Do not call those tools yourself. Answer in assistant text only.")
    );
    assert!(with_tools.contains("[User]\nhi"));

    let with_json = flatten_chat_messages(&serde_json::json!({
        "messages": [{"role": "user", "content": "hi"}],
        "response_format": {"type": "json_object"},
    }))
    .unwrap();
    assert!(with_json.contains("[System]\nReturn only valid JSON."));

    let with_schema = flatten_chat_messages(&serde_json::json!({
        "messages": [{"role": "user", "content": "hi"}],
        "response_format": {
            "type": "json_schema",
            "json_schema": {"schema": {"type": "object", "properties": {"city": {"type": "string"}}}}
        },
    }))
    .unwrap();
    assert!(with_schema.contains("[System]\nReturn only valid JSON matching this schema:"));
    assert!(with_schema.contains("\"city\""));

    assert!(super::prompt::has_client_tools(&serde_json::json!({
        "tools": [{"type": "function", "function": {"name": "f"}}]
    })));
    assert!(!super::prompt::has_client_tools(&serde_json::json!({
        "messages": [{"role": "user", "content": "hi"}]
    })));
}

#[test]
fn binary_discovery_prefers_the_override_then_known_paths_then_path() {
    let missing = discover_devin_binary(Some("Z:/nowhere/devin.exe"), None).unwrap_err();
    assert!(missing.contains("EXOROUTE_DEVIN_BIN"));

    let home = std::env::temp_dir().join(format!("exoroute-devin-test-{}", std::process::id()));
    let known = if cfg!(windows) {
        home.join("AppData/Local/devin/cli/bin/devin.exe")
    } else {
        home.join(".local/share/devin/bin/devin")
    };
    std::fs::create_dir_all(known.parent().unwrap()).unwrap();
    std::fs::write(&known, b"").unwrap();
    assert_eq!(
        discover_devin_binary(None, Some(&home)).unwrap(),
        known,
        "a documented install location is used"
    );

    assert_eq!(
        discover_devin_binary(None, Some(&home.join("empty"))).unwrap(),
        std::path::PathBuf::from("devin"),
        "otherwise the OS PATH lookup decides"
    );
}

#[test]
fn the_adapter_pins_the_local_endpoint_and_protocol() {
    assert_eq!(
        DEVIN_CLI_ADAPTER
            .endpoint(DEVIN_ENDPOINT_URL, Protocol::ChatCompletions, None)
            .unwrap()
            .as_str(),
        DEVIN_ENDPOINT_URL
    );
    assert!(
        DEVIN_CLI_ADAPTER
            .endpoint("https://example.com", Protocol::ChatCompletions, None)
            .is_err()
    );
    assert!(
        DEVIN_CLI_ADAPTER
            .endpoint(DEVIN_ENDPOINT_URL, Protocol::Messages, None)
            .is_err()
    );
}
