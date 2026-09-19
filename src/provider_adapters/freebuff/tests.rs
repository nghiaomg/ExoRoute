//! Freebuff adapter tests: catalog mapping, URL/model strictness, body
//! ownership, lifecycle against a local mock server, and credential probes.
//!
//! The mock server is a raw TCP responder with bounded request reads so the
//! tests exercise real HTTP framing without egress.

use super::catalog::{
    FREEBUFF_BASE_URL, FREEBUFF_CHAT_COMPLETIONS_PATH, FREEBUFF_GLM_V53_FLASH_MODEL_ID,
    FREEBUFF_MODELS, FREEBUFF_PROMPT, MODEL_TO_AGENT, agent_for_model, requested_model,
};
use super::client::{freebuff_client, read_auxiliary_body, send_bounded};
use super::credential_test::test_freebuff_credential_at;
use super::errors::freebuff_session_error;
use super::session::prepare_freebuff_request;
use super::*;
use crate::protocol::{Protocol, UpstreamProtocol};
use crate::provider_adapters::preset_for_adapter;
use crate::state::AppState;
use http::StatusCode;
use serde_json::json;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpListener},
    thread::JoinHandle,
};

struct CapturedRequest {
    request_line: String,
    headers: HashMap<String, String>,
    body: Value,
}

struct MockServer {
    root: String,
    handle: JoinHandle<Vec<CapturedRequest>>,
}

impl MockServer {
    fn join(self) -> Vec<CapturedRequest> {
        self.handle.join().expect("Freebuff mock server thread")
    }
}

fn spawn_mock_server(responses: Vec<(u16, String)>) -> MockServer {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind Freebuff mock server");
    let port = listener
        .local_addr()
        .expect("Freebuff mock server address")
        .port();
    let root = format!("http://127.0.0.1:{port}/api/v1");
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::with_capacity(responses.len());
        for (status, response_body) in responses {
            let (stream, _) = listener.accept().expect("accept Freebuff mock request");
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .expect("set Freebuff mock read timeout");
            let mut reader = BufReader::new(stream);
            let mut request_line = String::new();
            reader
                .read_line(&mut request_line)
                .expect("read Freebuff mock request line");
            let mut headers = HashMap::new();
            loop {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .expect("read Freebuff mock request header");
                if line == "\r\n" || line == "\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
                }
            }
            let content_length = headers
                .get("content-length")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            assert!(content_length <= 1024 * 1024);
            let mut body = vec![0_u8; content_length];
            reader
                .read_exact(&mut body)
                .expect("read Freebuff mock request body");
            let body = serde_json::from_slice(&body).expect("parse Freebuff mock JSON body");
            requests.push(CapturedRequest {
                request_line,
                headers,
                body,
            });

            let reason = match status {
                200 => "OK",
                401 => "Unauthorized",
                403 => "Forbidden",
                409 => "Conflict",
                429 => "Too Many Requests",
                500 => "Internal Server Error",
                _ => "Test Response",
            };
            let response_head = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response_body.len()
            );
            let mut stream = reader.into_inner();
            stream
                .write_all(response_head.as_bytes())
                .expect("write Freebuff mock response headers");
            stream
                .write_all(response_body.as_bytes())
                .expect("write Freebuff mock response body");
            stream.flush().expect("flush Freebuff mock response");
        }
        requests
    });
    MockServer { root, handle }
}

async fn test_state() -> (crate::support::test_support::TestDatabase, AppState) {
    let database = crate::support::test_support::TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some([47_u8; 32]);
    config.allow_private_provider_urls = true;
    let state = AppState::new(config, database.db.clone());
    (database, state)
}

fn test_auth<'a>(secret: &'a str) -> UpstreamAuthContext<'a> {
    UpstreamAuthContext {
        auth_type: "bearer",
        auth_header: None,
        secret: Some(secret),
        oauth_auth: None,
        session_id: "freebuff-test-session",
        protocol: UpstreamProtocol::ChatCompletions,
    }
}

