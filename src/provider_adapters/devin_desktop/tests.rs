use super::connect::{
    self, ChatRequestRefs, FieldValue, FrameParser, MetadataRefs, PromptRef, ToolCallRef,
    decode_auth_response, decode_fields, decode_gzip, encode_auth_request, encode_chat_request,
    encode_frame, finish_reason, parse_trailer, resolve_extension_version, resolve_ide_version,
};
use super::transport::run_exchange;
use super::*;
use crate::support::mock_upstream::spawn_upstream;
use axum::Router;
use axum::body::Bytes as AxumBytes;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::post;
use std::sync::Arc;
use std::time::Duration;

const AUTH_ENDPOINT: &str = "/exa.auth_pb.AuthService/GetUserJwt";
const CHAT_ENDPOINT: &str = "/exa.api_server_pb.ApiServerService/GetChatMessage";

fn jwt_response(jwt: &str) -> Vec<u8> {
    let mut out = vec![0x0a, jwt.len() as u8];
    out.extend_from_slice(jwt.as_bytes());
    out
}

fn text_delta_frame(flags: u8, text: &str) -> Vec<u8> {
    let mut payload = vec![0x1a, text.len() as u8];
    payload.extend_from_slice(text.as_bytes());
    encode_frame(&payload, flags)
}

fn usage_frame(input: u64, output: u64) -> Vec<u8> {
    // Usage submessage: fields 2 (input) and 3 (output) as varints.
    let usage = [0x10, input as u8, 0x18, output as u8];
    let mut payload = vec![0x3a, usage.len() as u8];
    payload.extend_from_slice(&usage);
    encode_frame(&payload, 0)
}

fn trailer_frame(json: &str) -> Vec<u8> {
    encode_frame(json.as_bytes(), connect::CONNECT_END_STREAM_FLAG)
}

fn gzip_frame(payload: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(payload).unwrap();
    encode_frame(&encoder.finish().unwrap(), connect::CONNECT_COMPRESSED_FLAG)
}

fn bytes_text(fields: &[super::connect::ProtoField], number: u32) -> String {
    fields
        .iter()
        .filter(|field| field.number == number)
        .find_map(|field| match &field.value {
            FieldValue::Bytes(payload) => Some(String::from_utf8_lossy(payload).into_owned()),
            _ => None,
        })
        .unwrap_or_default()
}

