use super::acp::{
    AcpSession, DEVIN_ENDPOINT_URL, TurnEvent, acquire_session_permit, buffered_turn,
    discover_devin_binary, flatten_chat_messages, streaming_turn,
};
use super::summary::{SummaryStripper, strip_summary_envelope};
use super::*;
use futures_util::StreamExt;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex};

/// The session semaphore is process-global and bounded to four concurrent
/// sessions, so parallel tests wait for a permit instead of failing when all
/// four are momentarily held.
async fn acquire_test_session_permit() -> tokio::sync::OwnedSemaphorePermit {
    loop {
        match acquire_session_permit() {
            Ok(permit) => return permit,
            Err(_) => tokio::time::sleep(Duration::from_millis(25)).await,
        }
    }
}

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
            caller_tools: Vec::new(),
            tool_call_id: "call_test".to_owned(),
            completion_id: "chatcmpl-empty".to_owned(),
            model: "swe-2-flash".to_owned(),
            created: 0,
            prompt_text: String::new(),
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
            caller_tools: Vec::new(),
            tool_call_id: "call_test".to_owned(),
            completion_id: "chatcmpl-1".to_owned(),
            model: "swe-1-7".to_owned(),
            created: 0,
            prompt_text: String::new(),
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
            caller_tools: Vec::new(),
            tool_call_id: "call_test".to_owned(),
            completion_id: "chatcmpl-1".to_owned(),
            model: "swe-1-7".to_owned(),
            created: 0,
            prompt_text: String::new(),
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
    assert!(with_tools.contains("end your turn with exactly one envelope"));
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