fn test_request_context<'a>(
    state: &'a AppState,
    root: &'a str,
    model: &'a str,
    request_id: &'a str,
    secret: &'a str,
    streaming: bool,
) -> AdapterRequestContext<'a> {
    AdapterRequestContext {
        state,
        base_url: FREEBUFF_BASE_URL,
        adapter_base_url_override: Some(root),
        model,
        request_id,
        streaming,
        auth: test_auth(secret),
    }
}

#[test]
fn catalog_and_agent_mapping_match_freebuff() {
    assert_eq!(FREEBUFF_MODELS.len(), 10);
    assert_eq!(
        MODEL_TO_AGENT,
        &[
            ("deepseek/deepseek-v4-flash", "base2-free-deepseek-flash"),
            ("deepseek/deepseek-v4-pro", "base2-free-deepseek"),
            ("openai/gpt-5.6-luna", "base2-free-luna"),
            ("minimax/minimax-m3", "base2-free-minimax-m3"),
            ("mimo/mimo-v2.5", "base2-free-mimo"),
            ("z-ai/glm-5.2", "base2-free-glm"),
            (FREEBUFF_GLM_V53_FLASH_MODEL_ID, "base2-free-glm-5-3-flash"),
            ("crof/kimi-k3-eco", "base2-free-kimi-k3-eco"),
            ("anthropic/claude-fable-5", "base2-free-fable"),
            ("meta/muse-spark-1.2-contributor", "base2-free-muse-spark"),
        ]
    );
    assert_eq!(
        agent_for_model(FREEBUFF_GLM_V53_FLASH_MODEL_ID),
        "base2-free-glm-5-3-flash"
    );
    assert_eq!(agent_for_model("manual/model"), "base2-free");
}

#[test]
fn endpoint_and_model_prefixes_are_strict() {
    assert_eq!(
        FREEBUFF_ADAPTER
            .endpoint(FREEBUFF_BASE_URL, Protocol::ChatCompletions, None)
            .expect("Freebuff completion endpoint")
            .as_str(),
        "https://www.codebuff.com/api/v1/chat/completions"
    );
    assert!(
        FREEBUFF_ADAPTER
            .endpoint(FREEBUFF_BASE_URL, Protocol::Responses, None)
            .is_err()
    );
    assert!(
        FREEBUFF_ADAPTER
            .endpoint(
                "http://www.codebuff.com/api/v1",
                Protocol::ChatCompletions,
                None
            )
            .is_err()
    );
    assert_eq!(
        requested_model("fb/deepseek/deepseek-v4-flash"),
        "deepseek/deepseek-v4-flash"
    );
    assert_eq!(requested_model("freebuff/manual/model"), "manual/model");
}

#[test]
fn prompt_and_internal_metadata_are_not_client_overridable() {
    let mut body = json!({
        "model":"fb/deepseek/deepseek-v4-flash",
        "messages":[
            {"role":"user","content":"hello"},
            null
        ],
        "codebuff_metadata":{
            "cost_mode":"paid",
            "client_id":"client-controlled",
            "freebuff_instance_id":"fake",
            "run_id":"fake-run",
            "custom":"kept"
        }
    });
    super::body::prepare_freebuff_body(
        &mut body,
        "deepseek/deepseek-v4-flash",
        true,
        "exoroute-client",
        "instance-1",
        Some("run-1"),
    )
    .expect("Freebuff body preparation");
    assert_eq!(body["messages"].as_array().map(Vec::len), Some(2));
    assert_eq!(body["messages"][0]["content"], FREEBUFF_PROMPT);
    assert_eq!(body["codebuff_metadata"]["cost_mode"], "free");
    assert_eq!(body["codebuff_metadata"]["client_id"], "exoroute-client");
    assert_eq!(
        body["codebuff_metadata"]["freebuff_instance_id"],
        "instance-1"
    );
    assert_eq!(body["codebuff_metadata"]["run_id"], "run-1");
    assert_eq!(body["codebuff_metadata"]["custom"], "kept");

    super::body::prepare_freebuff_body(
        &mut body,
        "deepseek/deepseek-v4-flash",
        false,
        "exoroute-client",
        "instance-1",
        None,
    )
    .expect("second Freebuff body preparation");
    assert_eq!(body["messages"].as_array().map(Vec::len), Some(2));
    assert_eq!(body["codebuff_metadata"].get("run_id"), None);
}