fn varint_value(fields: &[super::connect::ProtoField], number: u32) -> u64 {
    fields
        .iter()
        .find(|field| field.number == number)
        .and_then(|field| match &field.value {
            FieldValue::Varint(value) => Some(*value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing varint field {number}"))
}

fn nested_bytes(fields: &[super::connect::ProtoField], number: u32) -> Vec<u8> {
    fields
        .iter()
        .find(|field| field.number == number)
        .and_then(|field| match &field.value {
            FieldValue::Bytes(payload) => Some(payload.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing bytes field {number}"))
}

#[test]
fn the_chat_request_encodes_the_verified_wire_schema() {
    let tool_calls = [ToolCallRef {
        id: "call-1",
        name: "lookup",
        arguments: "{}",
    }];
    let prompts = [
        PromptRef {
            message_id: "m1",
            source: 1,
            prompt: "hello",
            tool_calls: &[],
            tool_call_id: "",
        },
        PromptRef {
            message_id: "m2",
            source: 2,
            prompt: "doing it",
            tool_calls: &tool_calls,
            tool_call_id: "",
        },
    ];
    let bytes = encode_chat_request(&ChatRequestRefs {
        metadata: MetadataRefs {
            api_key: "key-123",
            session_id: "sess-1",
            user_jwt: "jwt-abc",
        },
        model: "test-model",
        system_prompt: "be brief",
        prompts: &prompts,
        cascade_id: "casc-1",
        tools: &[],
        disable_parallel_tool_calls: false,
        tool_choice: None,
    });
    let fields = decode_fields(&bytes).unwrap();

    assert_eq!(
        varint_value(&fields, 7),
        5,
        "CHAT_MESSAGE_REQUEST_TYPE_CASCADE"
    );
    assert_eq!(bytes_text(&fields, 14), "test-model");
    assert_eq!(bytes_text(&fields, 21), "test-model");
    assert_eq!(bytes_text(&fields, 16), "casc-1");
    assert_eq!(bytes_text(&fields, 2), "be brief");

    // Metadata submessage: the raw key and the JWT ride inside field 1.
    let metadata_fields = decode_fields(&nested_bytes(&fields, 1)).unwrap();
    assert_eq!(bytes_text(&metadata_fields, 3), "key-123");
    assert_eq!(bytes_text(&metadata_fields, 21), "jwt-abc");
    assert_eq!(bytes_text(&metadata_fields, 1), "windsurf");
    assert_eq!(bytes_text(&metadata_fields, 10), "sess-1");

    // The assistant prompt carries its tool call; the user prompt does not.
    let prompt_fields: Vec<Vec<u8>> = fields
        .iter()
        .filter(|field| field.number == 3)
        .map(|field| match &field.value {
            FieldValue::Bytes(payload) => payload.clone(),
            _ => panic!("prompt field is length-delimited"),
        })
        .collect();
    assert_eq!(prompt_fields.len(), 2);
    let assistant = decode_fields(&prompt_fields[1]).unwrap();
    assert_eq!(varint_value(&assistant, 2), 2);
    assert!(assistant.iter().any(|field| field.number == 6));
    let user = decode_fields(&prompt_fields[0]).unwrap();
    assert_eq!(varint_value(&user, 2), 1);
    assert!(!user.iter().any(|field| field.number == 6));
}

#[test]
fn the_auth_request_wraps_metadata_and_the_response_extracts_the_jwt() {
    let bytes = encode_auth_request(&MetadataRefs {
        api_key: "key-123",
        session_id: "sess-1",
        user_jwt: "",
    });
    let fields = decode_fields(&bytes).unwrap();
    let metadata_fields = decode_fields(&nested_bytes(&fields, 1)).unwrap();
    assert_eq!(bytes_text(&metadata_fields, 3), "key-123");

    assert_eq!(
        decode_auth_response(&jwt_response("jwt-abc")).unwrap(),
        "jwt-abc"
    );
    assert!(decode_auth_response(&[]).is_err());
}

#[test]
fn the_frame_parser_reassembles_frames_split_across_chunks() {
    let frames = [
        text_delta_frame(0, "Hello "),
        text_delta_frame(0, "world"),
        trailer_frame("{}"),
    ];
    let mut wire = Vec::new();
    for frame in &frames {
        wire.extend_from_slice(frame);
    }

    let mut parser = FrameParser::new();
    let mut decoded = Vec::new();
    // Feed one byte at a time to prove partial frames buffer correctly.
    for byte in &wire {
        decoded.extend(parser.push(std::slice::from_ref(byte)).unwrap());
    }
    assert_eq!(decoded.len(), 3);
    assert_eq!(
        decoded[0].payload,
        [0x1a, 6]
            .iter()
            .chain(b"Hello ")
            .copied()
            .collect::<Vec<u8>>()
    );
    assert_eq!(
        decoded[1].payload,
        [0x1a, 5]
            .iter()
            .chain(b"world")
            .copied()
            .collect::<Vec<u8>>()
    );
    assert_eq!(
        decoded[2].flags & connect::CONNECT_END_STREAM_FLAG,
        connect::CONNECT_END_STREAM_FLAG
    );
    assert!(!parser.has_pending());
}

#[test]
fn the_frame_parser_rejects_bad_flags_and_oversized_frames() {
    let mut parser = FrameParser::new();
    let mut bad_flags = vec![0x80, 0, 0, 0, 0];
    assert!(parser.push(&bad_flags).is_err());

    let mut oversized = vec![0u8];
    oversized.extend_from_slice(&((connect::MAX_FRAME_BYTES + 1) as u32).to_be_bytes());
    assert!(parser.push(&oversized).is_err());
    let _ = bad_flags.pop();
}

#[test]
fn gzip_frames_round_trip_through_a_hard_output_bound() {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(b"compressed text").unwrap();
    let compressed = encoder.finish().unwrap();
    assert_eq!(decode_gzip(&compressed, 1024).unwrap(), b"compressed text");
    assert!(decode_gzip(&compressed, 4).is_err());
}

#[test]
fn the_trailer_parser_maps_success_error_and_garbage() {
    assert_eq!(parse_trailer(b"{}").unwrap(), None);
    assert_eq!(parse_trailer(b"{\"error\":null}").unwrap(), None);
    let error = parse_trailer(b"{\"error\":{\"code\":\"unavailable\",\"message\":\"down\"}}")
        .unwrap()
        .unwrap();
    assert_eq!(error.0, "unavailable");
    assert_eq!(error.1, "down");
    assert!(parse_trailer(b"not json").is_err());
}

#[test]
fn finish_reason_maps_the_verified_stop_reasons() {
    assert_eq!(finish_reason(0, false), "stop");
    assert_eq!(finish_reason(3, false), "length");
    assert_eq!(finish_reason(11, false), "content_filter");
    assert_eq!(finish_reason(10, false), "tool_calls");
    assert_eq!(finish_reason(0, true), "tool_calls");
}

#[test]
fn version_overrides_fall_back_to_the_verified_defaults() {
    // The env is process-global, so only assert the x.y.z shape here.
    for version in [resolve_ide_version(), resolve_extension_version()] {
        assert_eq!(version.split('.').count(), 3);
    }
}

#[test]
fn message_conversion_rejects_media_and_unknown_roles() {
    let image = message_text(&serde_json::json!([
        {"type": "text", "text": "look"},
        {"type": "image_url", "image_url": {"url": "data:image/png;base64,x"}},
    ]))
    .unwrap_err();
    assert!(image.contains("image_url"), "got: {image}");

    let unknown = convert_messages(&serde_json::json!({
        "messages": [{"role": "moderator", "content": "hi"}],
    }))
    .unwrap_err();
    assert!(unknown.contains("moderator"), "got: {unknown}");
}

#[test]
fn message_conversion_joins_system_parts_and_keeps_history_shapes() {
    let (system, prompts) = convert_messages(&serde_json::json!({
        "messages": [
            {"role": "system", "content": "one"},
            {"role": "developer", "content": "two"},
            {"role": "user", "content": "hello"},
            {"role": "assistant", "content": "", "tool_calls": [
                {"id": "call-1", "type": "function",
                 "function": {"name": "lookup", "arguments": "{\"q\":\"x\"}"}}
            ]},
            {"role": "tool", "tool_call_id": "call-1", "content": "result"},
        ],
    }))
    .unwrap();
    assert_eq!(system, "one\n\ntwo");
    assert_eq!(prompts.len(), 3);
    assert_eq!(prompts[0].source, 1);
    assert_eq!(prompts[1].source, 2);
    assert_eq!(prompts[1].tool_calls.len(), 1);
    assert_eq!(prompts[1].tool_calls[0].0, "call-1");
    assert_eq!(prompts[2].source, 4);
    assert_eq!(prompts[2].tool_call_id, "call-1");

    // A request with no user-visible messages still gets one empty prompt.
    let (_, empty) = convert_messages(&serde_json::json!({
        "messages": [{"role": "system", "content": "only"}],
    }))
    .unwrap();
    assert_eq!(empty.len(), 1);
}

#[test]
fn tool_conversion_enforces_function_tools_and_choice_options() {
    let unsupported = convert_tools(&serde_json::json!({
        "tools": [
            {"type": "function", "function": {"name": "lookup", "parameters": {"type": "object"}}},
            {"type": "web_search"},
        ],
    }))
    .unwrap_err();
    assert!(unsupported.contains("function tools"), "got: {unsupported}");

    let tools = convert_tools(&serde_json::json!({
        "tools": [
            {"type": "function", "function": {
                "name": "lookup", "description": "finds things",
                "parameters": {"type": "object"}, "strict": true}},
        ],
    }))
    .unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "lookup");
    assert_eq!(tools[0].schema_json, "{\"type\":\"object\"}");
    assert!(tools[0].strict);

    assert!(convert_tool_choice(&serde_json::json!({"tool_choice": "auto"})).is_ok());
    assert!(convert_tool_choice(&serde_json::json!({"tool_choice": "none"})).is_ok());
    assert!(convert_tool_choice(&serde_json::json!({"tool_choice": "required"})).is_ok());
    assert!(
        convert_tool_choice(&serde_json::json!({"tool_choice": {"type": "function",
        "function": {"name": "lookup"}}}))
        .is_err()
    );
    assert!(convert_tool_choice(&serde_json::json!({})).is_ok());
}

// --- End-to-end exchange against a scripted mock upstream --------------------

#[derive(Clone, Default)]
struct CapturedChat {
    content_type: Arc<std::sync::Mutex<String>>,
    body: Arc<std::sync::Mutex<Vec<u8>>>,
}

async fn devin_desktop_mock(
    auth_status: axum::http::StatusCode,
    chat_wire: Vec<u8>,
) -> (Router, CapturedChat) {
    let captured = CapturedChat::default();
    let chat_guard = captured.clone();
    let app = Router::new()
        .route(
            AUTH_ENDPOINT,
            post(move || async move {
                (
                    auth_status,
                    [(axum::http::header::CONTENT_TYPE, "application/proto")],
                    jwt_response("jwt-abc"),
                )
                    .into_response()
            }),
        )
        .route(
            CHAT_ENDPOINT,
            post(move |headers: HeaderMap, body: AxumBytes| {
                let guard = chat_guard.clone();
                async move {
                    if let Some(content_type) = headers.get(axum::http::header::CONTENT_TYPE) {
                        *guard.content_type.lock().unwrap() =
                            content_type.to_str().unwrap_or_default().to_owned();
                    }
                    *guard.body.lock().unwrap() = body.to_vec();
                    (
                        [(
                            axum::http::header::CONTENT_TYPE,
                            "application/connect+proto",
                        )],
                        chat_wire.clone(),
                    )
                        .into_response()
                }
            }),
        );
    (app, captured)
}

fn sample_body() -> Value {
    serde_json::json!({
        "messages": [{"role": "user", "content": "Hi"}],
    })
}

async fn mock_endpoints(address: std::net::SocketAddr) -> (String, String) {
    (
        format!("http://{address}{AUTH_ENDPOINT}"),
        format!("http://{address}{CHAT_ENDPOINT}"),
    )
}

async fn collect_stream(response: reqwest::Response) -> String {
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut out = Vec::new();
    while let Some(chunk) = stream.next().await {
        out.extend_from_slice(&chunk.unwrap());
    }
    String::from_utf8(out).unwrap()
}

#[tokio::test]
async fn the_exchange_preflights_auth_then_decodes_the_connect_stream() {
    let mut compressed_payload = vec![0x1a, 5];
    compressed_payload.extend_from_slice(b"world");
    let wire = [
        text_delta_frame(0, "Hello "),
        gzip_frame(&compressed_payload),
        usage_frame(7, 3),
        trailer_frame("{}"),
    ]
    .concat();

    let (router, captured) = devin_desktop_mock(axum::http::StatusCode::OK, wire).await;
    let address = spawn_upstream(router).await;
    let (auth_url, chat_url) = mock_endpoints(address).await;
    let response = run_exchange(
        &reqwest::Client::new(),
        &auth_url,
        &chat_url,
        "key-123",
        "test-model",
        &sample_body(),
        "req-1",
        false,
        Duration::from_secs(10),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), 200);
    let completion: Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert_eq!(completion["object"], "chat.completion");
    assert_eq!(completion["model"], "test-model");
    assert_eq!(
        completion["choices"][0]["message"]["content"],
        "Hello world"
    );
    assert_eq!(completion["choices"][0]["finish_reason"], "stop");
    assert_eq!(completion["usage"]["prompt_tokens"], 7);
    assert_eq!(completion["usage"]["completion_tokens"], 3);

    // The upstream saw the Connect content type, the raw key, and the JWT.
    assert_eq!(
        captured.content_type.lock().unwrap().as_str(),
        "application/connect+proto"
    );
    let sent = decode_fields(&captured.body.lock().unwrap()[5..]).unwrap();
    let metadata_fields = decode_fields(&nested_bytes(&sent, 1)).unwrap();
    assert_eq!(
        bytes_text(&metadata_fields, 3),
        "key-123",
        "the raw key rides in the protobuf Metadata, not a header"
    );
    assert_eq!(bytes_text(&metadata_fields, 21), "jwt-abc");
    let prompt_fields = decode_fields(&nested_bytes(&sent, 3)).unwrap();
    assert_eq!(bytes_text(&prompt_fields, 3), "Hi");
}

#[tokio::test]
async fn streaming_clients_receive_openai_style_sse_frames() {
    let wire = [
        text_delta_frame(0, "Hello "),
        text_delta_frame(0, "world"),
        usage_frame(7, 3),
        trailer_frame("{}"),
    ]
    .concat();

    let (router, _captured) = devin_desktop_mock(axum::http::StatusCode::OK, wire).await;
    let address = spawn_upstream(router).await;
    let (auth_url, chat_url) = mock_endpoints(address).await;
    let response = run_exchange(
        &reqwest::Client::new(),
        &auth_url,
        &chat_url,
        "key-123",
        "test-model",
        &sample_body(),
        "req-1",
        true,
        Duration::from_secs(10),
    )
    .await
    .unwrap();
    assert!(
        response
            .headers()
            .get(http::header::CONTENT_TYPE)
            .is_some_and(|value| value == "text/event-stream")
    );
    let body = collect_stream(response).await;
    let frames: Vec<&str> = body.split("\n\n").filter(|part| !part.is_empty()).collect();
    assert_eq!(frames.last().copied(), Some("data: [DONE]"), "got: {body}");
    let parsed: Vec<Value> = frames[..frames.len() - 1]
        .iter()
        .map(|frame| serde_json::from_str(frame.trim_start_matches("data: ")).unwrap())
        .collect();
    assert_eq!(parsed[0]["choices"][0]["delta"]["role"], "assistant");
    assert_eq!(parsed[1]["choices"][0]["delta"]["content"], "Hello ");
    assert_eq!(parsed[2]["choices"][0]["delta"]["content"], "world");
    let last = parsed.last().unwrap();
    assert_eq!(last["choices"][0]["finish_reason"], "stop");
    assert_eq!(last["usage"]["completion_tokens"], 3);
}

#[tokio::test]
async fn a_trailer_error_becomes_a_transport_failure() {
    let wire = trailer_frame("{\"error\":{\"code\":\"unavailable\",\"message\":\"down\"}}");
    let (router, _captured) = devin_desktop_mock(axum::http::StatusCode::OK, wire).await;
    let address = spawn_upstream(router).await;
    let (auth_url, chat_url) = mock_endpoints(address).await;
    let error = run_exchange(
        &reqwest::Client::new(),
        &auth_url,
        &chat_url,
        "key-123",
        "test-model",
        &sample_body(),
        "req-1",
        false,
        Duration::from_secs(10),
    )
    .await
    .unwrap_err();
    assert!(error.contains("unavailable"), "got: {error}");
}

#[tokio::test]
async fn an_auth_rejection_fails_before_the_chat_exchange() {
    let (router, captured) =
        devin_desktop_mock(axum::http::StatusCode::UNAUTHORIZED, Vec::new()).await;
    let address = spawn_upstream(router).await;
    let (auth_url, chat_url) = mock_endpoints(address).await;
    let error = run_exchange(
        &reqwest::Client::new(),
        &auth_url,
        &chat_url,
        "key-123",
        "test-model",
        &sample_body(),
        "req-1",
        false,
        Duration::from_secs(10),
    )
    .await
    .unwrap_err();
    assert!(error.contains("401"), "got: {error}");
    assert!(captured.body.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_missing_api_key_is_rejected_before_any_network_call() {
    let database = crate::support::test_support::TestDatabase::open().await;
    let state = AppState::new(database.config(), database.db.clone());
    let error = send_local_upstream(
        &state,
        &sample_body(),
        "test-model",
        "req-1",
        false,
        Duration::from_secs(10),
        None,
    )
    .await
    .unwrap_err();
    assert!(error.contains("API key"), "got: {error}");
}

#[test]
fn the_adapter_pins_the_canonical_endpoint_and_protocol() {
    assert_eq!(
        DEVIN_DESKTOP_ADAPTER
            .endpoint(connect::BASE_URL, Protocol::ChatCompletions, None)
            .unwrap()
            .as_str(),
        "https://server.codeium.com/"
    );
    assert!(
        DEVIN_DESKTOP_ADAPTER
            .endpoint("https://evil.example.com", Protocol::ChatCompletions, None)
            .is_err()
    );
    assert!(
        DEVIN_DESKTOP_ADAPTER
            .endpoint(connect::BASE_URL, Protocol::Messages, None)
            .is_err()
    );
}