/// Drives one scripted agent turn and returns the buffered completion the
/// router would send. `caller_tools` feeds the tool-envelope resolution.
async fn buffered_completion_with_turn(
    turn_text: String,
    caller_tools: &[&str],
    completion_id: &str,
) -> serde_json::Value {
    let (mut session, _requests, mut child_out) = scripted_session();
    let writer = tokio::spawn(async move {
        child_out
            .write_all(
                json_line(&serde_json::json!({
                    "jsonrpc": "2.0", "method": "session/update",
                    "params": {"update": {"sessionUpdate": "agent_message_chunk",
                        "content": [{"type": "text", "text": turn_text}]}},
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
    });
    let prompt_id = session.send_prompt("s-1", "hi").await.unwrap();
    let guard = acquire_test_session_permit().await;
    let render = super::acp::TurnRender {
        completion_id: completion_id.to_owned(),
        caller_tools: caller_tools
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
        tool_call_id: "call_test_1".to_owned(),
        model: "swe-2-flash".to_owned(),
        created: 0,
        prompt_text: String::new(),
        prompt_chars: 2,
        request_timeout: Duration::from_secs(5),
    };
    let completion = buffered_turn(session, guard, prompt_id, render)
        .await
        .expect("turn resolves");
    writer.await.unwrap();
    completion
}

/// The exact broken envelope from the field report: the model wrapped a
/// caller tool in its own tag and left the shell quotes unescaped, which
/// makes the JSON invalid. The router must repair it into a tool call
/// instead of leaking the raw markup to the client.
#[tokio::test]
async fn a_broken_native_envelope_converts_into_a_tool_call() {
    let broken = "<execute_bash> {\"command\":\"cd \"C:/WorkSpace/GitHub-Projects/qwencoder/frontend_v2/html\" && bun -e \"Bun.serve({port: 8931, fetch(req){ const p = new URL(req.url).pathname; }})\" \",\"description\":\"Serve mockup folder on port 8931\"} </execute_bash>";
    let completion =
        buffered_completion_with_turn(broken.to_owned(), &["execute_bash"], "chatcmpl-env-1").await;

    let message = &completion["choices"][0]["message"];
    assert!(message["content"].is_null());
    let call = &message["tool_calls"][0];
    assert_eq!(call["id"], "call_test_1");
    assert_eq!(call["type"], "function");
    assert_eq!(call["function"]["name"], "execute_bash");
    let arguments: serde_json::Value =
        serde_json::from_str(call["function"]["arguments"].as_str().unwrap())
            .expect("repaired arguments are valid JSON");
    let command = arguments["command"].as_str().unwrap();
    assert!(
        command.contains("cd \"C:/WorkSpace/GitHub-Projects"),
        "repaired command: {command}"
    );
    assert!(command.contains("Bun.serve"), "repaired command: {command}");
    assert_eq!(arguments["description"], "Serve mockup folder on port 8931");
    assert_eq!(completion["choices"][0]["finish_reason"], "tool_calls");
}

/// An envelope naming a tool the client never declared is model-owned
/// markup: the router strips it so raw tags never reach the client.
#[tokio::test]
async fn an_unknown_envelope_is_stripped_from_the_reply() {
    let text = "Working on it.\n<not_a_tool>{\"command\":\"ls\"}</not_a_tool>\nDone soon.";
    let completion =
        buffered_completion_with_turn(text.to_owned(), &["execute_bash"], "chatcmpl-env-2").await;
    let content = completion["choices"][0]["message"]["content"]
        .as_str()
        .unwrap();
    assert!(content.contains("Working on it."));
    assert!(
        !content.contains("not_a_tool"),
        "leaked envelope: {content}"
    );
    assert!(content.contains("Done soon."));
    assert_eq!(completion["choices"][0]["finish_reason"], "stop");
}

/// A turn with no envelope and no caller tools keeps the plain-text contract.
#[tokio::test]
async fn plain_text_turns_without_tools_are_untouched() {
    let completion =
        buffered_completion_with_turn("plain answer".to_owned(), &[], "chatcmpl-env-3").await;
    assert_eq!(
        completion["choices"][0]["message"]["content"],
        "plain answer"
    );
    assert!(
        completion["choices"][0]["message"]
            .get("tool_calls")
            .is_none()
    );
    assert_eq!(completion["choices"][0]["finish_reason"], "stop");
}

/// The bridge format (`<tool>` with a `name` field) converts too, and a
/// narrative around the envelope is dropped with the action, matching the
/// docs rule that mixed narrative and actions are not relayed as text.
#[tokio::test]
async fn the_tool_bridge_envelope_converts_with_its_arguments() {
    let text = "<tool>{\"name\": \"lookup_weather\", \"arguments\": {\"city\": \"Hanoi\"}}</tool>";
    let completion =
        buffered_completion_with_turn(text.to_owned(), &["lookup_weather"], "chatcmpl-env-4").await;
    let message = &completion["choices"][0]["message"];
    let call = &message["tool_calls"][0];
    assert_eq!(call["function"]["name"], "lookup_weather");
    assert_eq!(call["function"]["arguments"], "{\"city\":\"Hanoi\"}");
    assert_eq!(completion["choices"][0]["finish_reason"], "tool_calls");
}

/// The exact turn shape from the field report: the summarizer wraps its
/// markdown report in `<summary>…</summary>` and the closing tag leaked into
/// the chat frame as literal text.
#[test]
fn the_summary_envelope_is_stripped_from_a_completed_turn() {
    let turn = "<summary>\n## Overview\n\nThe file has been created.\n\n## Current State\n\nTask complete.\n</summary>";
    let cleaned = strip_summary_envelope(turn);
    assert!(cleaned.starts_with("## Overview"), "{cleaned}");
    assert!(cleaned.ends_with("Task complete."), "{cleaned}");
    assert!(!cleaned.contains("<summary"), "{cleaned}");
    assert!(!cleaned.contains("</summary"), "{cleaned}");

    // A turn that does not open with the envelope passes through untouched,
    // including `<summary>` sequences that are legitimate answer content.
    let plain = "Here is an example:\n\n```html\n<summary>text</summary>\n```";
    assert_eq!(strip_summary_envelope(plain), plain);
    // Whitespace around the envelope is part of the envelope, not the report.
    assert_eq!(
        strip_summary_envelope("  <summary>answer</summary>  "),
        "answer"
    );
    // A turn truncated before the closer keeps what arrived, minus the opener.
    assert_eq!(
        strip_summary_envelope("<summary>\npartial report"),
        "partial report"
    );
}

#[test]
fn the_streaming_stripper_hides_tags_split_across_deltas() {
    let mut stripper = SummaryStripper::new();
    let mut emitted = String::new();
    for delta in ["<sum", "mary>\n## Over", "view\n", "</sum", "mary>"] {
        emitted.push_str(&stripper.push(delta));
    }
    emitted.push_str(&stripper.finish());
    assert_eq!(emitted, "\n## Overview\n");

    // Inside the envelope a lone `<` is held back until the next delta
    // resolves it, then emitted in order; a wrapped `<tool>` call in the
    // buffer keeps streaming as text because the closer never arrives here.
    let mut stripper = SummaryStripper::new();
    let mut emitted = String::new();
    for delta in ["<summary>", "x <", " y", "</summary>", "tail"] {
        emitted.push_str(&stripper.push(delta));
    }
    emitted.push_str(&stripper.finish());
    assert_eq!(emitted, "x < ytail");

    // A plain answer never opening with the envelope passes through as-is.
    let mut stripper = SummaryStripper::new();
    let mut emitted = String::new();
    for delta in ["a < b", " and <summary", "> free text"] {
        emitted.push_str(&stripper.push(delta));
    }
    emitted.push_str(&stripper.finish());
    assert_eq!(emitted, "a < b and <summary> free text");
}

/// The field-report turn, streamed in deltas that split both the opener and
/// the closer across chunk boundaries: the client sees the report body and
/// never the envelope markup.
#[tokio::test]
async fn streaming_strips_the_summary_envelope_across_chunk_boundaries() {
    let (engine_to_child, _child_in) = duplex(64 * 1024);
    let (mut child_out, engine_reads) = duplex(64 * 1024);
    for delta in [
        "<sum",
        "mary>\n## Overview\n\n",
        "The file has been created.\n",
        "</sum",
        "mary>",
    ] {
        child_out
            .write_all(
                json_line(&serde_json::json!({
                    "jsonrpc": "2.0", "method": "session/update",
                    "params": {"update": {"sessionUpdate": "agent_message_chunk",
                        "content": {"type": "text", "text": delta}}},
                }))
                .as_bytes(),
            )
            .await
            .unwrap();
    }
    child_out
        .write_all(
            json_line(&serde_json::json!({
                "jsonrpc": "2.0", "id": 7, "result": {"stopReason": "end_turn"},
            }))
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
            caller_tools: Vec::new(),
            tool_call_id: "call_test".to_owned(),
            completion_id: "chatcmpl-sum-1".to_owned(),
            model: "swe-2-flash".to_owned(),
            created: 0,
            prompt_text: String::new(),
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
    assert!(!output.contains("<summary"), "leaked opener: {output}");
    assert!(!output.contains("</summary"), "leaked closer: {output}");
    assert!(output.contains("## Overview"), "{output}");
    assert!(output.contains("The file has been created."), "{output}");
    assert!(output.contains("\"finish_reason\":\"stop\""), "{output}");
}

/// A turn that is nothing but envelope markup has no client-facing content,
/// so it fails loudly instead of emitting an empty successful completion.
#[tokio::test]
async fn an_envelope_only_streaming_turn_still_fails_loudly() {
    let (engine_to_child, _child_in) = duplex(64 * 1024);
    let (mut child_out, engine_reads) = duplex(64 * 1024);
    child_out
        .write_all(
            json_line(&serde_json::json!({
                "jsonrpc": "2.0", "method": "session/update",
                "params": {"update": {"sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": "<summary> </summary>"}}},
            }))
            .as_bytes(),
        )
        .await
        .unwrap();
    child_out
        .write_all(
            json_line(&serde_json::json!({
                "jsonrpc": "2.0", "id": 7, "result": {"stopReason": "end_turn"},
            }))
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
            caller_tools: Vec::new(),
            tool_call_id: "call_test".to_owned(),
            completion_id: "chatcmpl-sum-2".to_owned(),
            model: "swe-2-flash".to_owned(),
            created: 0,
            prompt_text: String::new(),
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
    assert!(!output.contains("\"finish_reason\":\"stop\""), "{output}");
}

/// The buffered path strips the envelope the same way before the completion
/// is returned.
#[tokio::test]
async fn buffered_turn_strips_the_summary_envelope() {
    let turn = "<summary>\n## Overview\n\nDone.\n</summary>";
    let completion = buffered_completion_with_turn(turn.to_owned(), &[], "chatcmpl-sum-3").await;
    let content = completion["choices"][0]["message"]["content"]
        .as_str()
        .unwrap();
    assert!(content.starts_with("## Overview"), "{content}");
    assert!(!content.contains("summary"), "{content}");
    assert_eq!(completion["choices"][0]["finish_reason"], "stop");
}

/// A tool envelope inside a summary-wrapped turn still converts: the summary
/// strip runs before the tool-envelope resolution reads the text.
#[tokio::test]
async fn a_summary_wrapped_tool_envelope_still_converts() {
    let turn = "<summary>\n<tool>{\"name\": \"lookup_weather\", \"arguments\": {\"city\": \"Hanoi\"}}</tool>\n</summary>";
    let completion =
        buffered_completion_with_turn(turn.to_owned(), &["lookup_weather"], "chatcmpl-sum-4").await;
    assert_eq!(completion["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(
        completion["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
        "lookup_weather"
    );
}

/// The capacity refusal Devin's backend emits for a turn. The exact wire text
/// matters: the retry recognizes this class by its message.
const TRANSIENT_REFUSAL: &str = "Client error: Protocol error (invalid_argument): There is an issue with this request, please try a different model.";

/// A request-driven child script that runs the production turn flow: a
/// responder task answers each engine request (handshake, prompt, and — after
/// a retried refusal — the re-handshake) in arrival order. Returns the
/// collected stream output and the number of requests the engine sent.
async fn run_scripted_streaming_turn(
    respond: impl Fn(usize, &str) -> Vec<serde_json::Value> + Send + 'static,
    turn: super::acp::TurnRender,
) -> (String, usize) {
    let (engine_to_child, _child_in) = duplex(64 * 1024);
    let (mut child_out, engine_reads) = duplex(64 * 1024);
    let mut session = AcpSession::new(engine_to_child, engine_reads);
    let mut requests = BufReader::new(_child_in);

    let responder = tokio::spawn(async move {
        let mut request_line = String::new();
        let mut index = 0usize;
        loop {
            let read = requests
                .read_line(&mut request_line)
                .await
                .expect("read engine request");
            if read == 0 {
                break;
            }
            index += 1;
            for reply in respond(index, request_line.trim()) {
                child_out
                    .write_all(json_line(&reply).as_bytes())
                    .await
                    .unwrap();
            }
            request_line.clear();
        }
        index
    });

    // The production sequence: handshake, submit the prompt, then stream the
    // turn — streaming_turn itself only reads child messages.
    let session_id = session
        .start_session(Some(&turn.model))
        .await
        .expect("scripted handshake");
    let prompt_id = session
        .send_prompt(&session_id, &turn.prompt_text)
        .await
        .expect("scripted prompt");

    let stream = streaming_turn(session, (), prompt_id, turn);
    let mut output = String::new();
    futures_util::pin_mut!(stream);
    while let Some(frame) = stream.next().await {
        output.push_str(&String::from_utf8(frame.unwrap()).unwrap());
    }
    let request_count = responder.await.expect("responder task");
    (output, request_count)
}

fn retry_turn_render(completion_id: &str) -> super::acp::TurnRender {
    super::acp::TurnRender {
        caller_tools: Vec::new(),
        tool_call_id: "call_test".to_owned(),
        completion_id: completion_id.to_owned(),
        model: "swe-2-flash-medium".to_owned(),
        created: 0,
        prompt_text: "Reply with OK".to_owned(),
        prompt_chars: 13,
        request_timeout: Duration::from_secs(10),
    }
}

#[tokio::test]
async fn a_transient_model_refusal_is_retried_once_and_succeeds() {
    let (output, request_count) = run_scripted_streaming_turn(
        |index, _request| match index {
            1 => vec![serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": {}})],
            2 => vec![serde_json::json!({"jsonrpc": "2.0", "id": 2, "result": {"sessionId": "s-1"}})],
            3 => vec![serde_json::json!({"jsonrpc": "2.0", "id": 3, "error": {"code": -32000, "message": TRANSIENT_REFUSAL}})],
            4 => vec![serde_json::json!({"jsonrpc": "2.0", "id": 4, "result": {}})],
            5 => vec![serde_json::json!({"jsonrpc": "2.0", "id": 5, "result": {"sessionId": "s-2"}})],
            6 => vec![
                serde_json::json!({"jsonrpc": "2.0", "method": "session/update",
                    "params": {"update": {"sessionUpdate": "agent_message_chunk",
                        "content": {"type": "text", "text": "OK after the retry"}}}}),
                serde_json::json!({"jsonrpc": "2.0", "id": 6, "result": {"stopReason": "end_turn"}}),
            ],
            _ => Vec::new(),
        },
        retry_turn_render("chatcmpl-retry-1"),
    )
    .await;

    assert_eq!(request_count, 6, "the retry re-handshakes and re-prompts");
    assert!(
        !output.contains("\"error\""),
        "the retried turn must not surface the refusal: {output}"
    );
    assert!(
        output.contains("OK after the retry"),
        "the retried turn's text reaches the client: {output}"
    );
    assert!(
        output.contains("\"finish_reason\":\"stop\""),
        "the retried turn completes: {output}"
    );
    assert!(output.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn a_non_transient_turn_error_is_not_retried() {
    let (output, request_count) = run_scripted_streaming_turn(
        |index, _request| match index {
            1 => vec![serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": {}})],
            2 => vec![serde_json::json!({"jsonrpc": "2.0", "id": 2, "result": {"sessionId": "s-1"}})],
            3 => vec![serde_json::json!({"jsonrpc": "2.0", "id": 3, "error": {"code": -32000, "message": "the turn failed for a permanent reason"}})],
            _ => Vec::new(),
        },
        retry_turn_render("chatcmpl-retry-2"),
    )
    .await;

    assert!(
        output.contains("permanent reason"),
        "the original error surfaces: {output}"
    );
    assert_eq!(request_count, 3, "no re-handshake after a permanent error");
    assert!(output.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn a_refusal_after_output_has_streamed_is_not_retried() {
    let (output, request_count) = run_scripted_streaming_turn(
        |index, _request| match index {
            1 => vec![serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": {}})],
            2 => vec![serde_json::json!({"jsonrpc": "2.0", "id": 2, "result": {"sessionId": "s-1"}})],
            3 => vec![
                // Text streamed first, then the refusal: a retry would
                // duplicate what the client already saw.
                serde_json::json!({"jsonrpc": "2.0", "method": "session/update",
                    "params": {"update": {"sessionUpdate": "agent_message_chunk",
                        "content": {"type": "text", "text": "partial answer"}}}}),
                serde_json::json!({"jsonrpc": "2.0", "id": 3, "error": {"code": -32000, "message": TRANSIENT_REFUSAL}}),
            ],
            _ => Vec::new(),
        },
        retry_turn_render("chatcmpl-retry-3"),
    )
    .await;

    assert!(output.contains("partial answer"), "{output}");
    assert!(
        output.contains("please try a different model"),
        "the refusal surfaces after streamed output: {output}"
    );
    assert_eq!(request_count, 3, "no re-handshake once output streamed");
    assert!(output.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn a_transient_model_refusal_is_retried_in_buffered_turns() {
    let (session, _requests, mut child_out) = scripted_session();
    // The refused prompt (id 1, passed in below) never went over the wire, so
    // the session's own id counter is still at 1: the retry's re-initialize
    // also sends id 1, then session/new takes id 2 and the fresh prompt id 3.
    let writer = tokio::spawn(async move {
        for reply in [
            serde_json::json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32000, "message": TRANSIENT_REFUSAL}}),
            serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": {}}),
            serde_json::json!({"jsonrpc": "2.0", "id": 2, "result": {"sessionId": "s-2"}}),
            serde_json::json!({"jsonrpc": "2.0", "method": "session/update",
                "params": {"update": {"sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": "Reply with OK accepted."}}}}),
            serde_json::json!({"jsonrpc": "2.0", "id": 3, "result": {"stopReason": "end_turn"}}),
        ] {
            child_out
                .write_all(json_line(&reply).as_bytes())
                .await
                .unwrap();
        }
        // Keep the output pipe open until the retried turn resolves.
        tokio::time::sleep(Duration::from_secs(8)).await;
    });
    let guard = acquire_test_session_permit().await;
    let mut render = retry_turn_render("chatcmpl-retry-4");
    render.request_timeout = Duration::from_secs(12);
    let completion = buffered_turn(session, guard, 1, render)
        .await
        .expect("the retried buffered turn resolves");
    writer.abort();
    let content = completion["choices"][0]["message"]["content"]
        .as_str()
        .expect("assistant content");
    assert!(content.contains("Reply with OK accepted."), "{content}");
    assert_eq!(completion["choices"][0]["finish_reason"], "stop");
}