#[test]
fn config_requires_bearer_and_chat_completions() {
    let preset = preset_for_adapter(FREEBUFF_ADAPTER_ID).expect("Freebuff preset");
    assert!(
        FREEBUFF_ADAPTER
            .validate_config(
                preset,
                "bearer",
                FREEBUFF_BASE_URL,
                "chat_completions",
                &["chat_completions".to_owned()],
                true,
            )
            .is_ok()
    );
    assert!(
        FREEBUFF_ADAPTER
            .validate_config(
                preset,
                "header",
                FREEBUFF_BASE_URL,
                "chat_completions",
                &["chat_completions".to_owned()],
                true,
            )
            .is_err()
    );
}

#[tokio::test]
async fn key_validation_rejects_invalid_input_before_network() {
    let (_database, state) = test_state().await;
    let cases = [
        (
            FREEBUFF_BASE_URL,
            "bearer",
            "chat_completions",
            "",
            "Freebuff Auth Token required",
        ),
        (
            FREEBUFF_BASE_URL,
            "header",
            "chat_completions",
            "token",
            "Freebuff requires Bearer authentication",
        ),
        (
            FREEBUFF_BASE_URL,
            "bearer",
            "responses",
            "token",
            "Freebuff supports Chat Completions upstream only",
        ),
        (
            "https://codebuff.com/api/v1",
            "bearer",
            "chat_completions",
            "token",
            "Freebuff uses https://www.codebuff.com/api/v1",
        ),
    ];
    for (base_url, auth_type, protocol, credential, message) in cases {
        let outcome = test_freebuff_credential_at(
            &state,
            base_url,
            auth_type,
            protocol,
            &std::collections::BTreeMap::new(),
            credential,
            None,
        )
        .await;
        assert!(!outcome.test_passed);
        assert_eq!(outcome.message, message);
    }
}

#[tokio::test]
async fn lifecycle_uses_expected_paths_headers_and_body() {
    let (_database, state) = test_state().await;
    let server = spawn_mock_server(vec![
        (200, r#"{"instanceId":"instance-1"}"#.to_owned()),
        (200, r#"{"runId":"run-1"}"#.to_owned()),
        (
            200,
            r#"{"id":"chatcmpl-test","object":"chat.completion","choices":[]}"#.to_owned(),
        ),
        (200, "{}".to_owned()),
    ]);
    let secret = "test-secret";
    let mut body = json!({
        "model": "fb/deepseek/deepseek-v4-flash",
        "messages": [{"role":"user","content":"hello"}],
        "stream": false,
        "codebuff_metadata": {
            "cost_mode": "paid",
            "client_id": "client-controlled",
            "freebuff_instance_id": "client-instance",
            "run_id": "client-run",
            "custom": "kept"
        }
    });
    let preparation = prepare_freebuff_request(
        test_request_context(
            &state,
            &server.root,
            "fb/deepseek/deepseek-v4-flash",
            "request-1",
            secret,
            true,
        ),
        &mut body,
    )
    .await
    .expect("prepare Freebuff lifecycle request");

    assert_eq!(body["model"], "deepseek/deepseek-v4-flash");
    assert_eq!(body["stream"], true);
    assert_eq!(body["messages"][0]["content"], FREEBUFF_PROMPT);
    assert_eq!(
        body["messages"]
            .as_array()
            .expect("Freebuff messages")
            .iter()
            .filter(|message| message["content"] == FREEBUFF_PROMPT)
            .count(),
        1
    );
    assert_eq!(body["codebuff_metadata"]["cost_mode"], "free");
    assert_eq!(body["codebuff_metadata"]["client_id"], "exoroute-request-1");
    assert_eq!(
        body["codebuff_metadata"]["freebuff_instance_id"],
        "instance-1"
    );
    assert_eq!(body["codebuff_metadata"]["run_id"], "run-1");
    assert_eq!(body["codebuff_metadata"]["custom"], "kept");
    assert_eq!(
        preparation
            .headers
            .get("x-freebuff-instance-id")
            .and_then(|value| value.to_str().ok()),
        Some("instance-1")
    );
    assert_eq!(
        preparation
            .headers
            .get("x-codebuff-agent-id")
            .and_then(|value| value.to_str().ok()),
        Some("base2-free-deepseek-flash")
    );
    assert_eq!(
        preparation
            .headers
            .get("x-codebuff-run-id")
            .and_then(|value| value.to_str().ok()),
        Some("run-1")
    );

    let completion_endpoint = endpoint_from_root(&server.root, FREEBUFF_CHAT_COMPLETIONS_PATH)
        .expect("Freebuff completion endpoint");
    let (completion_endpoint, client) = freebuff_client(&state, &completion_endpoint)
        .await
        .expect("Freebuff completion client");
    let mut request = client
        .post(completion_endpoint)
        .bearer_auth(secret)
        .json(&body);
    for (name, value) in &preparation.headers {
        request = request.header(name, value);
    }
    let upstream = state.operational_settings().settings.upstream;
    let response = send_bounded(
        request,
        "test completion",
        upstream.freebuff_auxiliary_timeout,
    )
    .await
    .expect("Freebuff completion response");
    let status = response.status();
    read_auxiliary_body(
        response,
        upstream.freebuff_auxiliary_timeout,
        upstream.freebuff_auxiliary_response_max_bytes,
    )
    .await
    .expect("read Freebuff completion response");
    super::finish::finish_freebuff_agent_run(&state, preparation, test_auth(secret), Some(status))
        .await;

    let requests = server.join();
    assert_eq!(requests.len(), 4);
    let paths = requests
        .iter()
        .map(|request| {
            request
                .request_line
                .split_whitespace()
                .nth(1)
                .expect("Freebuff mock request path")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        vec![
            "/api/v1/freebuff/session",
            "/api/v1/agent-runs",
            "/api/v1/chat/completions",
            "/api/v1/agent-runs",
        ]
    );
    assert_eq!(requests[0].body, json!({}));
    assert_eq!(
        requests[0].headers.get("authorization").map(String::as_str),
        Some("Bearer test-secret")
    );
    assert_eq!(
        requests[0]
            .headers
            .get("x-freebuff-model")
            .map(String::as_str),
        Some("deepseek/deepseek-v4-flash")
    );
    assert_eq!(
        requests[1].body,
        json!({"action":"START","agentId":"base2-free-deepseek-flash"})
    );
    assert_eq!(
        requests[2]
            .headers
            .get("x-freebuff-instance-id")
            .map(String::as_str),
        Some("instance-1")
    );
    assert_eq!(
        requests[2]
            .headers
            .get("x-codebuff-agent-id")
            .map(String::as_str),
        Some("base2-free-deepseek-flash")
    );
    assert_eq!(
        requests[2]
            .headers
            .get("x-codebuff-run-id")
            .map(String::as_str),
        Some("run-1")
    );
    assert_eq!(
        requests[2].headers.get("accept").map(String::as_str),
        Some(super::catalog::FREEBUFF_COMPLETION_ACCEPT)
    );
    assert_eq!(requests[3].body["action"], "FINISH");
    assert_eq!(requests[3].body["runId"], "run-1");
    assert_eq!(requests[3].body["status"], "completed");
}

#[tokio::test]
async fn session_is_reused_for_same_credential_and_model() {
    let (_database, state) = test_state().await;
    let server = spawn_mock_server(vec![
        (200, r#"{"instanceId":"instance-1"}"#.to_owned()),
        (200, r#"{"runId":"run-1"}"#.to_owned()),
        (200, r#"{"runId":"run-2"}"#.to_owned()),
    ]);
    for request_id in ["request-cache-1", "request-cache-2"] {
        let mut body = json!({"messages":[{"role":"user","content":"hello"}]});
        let preparation = prepare_freebuff_request(
            test_request_context(
                &state,
                &server.root,
                "fb/deepseek/deepseek-v4-flash",
                request_id,
                "test-secret",
                false,
            ),
            &mut body,
        )
        .await
        .expect("Freebuff session should be prepared");
        assert_eq!(
            preparation
                .headers
                .get("x-freebuff-instance-id")
                .and_then(|value| value.to_str().ok()),
            Some("instance-1")
        );
    }
    let requests = server.join();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.request_line.contains("/freebuff/session"))
            .count(),
        1
    );
    assert_eq!(requests[1].body["action"], "START");
    assert_eq!(requests[2].body["action"], "START");
}

#[tokio::test]
async fn cached_session_rejects_switching_models_without_network_retry() {
    let (_database, state) = test_state().await;
    let server = spawn_mock_server(vec![
        (200, r#"{"instanceId":"instance-1"}"#.to_owned()),
        (200, r#"{"runId":"run-1"}"#.to_owned()),
    ]);
    let mut first_body = json!({"messages":[]});
    prepare_freebuff_request(
        test_request_context(
            &state,
            &server.root,
            "deepseek/deepseek-v4-flash",
            "request-cache-model-1",
            "test-secret",
            false,
        ),
        &mut first_body,
    )
    .await
    .expect("initial Freebuff session should be prepared");

    let mut second_body = json!({"messages":[]});
    let result = prepare_freebuff_request(
        test_request_context(
            &state,
            &server.root,
            FREEBUFF_GLM_V53_FLASH_MODEL_ID,
            "request-cache-model-2",
            "test-secret",
            false,
        ),
        &mut second_body,
    )
    .await;
    let error = match result {
        Ok(_) => panic!("Freebuff must not switch a cached session model"),
        Err(error) => error,
    };
    assert_eq!(error.status, Some(StatusCode::CONFLICT));
    assert!(error.message.contains("deepseek/deepseek-v4-flash"));
    assert!(error.message.contains(FREEBUFF_GLM_V53_FLASH_MODEL_ID));
    assert_eq!(server.join().len(), 2);
}

#[tokio::test]
async fn agent_start_failure_does_not_block_completion() {
    let (_database, state) = test_state().await;
    let server = spawn_mock_server(vec![
        (200, r#"{"instanceId":"instance-1"}"#.to_owned()),
        (500, r#"{"error":"unavailable"}"#.to_owned()),
        (200, r#"{"id":"chatcmpl-test","choices":[]}"#.to_owned()),
    ]);
    let mut body = json!({"messages":[{"role":"user","content":"hello"}]});
    let preparation = prepare_freebuff_request(
        test_request_context(
            &state,
            &server.root,
            "freebuff/manual/model",
            "request-2",
            "test-secret",
            false,
        ),
        &mut body,
    )
    .await
    .expect("Freebuff request remains usable after START failure");
    assert!(preparation.finish_endpoint.is_none());
    assert!(preparation.finish_run_id.is_none());

    let completion_endpoint = endpoint_from_root(&server.root, FREEBUFF_CHAT_COMPLETIONS_PATH)
        .expect("Freebuff completion endpoint");
    let (completion_endpoint, client) = freebuff_client(&state, &completion_endpoint)
        .await
        .expect("Freebuff completion client");
    let response = client
        .post(completion_endpoint)
        .bearer_auth("test-secret")
        .json(&body)
        .send()
        .await
        .expect("Freebuff completion after START failure");
    assert!(response.status().is_success());
    let requests = server.join();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].body["action"], "START");
    assert_eq!(
        requests[2].request_line.split_whitespace().nth(1),
        Some("/api/v1/chat/completions")
    );
}

#[tokio::test]
async fn key_validation_handles_accepted_and_rejected_session_statuses() {
    let (_database, state) = test_state().await;
    for (status, expected_passed) in [
        (200, true),
        (409, true),
        (401, false),
        (403, false),
        (429, false),
    ] {
        let body = if status == 200 {
            r#"{"instanceId":"instance-1"}"#
        } else {
            "{}"
        };
        let server = spawn_mock_server(vec![(status, body.to_owned())]);
        let outcome = test_freebuff_credential_at(
            &state,
            FREEBUFF_BASE_URL,
            "bearer",
            "chat_completions",
            &std::collections::BTreeMap::new(),
            "test-secret",
            Some(&server.root),
        )
        .await;
        assert_eq!(outcome.test_passed, expected_passed, "HTTP {status}");
        assert_eq!(outcome.status, Some(status), "HTTP {status}");
        if status == 401 || status == 403 {
            assert_eq!(outcome.message, "Invalid or expired Freebuff Auth Token");
        }
        if status == 429 {
            assert_eq!(outcome.message, "Freebuff validation returned HTTP 429");
        }
        assert_eq!(server.join().len(), 1);
    }
}

#[tokio::test]
async fn session_response_is_bounded_and_validated() {
    let (_database, state) = test_state().await;
    for response_body in [
        "not-json".to_owned(),
        "{}".to_owned(),
        format!(
            r#"{{"instanceId":"{}"}}"#,
            "x".repeat(
                crate::config::UpstreamSettings::default().freebuff_auxiliary_response_max_bytes,
            )
        ),
    ] {
        let server = spawn_mock_server(vec![(200, response_body)]);
        let mut body = json!({"messages":[]});
        let result = prepare_freebuff_request(
            test_request_context(
                &state,
                &server.root,
                "fb/manual/model",
                "request-3",
                "test-secret",
                false,
            ),
            &mut body,
        )
        .await;
        let error = match result {
            Ok(_) => panic!("invalid or oversized Freebuff session response was accepted"),
            Err(error) => error,
        };
        assert_eq!(error.status, Some(StatusCode::BAD_GATEWAY));
        assert!(!error.message.is_empty());
        assert_eq!(server.join().len(), 1);
    }
}

#[tokio::test]
async fn session_conflict_preserves_sanitized_upstream_diagnostic() {
    let (_database, state) = test_state().await;
    let server = spawn_mock_server(vec![(
        409,
        r#"{
                "status":"model_locked",
                "currentModel":"deepseek/deepseek-v4-flash",
                "requestedModel":"z-ai/glm-5.3-flash",
                "token":"do-not-return-this"
            }"#
        .to_owned(),
    )]);
    let mut body = json!({"messages":[]});
    let result = prepare_freebuff_request(
        test_request_context(
            &state,
            &server.root,
            FREEBUFF_GLM_V53_FLASH_MODEL_ID,
            "request-conflict",
            "test-secret",
            false,
        ),
        &mut body,
    )
    .await;
    let error = match result {
        Ok(_) => panic!("model-locked session must reject the request"),
        Err(error) => error,
    };

    assert_eq!(error.status, Some(StatusCode::CONFLICT));
    assert!(error.message.contains("deepseek/deepseek-v4-flash"));
    assert!(error.message.contains("z-ai/glm-5.3-flash"));
    let provider_response_body = error
        .provider_response_body
        .as_deref()
        .expect("sanitized Freebuff body");
    assert!(provider_response_body.contains("model_locked"));
    assert!(provider_response_body.contains("[redacted]"));
    assert!(!provider_response_body.contains("do-not-return-this"));
    assert_eq!(server.join().len(), 1);
}

#[test]
fn session_error_falls_back_to_bounded_sanitized_body() {
    let body = format!(
        r#"{{"message":"{}"}}"#,
        "x".repeat(
            crate::config::UpstreamSettings::default().freebuff_auxiliary_response_max_bytes,
        )
    );
    let (message, provider_response_body) =
        freebuff_session_error(StatusCode::CONFLICT, body.as_bytes());
    assert!(message.starts_with("Freebuff session returned HTTP 409: "));
    assert!(provider_response_body.is_some_and(|body| body.chars().count() <= 8 * 1024 + 1));
}
