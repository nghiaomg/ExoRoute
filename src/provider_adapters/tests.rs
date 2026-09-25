//! Tests for provider adapter registry, capabilities, and dispatch.
use super::*;

use crate::{
    infra::storage::{Field, Record, StorageError},
    security,
    support::test_support::{ProviderSeed, TestDatabase, seed_provider},
};
use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    task::JoinHandle,
};

async fn test_state() -> (TestDatabase, AppState) {
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some([47_u8; 32]);
    config.allow_private_provider_urls = true;
    let state = AppState::new(config, database.db.clone());
    (database, state)
}

#[test]
fn custom_headers_are_applied_to_provider_requests() {
    let headers = BTreeMap::from([
        ("x-test".to_owned(), "bla bla".to_owned()),
        ("x-test-b".to_owned(), "blue blue".to_owned()),
    ]);
    let request = apply_custom_headers(
        reqwest::Client::new().get("https://api.example.com/v1/models"),
        &headers,
    )
    .expect("custom headers are valid")
    .build()
    .expect("provider request builds");

    assert_eq!(request.headers().get("x-test").unwrap(), "bla bla");
    assert_eq!(request.headers().get("x-test-b").unwrap(), "blue blue");
}

async fn start_mock_server(
    responses: Vec<(u16, Vec<u8>)>,
) -> (String, JoinHandle<Vec<(String, Option<String>)>>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("mock listener");
    let address = listener.local_addr().expect("mock address");
    let server = tokio::spawn(async move {
        let mut requests = Vec::with_capacity(responses.len());
        for (status, body) in responses {
            let (stream, _) = listener.accept().await.expect("mock request");
            let mut stream = BufReader::new(stream);
            let mut request_line = String::new();
            stream
                .read_line(&mut request_line)
                .await
                .expect("request line");
            let mut authorization = None;
            loop {
                let mut line = String::new();
                stream.read_line(&mut line).await.expect("request header");
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some((name, value)) = line.trim_end().split_once(':')
                    && name.eq_ignore_ascii_case("authorization")
                {
                    authorization = Some(value.trim().to_owned());
                }
            }
            let reason = if (200..300).contains(&status) {
                "OK"
            } else {
                "Unavailable"
            };
            let headers = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream
                .get_mut()
                .write_all(headers.as_bytes())
                .await
                .expect("mock response headers");
            stream
                .get_mut()
                .write_all(&body)
                .await
                .expect("mock response body");
            requests.push((request_line.trim_end().to_owned(), authorization));
        }
        requests
    });
    (format!("http://{address}"), server)
}

#[test]
fn codex_empty_completed_response_is_rejected_before_non_stream_failover() {
    let error = parse_adapter_event_stream(
        br#"event: response.completed
data: {"type":"response.completed","response":{"status":"completed","output":[]}}

"#,
    )
    .expect_err("empty Codex completion must not be accepted");
    assert!(
        error.message.contains("without content"),
        "{}",
        error.message
    );
    assert!(error.safe_to_fail_over);

    let response = parse_adapter_event_stream(
        br#"event: response.completed
data: {"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}}

"#,
    )
    .expect("non-empty Codex completion remains valid");
    assert_eq!(response["output"][0]["content"][0]["text"], "ok");
}

#[test]
fn codex_completed_response_reconstructs_output_from_incremental_events() {
    let response = parse_adapter_event_stream(
        br#"event: response.output_item.added
data: {"type":"response.output_item.added","output_index":0,"item":{"id":"msg_1","type":"message","role":"assistant","content":[]}}

event: response.output_text.delta
data: {"type":"response.output_text.delta","item_id":"msg_1","output_index":0,"content_index":0,"delta":"OK"}

event: response.output_text.done
data: {"type":"response.output_text.done","item_id":"msg_1","output_index":0,"content_index":0,"text":"OK."}

event: response.output_item.done
data: {"type":"response.output_item.done","output_index":0,"item":{"id":"msg_1","type":"message","role":"assistant","content":[]}}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_1","model":"gpt-5.6-luna","status":"completed","output":[]}}

"#,
    )
    .expect("incremental Codex output should complete successfully");

    assert_eq!(response["output"][0]["id"], "msg_1");
    assert_eq!(response["output"][0]["content"][0]["text"], "OK.");
}

#[test]
fn codex_completed_response_reconstructs_custom_tool_call_input() {
    // Codex freeform tools such as apply_patch stream their patch through
    // custom_tool_call_input events and complete with an empty-input item.
    let response = parse_adapter_event_stream(
        br#"event: response.output_item.added
data: {"type":"response.output_item.added","output_index":0,"item":{"id":"ctc_1","type":"custom_tool_call","status":"in_progress","call_id":"call_1","name":"apply_patch","input":""}}

event: response.custom_tool_call_input.delta
data: {"type":"response.custom_tool_call_input.delta","item_id":"ctc_1","output_index":0,"delta":"*** Begin Patch\n"}

event: response.custom_tool_call_input.delta
data: {"type":"response.custom_tool_call_input.delta","item_id":"ctc_1","output_index":0,"delta":"*** End Patch"}

event: response.custom_tool_call_input.done
data: {"type":"response.custom_tool_call_input.done","item_id":"ctc_1","output_index":0,"input":"*** Begin Patch\n*** End Patch"}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_1","model":"gpt-5.6-luna","status":"completed","output":[]}}

"#,
    )
    .expect("incremental Codex custom tool call should complete successfully");

    assert_eq!(response["output"][0]["type"], "custom_tool_call");
    assert_eq!(response["output"][0]["call_id"], "call_1");
    assert_eq!(response["output"][0]["name"], "apply_patch");
    assert_eq!(
        response["output"][0]["input"],
        "*** Begin Patch\n*** End Patch"
    );
}

#[test]
fn codex_completed_response_reconstructs_function_call_arguments() {
    let response = parse_adapter_event_stream(
        br#"event: response.output_item.added
data: {"type":"response.output_item.added","output_index":0,"item":{"id":"fc_1","type":"function_call","status":"in_progress","call_id":"call_1","name":"lookup","arguments":""}}

event: response.function_call_arguments.delta
 data: {"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":0,"delta":"{\"query\":\"sta"}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":0,"delta":"tus\"}"}

event: response.function_call_arguments.done
data: {"type":"response.function_call_arguments.done","item_id":"fc_1","output_index":0,"arguments":"{\"query\":\"status\"}"}

event: response.output_item.done
data: {"type":"response.output_item.done","output_index":0,"item":{"id":"fc_1","type":"function_call","status":"completed","call_id":"call_1","name":"lookup","arguments":""}}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_1","model":"gpt-5.6-luna","status":"completed","output":[{"id":"fc_1","type":"function_call","status":"completed","call_id":"call_1","name":"lookup","arguments":""}]}}

"#,
    )
    .expect("incremental Codex function call should complete successfully");

    assert_eq!(response["output"][0]["type"], "function_call");
    assert_eq!(response["output"][0]["call_id"], "call_1");
    assert_eq!(response["output"][0]["name"], "lookup");
    assert_eq!(response["output"][0]["arguments"], "{\"query\":\"status\"}");
}

#[test]
fn only_codex_can_accept_a_missing_event_stream_content_type() {
    assert!(
        adapter(CODEX_ADAPTER_ID)
            .expect("Codex adapter")
            .allows_missing_event_stream_content_type()
    );
    assert!(
        !adapter(GENERIC_ADAPTER_ID)
            .expect("generic adapter")
            .allows_missing_event_stream_content_type()
    );
}

#[test]
fn registry_has_unique_ids_and_valid_metadata() {
    let mut adapter_ids = HashSet::new();
    for adapter in registry::adapters() {
        let adapter_id = adapter.adapter_id();
        assert!(adapter_ids.insert(adapter_id), "duplicate adapter ID");
        assert!(preset_for_adapter(adapter_id).is_some());
    }
    let mut preset_ids = HashSet::new();
    for preset in presets() {
        assert!(preset_ids.insert(preset.id), "duplicate preset ID");
        let registered_adapter = adapter(preset.adapter_id).expect("registered preset adapter");
        assert!(!preset.name.trim().is_empty());
        assert!(!preset.default_supported_protocols.is_empty());
        assert!(
            preset
                .default_supported_protocols
                .contains(&preset.default_preferred_protocol)
        );
        assert!(
            preset
                .supported_auth_types
                .contains(&preset.default_auth_type)
        );
        assert!(preset.default_supported_protocols.iter().all(|protocol| {
            let parsed: Result<UpstreamProtocol, _> = protocol.parse();
            crate::protocol::UpstreamProtocol::as_str_list().contains(protocol) && parsed.is_ok()
        }));
        assert!(preset.capabilities.api_keys || preset.capabilities.oauth_accounts);
        assert_eq!(
            preset.capabilities.oauth_accounts || preset.capabilities.api_key_auth_assist,
            preset.capabilities.auth_panel.is_some()
        );
        if let Some(panel) = preset.capabilities.auth_panel {
            assert!(
                crate::provider_adapters::auth::panels::is_known(panel),
                "unknown auth panel"
            );
            assert!(
                auth_panel_is_known(panel),
                "auth panel contract is inconsistent"
            );
            assert!(
                known_auth_panels().contains(&panel),
                "auth panel contract is inconsistent"
            );
        }
        if preset.capabilities.api_key_auth_assist {
            assert!(!registered_adapter.api_key_auth_assist_origins().is_empty());
        }
        for sibling in all_presets().filter(|sibling| sibling.adapter_id == preset.adapter_id) {
            assert_eq!(sibling.capabilities, preset.capabilities);
            assert_eq!(sibling.supported_auth_types, preset.supported_auth_types);
        }
    }
    assert!(adapter_ids.contains(GENERIC_ADAPTER_ID));
    assert!(adapter_ids.contains(KILO_GATEWAY_ADAPTER_ID));
    assert!(adapter_ids.contains(CODEX_ADAPTER_ID));
    assert!(adapter_ids.contains(COMMAND_CODE_ADAPTER_ID));
    assert!(adapter_ids.contains(OPENCODE_GO_ADAPTER_ID));
    assert!(adapter_ids.contains(OPENCODE_ZEN_ADAPTER_ID));
    assert!(adapter_ids.contains(OPENROUTER_ADAPTER_ID));
    assert!(adapter_ids.contains(NVIDIA_NIM_ADAPTER_ID));
    assert!(adapter_ids.contains(FREEBUFF_ADAPTER_ID));
    assert!(adapter_ids.contains(ANTIGRAVITY_ADAPTER_ID));
    assert!(adapter_ids.contains(CLINE_ADAPTER_ID));
    assert!(adapter_ids.contains(CLINEPASS_ADAPTER_ID));
    assert_eq!(
        preset_for_adapter(CLINE_ADAPTER_ID)
            .expect("Cline preset")
            .default_model_prefix,
        Some("cl")
    );
    assert_eq!(
        preset_for_adapter(CLINEPASS_ADAPTER_ID)
            .expect("ClinePass preset")
            .default_model_prefix,
        Some("cp")
    );
    assert!(
        capabilities(CLINE_ADAPTER_ID)
            .expect("Cline capability")
            .local_usage_meter
    );
    assert!(
        capabilities(CLINEPASS_ADAPTER_ID)
            .expect("ClinePass capability")
            .local_usage_meter
    );
    assert!(capabilities(CODEX_ADAPTER_ID).unwrap().usage_limits);
    assert!(!capabilities(GENERIC_ADAPTER_ID).unwrap().usage_limits);
    assert!(preset_ids.contains("custom"));
    assert!(preset_ids.contains("kilo_gateway"));
    assert!(preset_ids.contains("openai_codex"));
    assert!(preset_ids.contains("command_code"));
    assert!(preset_ids.contains("opencode_go"));
    assert!(preset_ids.contains("opencode_zen"));
    assert!(preset_ids.contains("openrouter"));
    assert!(preset_ids.contains("nvidia_nim"));
    assert!(preset_ids.contains("freebuff"));
    assert_eq!(
        capabilities(OPENCODE_GO_ADAPTER_ID)
            .expect("OpenCode Go capability")
            .api_key_usage_status,
        "unverified"
    );
    assert!(
        capabilities(OPENCODE_GO_ADAPTER_ID)
            .expect("OpenCode Go capability")
            .api_key_usage
    );
    assert_eq!(
        capabilities(OPENCODE_ZEN_ADAPTER_ID)
            .expect("OpenCode Zen capability")
            .api_key_usage_status,
        "unverified"
    );
    assert!(
        !capabilities(OPENCODE_ZEN_ADAPTER_ID)
            .expect("OpenCode Zen capability")
            .api_key_usage
    );
    for adapter_id in [OPENCODE_GO_ADAPTER_ID, OPENCODE_ZEN_ADAPTER_ID] {
        assert!(
            capabilities(adapter_id)
                .expect("OpenCode capability")
                .model_protocol_routing,
            "OpenCode adapters route by documented per-model upstream protocol"
        );
    }
    let openrouter_capabilities =
        capabilities(OPENROUTER_ADAPTER_ID).expect("OpenRouter capability");
    assert!(openrouter_capabilities.api_keys);
    assert!(openrouter_capabilities.model_discovery);
    assert!(openrouter_capabilities.api_key_usage);
    assert_eq!(openrouter_capabilities.api_key_usage_status, "supported");
    assert!(openrouter_capabilities.model_catalog_authoritative);
    assert!(!openrouter_capabilities.model_protocol_routing);
    assert!(!openrouter_capabilities.usage_limits);
    let nvidia_capabilities = capabilities(NVIDIA_NIM_ADAPTER_ID).expect("NVIDIA NIM capability");
    assert!(nvidia_capabilities.api_keys);
    assert!(nvidia_capabilities.model_discovery);
    assert!(nvidia_capabilities.local_quota_tracking);
    assert!(!nvidia_capabilities.model_catalog_authoritative);
    assert_eq!(
        nvidia_capabilities.supported_upstream_protocols,
        ["chat_completions"]
    );
    let freebuff_capabilities = capabilities(FREEBUFF_ADAPTER_ID).expect("Freebuff capability");
    assert!(freebuff_capabilities.api_keys);
    assert!(freebuff_capabilities.model_discovery);
    assert!(!freebuff_capabilities.local_usage_meter);
    assert!(!freebuff_capabilities.local_quota_tracking);
    assert!(!freebuff_capabilities.usage_limits);
    assert!(!freebuff_capabilities.api_key_usage);
    assert_eq!(freebuff_capabilities.api_key_usage_status, "unsupported");
    assert!(!freebuff_capabilities.model_catalog_authoritative);
    assert_eq!(
        freebuff_capabilities.supported_upstream_protocols,
        ["chat_completions"]
    );
    assert!(!retry_upstream_response_as_key_rejection(
        FREEBUFF_ADAPTER_ID
    ));
    assert!(retry_upstream_response_as_key_rejection(GENERIC_ADAPTER_ID));
    assert_eq!(
        preset_for_adapter(FREEBUFF_ADAPTER_ID)
            .expect("Freebuff preset")
            .default_model_prefix,
        Some("fb")
    );
    let antigravity_capabilities =
        capabilities(ANTIGRAVITY_ADAPTER_ID).expect("Antigravity capability");
    assert!(!antigravity_capabilities.api_keys);
    assert!(antigravity_capabilities.oauth_accounts);
    assert!(antigravity_capabilities.model_discovery);
    assert!(antigravity_capabilities.usage_limits);
    assert!(!antigravity_capabilities.local_quota_tracking);
    assert!(!antigravity_capabilities.model_catalog_authoritative);
    assert_eq!(
        antigravity_capabilities.supported_upstream_protocols,
        ["google_generate_content"]
    );
    assert_eq!(
        preset_for_adapter(ANTIGRAVITY_ADAPTER_ID)
            .expect("Antigravity preset")
            .default_model_prefix,
        Some("ag")
    );
    assert_eq!(
        preset_for_adapter(ANTIGRAVITY_ADAPTER_ID)
            .expect("Antigravity preset")
            .default_auth_type,
        "antigravity_oauth"
    );
    assert!(supports_custom_oauth_redirect_uri(ANTIGRAVITY_ADAPTER_ID));
    assert!(!supports_custom_oauth_redirect_uri(CODEX_ADAPTER_ID));
}

#[test]
fn yaml_catalog_preserves_builtin_provider_defaults() {
    let presets = presets();
    let expected = [
        (
            "kilo_gateway",
            "Kilo AI Gateway",
            Some("https://api.kilo.ai/api/gateway"),
            ProviderCategory::Gateway,
        ),
        (
            "openai_codex",
            "OpenAI Codex",
            Some(codex_oauth::BASE_URL),
            ProviderCategory::Oauth,
        ),
        (
            "command_code",
            "Command Code",
            Some("https://api.commandcode.ai"),
            ProviderCategory::Gateway,
        ),
        (
            "opencode_go",
            "OpenCode Go",
            Some("https://opencode.ai/zen/go/v1"),
            ProviderCategory::Gateway,
        ),
        (
            "opencode_zen",
            "OpenCode Zen",
            Some("https://opencode.ai/zen/v1"),
            ProviderCategory::Gateway,
        ),
        (
            "openrouter",
            "OpenRouter",
            Some("https://openrouter.ai/api/v1"),
            ProviderCategory::Gateway,
        ),
        (
            "nvidia_nim",
            "NVIDIA NIM",
            Some("https://integrate.api.nvidia.com/v1"),
            ProviderCategory::Gateway,
        ),
    ];

    for (id, name, base_url, category) in expected {
        let preset = presets
            .iter()
            .find(|preset| preset.id == id)
            .expect("provider preset is embedded from YAML");
        assert_eq!(preset.name, name);
        assert_eq!(preset.default_base_url, base_url);
        assert_eq!(preset.category, category);
        assert!(preset.labels.is_empty());
    }
    let freebuff = presets
        .iter()
        .find(|preset| preset.id == "freebuff")
        .expect("Freebuff provider preset is embedded from YAML");
    assert_eq!(freebuff.name, "Freebuff");
    assert_eq!(
        freebuff.default_base_url,
        Some("https://www.codebuff.com/api/v1")
    );
    assert_eq!(freebuff.default_model_prefix, Some("fb"));
    assert_eq!(freebuff.labels, [ProviderLabel::Free]);
    assert_eq!(freebuff.default_auth_type, "bearer");
    assert_eq!(freebuff.default_supported_protocols, ["chat_completions"]);
}

#[test]
fn provider_categories_and_labels_serialize_as_stable_identifiers() {
    assert_eq!(
        serde_json::to_value(ProviderCategory::CloudApi).expect("serialize category"),
        "cloud_api"
    );
    assert_eq!(
        serde_json::to_value(ProviderLabel::Free).expect("serialize label"),
        "free"
    );
    assert_eq!(
        serde_json::to_value(ProviderLabel::FreeTier).expect("serialize label"),
        "free_tier"
    );
}

#[tokio::test]
async fn freebuff_static_catalog_does_not_need_a_key_or_upstream_call() {
    let (_database, state) = test_state().await;
    let discovery = discover_api_key_models(
        FREEBUFF_ADAPTER_ID,
        AdapterApiKeyRequest {
            state: &state,
            base_url: "https://www.codebuff.com/api/v1",
            auth_type: "bearer",
            auth_header: None,
            custom_headers: &BTreeMap::new(),
            preferred_protocol: "chat_completions",
            credential: "",
        },
    )
    .await
    .expect("Freebuff static model catalog");
    assert!(matches!(
        discovery,
        ModelDiscoveryResult::Available { ref models, truncated: false }
            if models == &[
                "deepseek/deepseek-v4-flash",
                "deepseek/deepseek-v4-pro",
                "openai/gpt-5.6-luna",
                "minimax/minimax-m3",
                "mimo/mimo-v2.5",
                "z-ai/glm-5.2",
                "z-ai/glm-5.3-flash",
                "crof/kimi-k3-eco",
                "anthropic/claude-fable-5",
                "meta/muse-spark-1.2-contributor",
            ]
    ));
}

#[test]
fn command_code_uses_documented_provider_paths_and_capabilities() {
    let command_code = adapter(COMMAND_CODE_ADAPTER_ID).expect("Command Code adapter");
    assert_eq!(
        command_code
            .endpoint(
                "https://api.commandcode.ai",
                Protocol::ChatCompletions,
                None,
            )
            .expect("chat endpoint")
            .as_str(),
        "https://api.commandcode.ai/provider/v1/chat/completions"
    );
    assert_eq!(
        command_code
            .endpoint("https://api.commandcode.ai", Protocol::Messages, None)
            .expect("messages endpoint")
            .as_str(),
        "https://api.commandcode.ai/provider/v1/messages"
    );
    assert_eq!(
        command_code
            .model_list_endpoint("https://api.commandcode.ai")
            .expect("models endpoint")
            .as_str(),
        "https://api.commandcode.ai/provider/v1/models"
    );
    assert!(
        command_code
            .endpoint("https://api.commandcode.ai", Protocol::Responses, None)
            .is_err()
    );
    let capabilities = capabilities(COMMAND_CODE_ADAPTER_ID).expect("capabilities");
    assert!(capabilities.api_key_auth_assist);
    assert!(capabilities.api_key_usage);
    assert!(!capabilities.usage_limits);
    assert!(!capabilities.model_catalog_authoritative);
    assert!(
        validate_adapter_config(
            COMMAND_CODE_ADAPTER_ID,
            "bearer",
            "https://api.commandcode.ai",
            "messages",
            &["messages".to_owned()],
            true,
        )
        .is_ok()
    );
    assert!(
        validate_adapter_config(
            COMMAND_CODE_ADAPTER_ID,
            "bearer",
            "https://api.commandcode.ai",
            "responses",
            &["responses".to_owned()],
            true,
        )
        .is_err()
    );
}

#[test]
fn openrouter_accepts_its_three_protocols_and_requires_bearer_auth() {
    let supported = vec![
        "chat_completions".to_owned(),
        "responses".to_owned(),
        "messages".to_owned(),
    ];
    assert!(
        validate_adapter_config(
            OPENROUTER_ADAPTER_ID,
            "bearer",
            "https://openrouter.ai/api/v1",
            "chat_completions",
            &supported,
            true,
        )
        .is_ok()
    );

    let unsupported = ["chat_completions", "google_generate_content"].map(str::to_owned);
    assert!(
        validate_adapter_config(
            OPENROUTER_ADAPTER_ID,
            "bearer",
            "https://openrouter.ai/api/v1",
            "chat_completions",
            &unsupported,
            true,
        )
        .is_err()
    );
    assert!(
        validate_adapter_config(
            OPENROUTER_ADAPTER_ID,
            "header",
            "https://openrouter.ai/api/v1",
            "chat_completions",
            &["chat_completions".to_owned()],
            true,
        )
        .is_err()
    );
    assert_eq!(
        model_upstream_protocol(OPENROUTER_ADAPTER_ID, "anthropic/claude-test"),
        None,
        "OpenRouter model IDs must not imply an upstream protocol"
    );
}

#[test]
fn opencode_models_route_to_documented_upstream_protocols() {
    use UpstreamProtocol::{ChatCompletions, Messages, Responses};
    // Go: Responses-only models previously fell back to the provider's
    // preferred chat_completions endpoint and upstream rejected them with
    // HTTP 400 "Invalid request parameters".
    for model in ["grok-4.7", "grok-4.6", "gpt-6-luna", "gpt-5.6-luna"] {
        assert_eq!(
            model_upstream_protocol(OPENCODE_GO_ADAPTER_ID, model),
            Some(Responses),
            "Go {model} must use the Responses endpoint"
        );
    }
    for model in ["minimax-m3", "qwen3.8-flash"] {
        assert_eq!(
            model_upstream_protocol(OPENCODE_GO_ADAPTER_ID, model),
            Some(Messages),
            "Go {model} must use the Messages endpoint"
        );
    }
    for model in [
        "glm-5.3",
        "kimi-k3",
        "deepseek-v4.1-flash",
        "mimo-v2.6-flash",
        "space-bunny-free",
    ] {
        assert_eq!(
            model_upstream_protocol(OPENCODE_GO_ADAPTER_ID, model),
            Some(ChatCompletions)
        );
    }
    // Zen additions verified against the published endpoint table.
    assert_eq!(
        model_upstream_protocol(OPENCODE_ZEN_ADAPTER_ID, "grok-4.7"),
        Some(Responses)
    );
    assert_eq!(
        model_upstream_protocol(OPENCODE_ZEN_ADAPTER_ID, "claude-opus-5-5"),
        Some(Messages)
    );
    assert_eq!(
        model_upstream_protocol(OPENCODE_ZEN_ADAPTER_ID, "gpt-6-sol"),
        Some(Responses)
    );
    assert_eq!(
        model_upstream_protocol(OPENCODE_ZEN_ADAPTER_ID, "gpt-6-luna"),
        Some(Responses)
    );
    assert_eq!(
        model_upstream_protocol(OPENCODE_ZEN_ADAPTER_ID, "glm-5"),
        Some(ChatCompletions)
    );
    // Models behind the dedicated /systemone endpoint are not routable
    // through the three OpenAI/Anthropic surfaces.
    assert_eq!(
        model_upstream_protocol(OPENCODE_ZEN_ADAPTER_ID, "jev-1.13"),
        None
    );
}

#[test]
fn command_code_auth_assist_is_dispatched_through_its_adapter() {
    let callback_url = "http://localhost:5959/callback";
    let state = "flow-state-value";
    let auth_url = api_key_auth_assist_start_url(COMMAND_CODE_ADAPTER_ID, callback_url, state)
        .expect("build Command Code Studio URL");
    let parsed = reqwest::Url::parse(&auth_url).expect("valid sign-in URL");
    assert_eq!(parsed.host_str(), Some("commandcode.ai"));
    let query = parsed.query_pairs().collect::<HashMap<_, _>>();
    assert_eq!(
        query.get("callback").map(|value| value.as_ref()),
        Some(callback_url)
    );
    assert_eq!(query.get("state").map(|value| value.as_ref()), Some(state));
    assert!(api_key_auth_assist_origin_allowed(
        COMMAND_CODE_ADAPTER_ID,
        "https://commandcode.ai"
    ));
    assert!(!api_key_auth_assist_origin_allowed(
        GENERIC_ADAPTER_ID,
        "https://commandcode.ai"
    ));
    assert!(api_key_auth_assist_origin_registered(
        "https://commandcode.ai"
    ));
    assert!(!api_key_auth_assist_origin_registered(
        "https://evil.commandcode.ai"
    ));

    let callback = parse_api_key_auth_callback(
        COMMAND_CODE_ADAPTER_ID,
        br#"{"apiKey":"secret","state":"state","userId":"user"}"#,
    )
    .expect("parse Command Code callback");
    assert_eq!(callback.state, "state");
    assert_eq!(callback.user_id.as_deref(), Some("user"));
    assert!(parse_api_key_auth_callback(GENERIC_ADAPTER_ID, b"{}").is_err());
}

#[tokio::test]
async fn command_code_key_test_uses_whoami_and_bearer_without_inference() {
    let (base_url, server) = start_mock_server(vec![(200, b"{}".to_vec())]).await;
    let (_database, state) = test_state().await;
    let outcome =
        test_command_code_api_key(&state, &base_url, &BTreeMap::new(), "test-secret").await;
    let requests = server.await.expect("mock server task");
    assert!(outcome.test_passed);
    assert_eq!(outcome.status, Some(200));
    assert_eq!(requests.len(), 1);
    assert!(requests[0].0.starts_with("GET /alpha/whoami HTTP/1.1"));
    assert_eq!(requests[0].1.as_deref(), Some("Bearer test-secret"));
}

#[tokio::test]
async fn model_test_preserves_sanitized_provider_error_body() {
    let provider_body = br#"{"error":{"message":"model does not support this request","code":"invalid_request"},"api_key":"test-secret","input":"private prompt"}"#.to_vec();
    let (base_url, server) = start_mock_server(vec![(400, provider_body)]).await;
    let (_database, state) = test_state().await;
    let credential = Some("test-secret".to_owned());
    let credentials = [credential];

    let outcome = test_api_key_model(
        GENERIC_ADAPTER_ID,
        AdapterModelTestRequest {
            state: &state,
            base_url: &base_url,
            provider_id: "provider",
            model: "test-model",
            auth_type: "bearer",
            auth_header: None,
            custom_headers: &BTreeMap::new(),
            protocol: Protocol::ChatCompletions,
            credentials: &credentials,
        },
    )
    .await;
    let requests = server.await.expect("model test mock server");

    assert!(!outcome.test_passed);
    assert_eq!(outcome.status, Some(400));
    let provider_response = outcome
        .provider_response_body
        .as_deref()
        .expect("provider error body");
    assert!(provider_response.contains("model does not support this request"));
    assert!(provider_response.contains("invalid_request"));
    assert!(!provider_response.contains("test-secret"));
    assert!(!provider_response.contains("private prompt"));
    assert_eq!(requests.len(), 1);
}

#[test]
fn non_sse_model_test_preserves_provider_diagnostics() {
    let outcome = model_test_non_sse_response(
        StatusCode::OK,
        Some("application/json; charset=utf-8"),
        br#"{"error":{"message":"streaming is unavailable for this model","api_key":"test-secret"}}"#,
    );

    assert!(!outcome.test_passed);
    assert_eq!(outcome.status, Some(StatusCode::BAD_GATEWAY.as_u16()));
    assert!(outcome.message.contains("HTTP 200"));
    assert!(outcome.message.contains("application/json; charset=utf-8"));
    let provider_response = outcome
        .provider_response_body
        .as_deref()
        .expect("provider non-SSE body");
    assert!(provider_response.contains("streaming is unavailable for this model"));
    assert!(!provider_response.contains("test-secret"));
}

#[tokio::test]
async fn openrouter_key_test_quota_and_discovery_use_only_documented_get_endpoints() {
    let key_response = serde_json::to_vec(&json!({"data": {
        "limit": 100.0,
        "limit_remaining": 74.5,
        "usage": 25.5,
        "limit_reset": "monthly",
        "is_free_tier": false
    }}))
    .expect("OpenRouter key response JSON");
    let models_response =
        br#"{"data":[{"id":"anthropic/claude-test"},{"id":"vendor/model:free"}]}"#.to_vec();
    let (base_url, server) = start_mock_server(vec![
        (200, key_response.clone()),
        (200, key_response),
        (200, models_response),
    ])
    .await;
    let (_database, state) = test_state().await;
    let base_url = format!("{base_url}/api/v1");

    let key_test = test_api_key_credential(
        OPENROUTER_ADAPTER_ID,
        AdapterApiKeyRequest {
            state: &state,
            base_url: &base_url,
            auth_type: "bearer",
            auth_header: None,
            custom_headers: &BTreeMap::new(),
            preferred_protocol: "chat_completions",
            credential: "test-secret",
        },
    )
    .await;
    assert!(key_test.test_passed);
    assert_eq!(key_test.status, Some(200));
    let usage = fetch_api_key_usage(OPENROUTER_ADAPTER_ID, &state, &base_url, "test-secret")
        .await
        .expect("OpenRouter key usage");
    assert_eq!(usage.quotas[0].used_amount, Some(25.5));
    let discovery = discover_api_key_models(
        OPENROUTER_ADAPTER_ID,
        AdapterApiKeyRequest {
            state: &state,
            base_url: &base_url,
            auth_type: "bearer",
            auth_header: None,
            custom_headers: &BTreeMap::new(),
            preferred_protocol: "chat_completions",
            credential: "test-secret",
        },
    )
    .await
    .expect("OpenRouter model catalog");
    assert!(matches!(
        discovery,
        ModelDiscoveryResult::Available { ref models, truncated: false }
            if models == &["anthropic/claude-test", "vendor/model:free"]
    ));

    let requests = server.await.expect("OpenRouter mock server");
    assert_eq!(requests.len(), 3);
    assert!(requests[0].0.starts_with("GET /api/v1/key HTTP/1.1"));
    assert!(requests[1].0.starts_with("GET /api/v1/key HTTP/1.1"));
    assert!(requests[2].0.starts_with("GET /api/v1/models HTTP/1.1"));
    assert!(
        requests
            .iter()
            .all(|request| request.1.as_deref() == Some("Bearer test-secret"))
    );
}

#[tokio::test]
async fn openrouter_usage_errors_are_redacted_and_return_http_statuses() {
    let (base_url, server) = start_mock_server(vec![
        (
            401,
            br#"{"error":{"message":"secret upstream response"}}"#.to_vec(),
        ),
        (
            403,
            br#"{"error":{"message":"secret upstream response"}}"#.to_vec(),
        ),
        (
            429,
            br#"{"error":{"message":"secret upstream response"}}"#.to_vec(),
        ),
        (
            503,
            br#"{"error":{"message":"secret upstream response"}}"#.to_vec(),
        ),
    ])
    .await;
    let (_database, state) = test_state().await;
    let base_url = format!("{base_url}/api/v1");

    for (status, expected) in [
        (401, "rejected this API key"),
        (403, "rejected this API key"),
        (429, "HTTP 429"),
        (503, "HTTP 503"),
    ] {
        let error = fetch_api_key_usage(OPENROUTER_ADAPTER_ID, &state, &base_url, "test-secret")
            .await
            .expect_err("non-success status must preserve an unavailable quota state");
        assert!(error.contains(expected));
        assert!(!error.contains("secret upstream response"));
        assert!(!error.contains("test-secret"));
        if status == 401 || status == 403 {
            assert!(error.contains(&status.to_string()));
        }
    }
    let requests = server.await.expect("OpenRouter error mock server");
    assert_eq!(requests.len(), 4);
    assert!(
        requests
            .iter()
            .all(|request| request.0.starts_with("GET /api/v1/key HTTP/1.1"))
    );
}

#[tokio::test]
async fn openrouter_usage_rejects_invalid_json_and_oversized_bodies() {
    let oversized = vec![b' '; 256 * 1024 + 1];
    let (base_url, server) =
        start_mock_server(vec![(200, b"not-json".to_vec()), (200, oversized)]).await;
    let (_database, state) = test_state().await;
    let base_url = format!("{base_url}/api/v1");

    let invalid = fetch_api_key_usage(OPENROUTER_ADAPTER_ID, &state, &base_url, "test-secret")
        .await
        .expect_err("invalid JSON should be rejected");
    assert!(invalid.contains("invalid API key usage response"));
    let too_large = fetch_api_key_usage(OPENROUTER_ADAPTER_ID, &state, &base_url, "test-secret")
        .await
        .expect_err("oversized usage responses must be rejected");
    assert!(too_large.contains("configured size limit"));
    let requests = server.await.expect("OpenRouter response-limit mock server");
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.0.starts_with("GET /api/v1/key HTTP/1.1"))
    );
}

#[tokio::test]
async fn openrouter_egress_validation_error_is_sanitized() {
    let (_database, state) = test_state().await;
    let error = fetch_api_key_usage(
        OPENROUTER_ADAPTER_ID,
        &state,
        "file:///not-a-provider",
        "test-secret",
    )
    .await
    .expect_err("unsupported provider URL schemes are rejected");
    assert_eq!(error, "OpenRouter provider egress validation failed");
    assert!(!error.contains("test-secret"));
}

#[tokio::test]
async fn openrouter_usage_timeout_is_bounded_and_sanitized() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("slow OpenRouter mock listener");
    let address = listener.local_addr().expect("slow mock address");
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("usage request");
        let mut request = BufReader::new(socket);
        loop {
            let mut line = String::new();
            request.read_line(&mut line).await.expect("usage header");
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }
        tokio::time::sleep(Duration::from_secs(12)).await;
    });
    let (_database, state) = test_state().await;
    let started = Instant::now();
    let configured_timeout = state
        .operational_settings()
        .settings
        .request_timeout
        .min(Duration::from_secs(8));
    let error = fetch_api_key_usage(
        OPENROUTER_ADAPTER_ID,
        &state,
        &format!("http://{address}/api/v1"),
        "test-secret",
    )
    .await
    .expect_err("silent upstream must time out");
    assert_eq!(error, "OpenRouter usage request timed out");
    assert!(started.elapsed() >= configured_timeout.saturating_sub(Duration::from_millis(500)));
    assert!(started.elapsed() < configured_timeout + Duration::from_secs(2));
    server.abort();
}

#[tokio::test]
async fn cancelling_openrouter_usage_read_drops_the_in_flight_request() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("cancel mock listener");
    let address = listener.local_addr().expect("cancel mock address");
    let (request_seen_tx, request_seen_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("usage request");
        let mut request = BufReader::new(socket);
        let mut request_line = String::new();
        request
            .read_line(&mut request_line)
            .await
            .expect("request line");
        loop {
            let mut line = String::new();
            request.read_line(&mut line).await.expect("request header");
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }
        let _ = request_seen_tx.send(request_line);
        std::future::pending::<()>().await;
    });
    let (_database, state) = test_state().await;
    let base_url = format!("http://{address}/api/v1");
    {
        let usage = fetch_api_key_usage(OPENROUTER_ADAPTER_ID, &state, &base_url, "test-secret");
        tokio::pin!(usage);
        tokio::select! {
            request_line = request_seen_rx => {
                assert!(request_line.expect("request arrives").starts_with("GET /api/v1/key HTTP/1.1"));
            }
            result = &mut usage => {
                let _ = result;
                panic!("usage request unexpectedly completed before cancellation");
            }
        }
    }
    server.abort();
}

#[tokio::test]
async fn opencode_go_and_zen_discovery_use_their_models_endpoints_with_bearer_auth() {
    let responses = vec![
        (200, br#"{"data":[{"id":"go-model"}]}"#.to_vec()),
        (200, br#"{"models":[{"name":"zen-model"}]}"#.to_vec()),
    ];
    let (root, server) = start_mock_server(responses).await;
    let (_database, state) = test_state().await;
    let go = discover_api_key_models(
        OPENCODE_GO_ADAPTER_ID,
        AdapterApiKeyRequest {
            state: &state,
            base_url: &format!("{root}/zen/go/v1"),
            auth_type: "bearer",
            auth_header: None,
            custom_headers: &BTreeMap::new(),
            preferred_protocol: "chat_completions",
            credential: "go-secret",
        },
    )
    .await
    .expect("Go model discovery");
    let zen = discover_api_key_models(
        OPENCODE_ZEN_ADAPTER_ID,
        AdapterApiKeyRequest {
            state: &state,
            base_url: &format!("{root}/zen/v1"),
            auth_type: "bearer",
            auth_header: None,
            custom_headers: &BTreeMap::new(),
            preferred_protocol: "chat_completions",
            credential: "zen-secret",
        },
    )
    .await
    .expect("Zen model discovery");
    let requests = server.await.expect("model mock server");

    assert!(matches!(
        go,
        ModelDiscoveryResult::Available { ref models, truncated: false }
            if models == &["go-model"]
    ));
    assert!(matches!(
        zen,
        ModelDiscoveryResult::Available { ref models, truncated: false }
            if models == &["zen-model"]
    ));
    assert_eq!(requests.len(), 2);
    assert!(requests[0].0.starts_with("GET /zen/go/v1/models HTTP/1.1"));
    assert_eq!(requests[0].1.as_deref(), Some("Bearer go-secret"));
    assert!(requests[1].0.starts_with("GET /zen/v1/models HTTP/1.1"));
    assert_eq!(requests[1].1.as_deref(), Some("Bearer zen-secret"));
    assert!(!model_catalog_authoritative(OPENCODE_GO_ADAPTER_ID));
    assert!(!model_catalog_authoritative(OPENCODE_ZEN_ADAPTER_ID));
}

#[tokio::test]
async fn opencode_go_usage_uses_bearer_limits_body_and_keeps_zen_unverified() {
    let valid = json!({"usage":{
        "rolling":{"status":"ok","percent":10,"resetsAt":"2026-09-15T01:00:00Z"},
        "weekly":{"status":"ok","percent":20,"resetsAt":"2026-09-20T00:00:00Z"},
        "monthly":{"status":"ok","percent":30,"resetsAt":"2026-10-01T00:00:00Z"}
    }});
    let (root, server) = start_mock_server(vec![(
        200,
        serde_json::to_vec(&valid).expect("valid usage JSON"),
    )])
    .await;
    let (_database, state) = test_state().await;
    let snapshot = fetch_api_key_usage(
        OPENCODE_GO_ADAPTER_ID,
        &state,
        &format!("{root}/zen/go/v1"),
        "go-secret",
    )
    .await
    .expect("Go quota is available");
    let requests = server.await.expect("usage mock server");
    assert_eq!(snapshot.quotas.len(), 3);
    assert!(
        snapshot
            .quotas
            .iter()
            .all(|quota| quota.unit.as_deref() == Some("percent"))
    );
    assert!(requests[0].0.starts_with("GET /zen/go/v1/usage HTTP/1.1"));
    assert_eq!(requests[0].1.as_deref(), Some("Bearer go-secret"));

    let unsupported = fetch_api_key_usage(
        OPENCODE_ZEN_ADAPTER_ID,
        &state,
        &format!("{root}/zen/v1"),
        "zen-secret",
    )
    .await
    .expect_err("Zen quota has no verified per-key source");
    assert!(unsupported.contains("does not support API-key usage"));
}

#[tokio::test]
async fn opencode_go_usage_rejects_unauthorized_and_oversized_responses() {
    let (_database, state) = test_state().await;
    let (unauthorized_root, unauthorized_server) =
        start_mock_server(vec![(401, b"{}".to_vec())]).await;
    let unauthorized = fetch_api_key_usage(
        OPENCODE_GO_ADAPTER_ID,
        &state,
        &format!("{unauthorized_root}/zen/go/v1"),
        "bad-secret",
    )
    .await
    .expect_err("401 must be reported as a rejected key");
    assert!(unauthorized.contains("rejected this API key"));
    let _ = unauthorized_server.await.expect("unauthorized mock server");

    let oversized = vec![b' '; 256 * 1024 + 1];
    let (oversized_root, oversized_server) = start_mock_server(vec![(200, oversized)]).await;
    let error = fetch_api_key_usage(
        OPENCODE_GO_ADAPTER_ID,
        &state,
        &format!("{oversized_root}/zen/go/v1"),
        "test-secret",
    )
    .await
    .expect_err("oversized quota response must be rejected");
    assert!(error.contains("256 KiB"));
    let _ = oversized_server.await.expect("oversized mock server");
}

#[tokio::test]
async fn opencode_go_usage_has_an_overall_timeout() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("slow mock listener");
    let address = listener.local_addr().expect("slow mock address");
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("quota request");
        let mut request = BufReader::new(socket);
        loop {
            let mut line = String::new();
            request.read_line(&mut line).await.expect("quota header");
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    });
    let (_database, state) = test_state().await;
    let configured_timeout = state
        .operational_settings()
        .settings
        .request_timeout
        .min(Duration::from_secs(8));
    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        fetch_api_key_usage(
            OPENCODE_GO_ADAPTER_ID,
            &state,
            &format!("http://{address}/zen/go/v1"),
            "test-secret",
        ),
    )
    .await
    .expect("quota fetch must return before the outer safety timeout");
    let error = result.expect_err("no upstream response should time out");
    assert!(!error.is_empty());
    assert!(started.elapsed() >= configured_timeout.saturating_sub(Duration::from_millis(500)));
    assert!(started.elapsed() < Duration::from_secs(10));
    server.abort();
}

#[tokio::test]
async fn command_code_usage_uses_optional_sources_and_preserves_upstream_units() {
    let credits = json!({
        "credits": {
            "monthlyCredits": 12,
            "purchasedCredits": 3.5,
            "freeCredits": "2"
        },
        "windowLimits": {
            "fiveHour": {"cap": 50, "used": 12.5, "resetAt": 1_800_000_000},
            "weekly": {"cap": 300, "used": 99, "resetAt": "2026-09-20T00:00:00Z"},
            "limited": false,
            "exceeded": false
        }
    });
    let responses = vec![
        (503, b"{}".to_vec()),
        (200, serde_json::to_vec(&credits).expect("credits JSON")),
        (
            200,
            br#"{"data":{"planId":"team","currentPeriodStart":"2026-09-01T00:00:00Z","currentPeriodEnd":"2026-10-01T00:00:00Z"}}"#.to_vec(),
        ),
        (200, br#"{"totalMonthlyCredits":26.5,"totalCost":9999}"#.to_vec()),
    ];
    let (base_url, server) = start_mock_server(responses).await;
    let (_database, state) = test_state().await;
    let snapshot = fetch_command_code_usage(&state, &base_url, "usage-secret")
        .await
        .expect("usage snapshot despite optional whoami failure");
    let requests = server.await.expect("mock server task");

    assert_eq!(snapshot.plan.as_deref(), Some("team"));
    assert_eq!(snapshot.quotas.len(), 3);
    assert_eq!(snapshot.quotas[0].used_amount, Some(12.5));
    assert_eq!(snapshot.quotas[0].limit_amount, Some(50.0));
    assert_eq!(snapshot.quotas[0].unit.as_deref(), Some("upstream units"));
    assert_eq!(snapshot.quotas[2].id, "monthly");
    assert_eq!(snapshot.quotas[2].used_amount, Some(26.5));
    assert_eq!(snapshot.quotas[2].limit_amount, Some(38.5));
    let balance = snapshot.credit_balance.expect("credit balance");
    assert_eq!(balance.monthly_remaining, Some(12.0));
    assert_eq!(balance.purchased_remaining, Some(3.5));
    assert_eq!(balance.free_remaining, Some(2.0));
    assert_eq!(balance.period_used, Some(26.5));
    assert_eq!(balance.unit, "credits");
    assert_eq!(
        snapshot.quotas[1].reset_at,
        parse_rfc3339_epoch("2026-09-20T00:00:00Z")
    );
    assert_eq!(requests.len(), 4);
    assert!(requests[0].0.starts_with("GET /alpha/whoami HTTP/1.1"));
    assert!(
        requests[1]
            .0
            .starts_with("GET /alpha/billing/credits HTTP/1.1")
    );
    assert!(!requests[1].0.contains("orgId"));
    assert!(
        requests[2]
            .0
            .starts_with("GET /alpha/billing/subscriptions")
    );
    assert!(requests[3].0.starts_with("GET /alpha/usage/summary?since="));
    assert!(
        requests
            .iter()
            .all(|(_, auth)| auth.as_deref() == Some("Bearer usage-secret"))
    );
}

#[tokio::test]
async fn command_code_usage_does_not_let_slow_optional_whoami_hide_credits() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("slow Command Code mock listener");
    let address = listener
        .local_addr()
        .expect("slow Command Code mock address");
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("whoami request");
        let mut whoami = BufReader::new(socket);
        let mut request_line = String::new();
        whoami
            .read_line(&mut request_line)
            .await
            .expect("whoami request line");
        loop {
            let mut line = String::new();
            whoami
                .read_line(&mut line)
                .await
                .expect("whoami request header");
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(12)).await;
            drop(whoami);
        });

        let responses = [
            (
                200,
                br#"{"credits":{"monthlyCredits":5},"windowLimits":{}}"#.to_vec(),
            ),
            (200, br#"{"data":{}}"#.to_vec()),
            (200, br#"{"totalMonthlyCredits":0}"#.to_vec()),
        ];
        for (status, body) in responses {
            let (socket, _) = listener.accept().await.expect("Command Code usage request");
            let mut stream = BufReader::new(socket);
            let mut request_line = String::new();
            stream
                .read_line(&mut request_line)
                .await
                .expect("usage request line");
            loop {
                let mut line = String::new();
                stream
                    .read_line(&mut line)
                    .await
                    .expect("usage request header");
                if line == "\r\n" || line.is_empty() {
                    break;
                }
            }
            let headers = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream
                .get_mut()
                .write_all(headers.as_bytes())
                .await
                .expect("usage response headers");
            stream
                .get_mut()
                .write_all(&body)
                .await
                .expect("usage response body");
        }
    });
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some([47_u8; 32]);
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(20);
    let state = AppState::new(config, database.db.clone());

    let result =
        fetch_command_code_usage(&state, &format!("http://{address}"), "usage-secret").await;
    if result.is_err() {
        server.abort();
    }
    let snapshot = result.expect("credits must remain available when whoami times out");
    server.await.expect("Command Code mock server task");
    assert_eq!(
        snapshot
            .credit_balance
            .expect("credit balance from credits endpoint")
            .monthly_remaining,
        Some(5.0)
    );
}

#[tokio::test]
async fn command_code_usage_rejects_oversized_upstream_responses() {
    let mut oversized = br#"{"credits":{}"#.to_vec();
    oversized.resize(
        crate::config::UpstreamSettings::default().usage_response_max_bytes + 1,
        b' ',
    );
    let (base_url, server) = start_mock_server(vec![(404, b"{}".to_vec()), (200, oversized)]).await;
    let (_database, state) = test_state().await;
    let error = fetch_command_code_usage(&state, &base_url, "usage-secret")
        .await
        .expect_err("response cap is enforced");
    let _ = server.await.expect("mock server task");
    assert!(error.contains("256 KiB limit"));
}

#[test]
fn command_code_usage_parser_keeps_partial_data_and_recognizes_limits() {
    let partial = parse_command_code_usage(&json!({"credits":{"monthlyCredits":8}}), None, None)
        .expect("partial usage snapshot");
    assert_eq!(
        partial
            .credit_balance
            .expect("partial balance")
            .monthly_remaining,
        Some(8.0)
    );
    assert!(partial.quotas.is_empty());

    let limited = parse_command_code_usage(
        &json!({
            "credits":{"freeCredits":1},
            "windowLimits":{"weekly":{"cap":10,"used":10},"exceeded":true}
        }),
        None,
        None,
    )
    .expect("limited snapshot");
    assert!(limited.limit_reached);
    assert_eq!(limited.quotas.len(), 1);
    let not_limited = parse_command_code_usage(
        &json!({"credits":{},"windowLimits":{"limited":"false"}}),
        None,
        None,
    )
    .expect("unlimited snapshot");
    assert!(!not_limited.limit_reached);
    let unsaturated = parse_command_code_usage(
        &json!({
            "credits": {"monthlyCredits": 22.91},
            "windowLimits": {
                "fiveHour": {"cap": 14, "used": 6.71},
                "weekly": {"cap": 35, "used": 12.09},
                "monthly": {"cap": 70, "used": 47.6},
                "limited": true
            }
        }),
        None,
        None,
    )
    .expect("unsaturated snapshot");
    assert!(!unsaturated.limit_reached);
    assert_eq!(unsaturated.quotas.len(), 3);
    assert_eq!(unsaturated.quotas[2].id, "monthly");

    let three_quotas = parse_command_code_usage(
        &json!({
            "credits": {"monthlyCredits": 22.55},
            "windowLimits": {
                "fiveHour": {"cap": 14, "used": 7.07},
                "weekly": {"cap": 35, "used": 12.45}
            }
        }),
        None,
        Some(&json!({"totalMonthlyCredits": 47.98})),
    )
    .expect("three quotas snapshot");
    assert_eq!(three_quotas.quotas.len(), 3);
    assert_eq!(three_quotas.quotas[0].id, "five_hour");
    assert_eq!(three_quotas.quotas[1].id, "weekly");
    assert_eq!(three_quotas.quotas[2].id, "monthly");
    assert_eq!(three_quotas.quotas[2].used_amount, Some(47.98));
    assert_eq!(three_quotas.quotas[2].limit_amount, Some(47.98 + 22.55));
    assert!(!three_quotas.limit_reached);
    assert!(parse_rfc3339_epoch("2026-02-29T00:00:00Z").is_none());
    assert_eq!(
        parse_rfc3339_epoch("2026-10-01T02:30:00+02:30"),
        parse_rfc3339_epoch("2026-10-01T00:00:00Z")
    );
}

#[test]
fn kilo_gateway_uses_its_chat_and_model_paths_and_forwards_only_safe_mode_hints() {
    let kilo = adapter(KILO_GATEWAY_ADAPTER_ID).expect("Kilo adapter");
    assert_eq!(
        kilo.endpoint(
            "https://api.kilo.ai/api/gateway",
            Protocol::ChatCompletions,
            None
        )
        .expect("Kilo chat endpoint")
        .as_str(),
        "https://api.kilo.ai/api/gateway/chat/completions"
    );
    assert_eq!(
        kilo.model_list_endpoint("https://api.kilo.ai/api/gateway")
            .expect("Kilo models endpoint")
            .as_str(),
        "https://api.kilo.ai/api/gateway/models"
    );
    let mut headers = HeaderMap::new();
    headers.insert("x-kilocode-mode", HeaderValue::from_static("plan"));
    let request = kilo
        .apply_client_headers(
            reqwest::Client::new().post("https://api.kilo.ai/api/gateway"),
            &headers,
        )
        .build()
        .expect("Kilo request");
    assert_eq!(
        request.headers().get("x-kilocode-mode"),
        Some(&HeaderValue::from_static("plan"))
    );

    headers.insert("x-kilocode-mode", HeaderValue::from_static("plan:override"));
    let request = kilo
        .apply_client_headers(
            reqwest::Client::new().post("https://api.kilo.ai/api/gateway"),
            &headers,
        )
        .build()
        .expect("Kilo request with invalid mode");
    assert!(!request.headers().contains_key("x-kilocode-mode"));

    let page = parse_provider_model_page(&json!({
        "data": [{"id":"anthropic/claude-sonnet-4.6","context_length":200000}]
    }))
    .expect("Kilo model catalog response");
    assert_eq!(page.models, ["anthropic/claude-sonnet-4.6"]);
}

#[test]
fn codex_model_parser_accepts_live_slug_catalog_and_filters_hidden_models() {
    let models = parse_codex_model_list(&json!({
        "models": [
            {
                "slug": "gpt-5.6-luna",
                "display_name": "GPT 5.6 Luna",
                "visibility": "list",
                "supported_in_api": true
            },
            {
                "slug": "hidden-model",
                "visibility": "hide",
                "supported_in_api": true
            },
            {
                "slug": "internal-model",
                "visibility": "list",
                "supported_in_api": false
            },
            { "id": "gpt-5.5" },
            { "name": "gpt-5.5" }
        ]
    }))
    .expect("Codex live catalog");

    assert_eq!(models, ["gpt-5.6-luna", "gpt-5.5"]);
}

#[test]
fn codex_model_parser_accepts_root_arrays_and_model_maps() {
    let root_array = parse_codex_model_list(&json!([
        { "model": "gpt-5.5" },
        { "id": "gpt-5.4" }
    ]))
    .expect("Codex root array");
    assert_eq!(root_array, ["gpt-5.5", "gpt-5.4"]);

    let model_map = parse_codex_model_list(&json!({
        "gpt-5.6-luna": { "title": "GPT 5.6 Luna" },
        "gpt-5.5": { "title": "GPT 5.5" }
    }))
    .expect("Codex model map");
    assert_eq!(model_map, ["gpt-5.5", "gpt-5.6-luna"]);
}

#[test]
fn codex_model_request_uses_backend_identity_headers() {
    let account = CodexAccount {
        access_token: "access-token".to_owned(),
        refresh_token: None,
        id_token: None,
        expires_at: 0,
        account_id: Some("account-123".to_owned()),
        chatgpt_user_id: None,
        email: None,
        plan: None,
    };
    let url = reqwest::Url::parse("https://chatgpt.com/backend-api/codex/models").unwrap();
    let request = build_codex_model_request(&reqwest::Client::new(), &url, &account)
        .expect("Codex model request")
        .build()
        .expect("build Codex model request");

    assert_eq!(request.headers()["authorization"], "Bearer access-token");
    assert_eq!(request.headers()["originator"], "codex_cli_rs");
    assert_eq!(request.headers()["version"], codex_oauth::CODEX_CLI_VERSION);
    assert_eq!(request.headers()["openai-beta"], "responses=experimental");
    assert_eq!(
        request.headers()["x-codex-beta-features"],
        "responses_websockets"
    );
    assert_eq!(request.headers()["chatgpt-account-id"], "account-123");
}

fn codex_account_for_match(
    account_id: Option<&str>,
    user_id: Option<&str>,
    email: Option<&str>,
) -> CodexAccount {
    CodexAccount {
        access_token: "access-token".to_owned(),
        refresh_token: None,
        id_token: None,
        expires_at: 0,
        account_id: account_id.map(str::to_owned),
        chatgpt_user_id: user_id.map(str::to_owned),
        email: email.map(str::to_owned),
        plan: None,
    }
}

#[test]
fn codex_account_match_requires_the_same_workspace_and_user() {
    let incoming = codex_account_for_match(
        Some("workspace-1"),
        Some("user-1"),
        Some("user@example.com"),
    );
    let same_user = codex_account_for_match(
        Some("workspace-1"),
        Some("user-1"),
        Some("user@example.com"),
    );
    let different_user = codex_account_for_match(
        Some("workspace-1"),
        Some("user-2"),
        Some("user@example.com"),
    );

    let mut same_state = CodexAccountMatchState::default();
    same_state.consider("same", &same_user, &incoming);
    assert_eq!(same_state.selected_id().as_deref(), Some("same"));

    let mut different_state = CodexAccountMatchState::default();
    different_state.consider("different", &different_user, &incoming);
    assert_eq!(different_state.selected_id(), None);
}

#[test]
fn codex_account_match_rejects_ambiguous_legacy_rows() {
    let incoming = codex_account_for_match(
        Some("workspace-1"),
        Some("user-1"),
        Some("user@example.com"),
    );
    let legacy = codex_account_for_match(Some("workspace-1"), None, Some("user@example.com"));
    let mut state = CodexAccountMatchState::default();
    state.consider("legacy-1", &legacy, &incoming);
    state.consider("legacy-2", &legacy, &incoming);

    assert_eq!(state.selected_id(), None);
}

#[test]
fn codex_account_match_can_upgrade_one_legacy_row() {
    let incoming = codex_account_for_match(
        Some("workspace-1"),
        Some("user-1"),
        Some("user@example.com"),
    );
    let legacy = codex_account_for_match(Some("workspace-1"), None, Some("user@example.com"));
    let mut state = CodexAccountMatchState::default();
    state.consider("legacy", &legacy, &incoming);

    assert_eq!(state.selected_id().as_deref(), Some("legacy"));
}

#[test]
fn codex_account_match_accepts_a_callback_with_partial_metadata() {
    let previous = codex_account_for_match(
        Some("workspace-1"),
        Some("user-1"),
        Some("user@example.com"),
    );
    let incoming = codex_account_for_match(Some("workspace-1"), None, Some("USER@example.com"));
    let mut state = CodexAccountMatchState::default();
    state.consider("existing", &previous, &incoming);

    assert_eq!(state.selected_id().as_deref(), Some("existing"));
}

#[test]
fn codex_account_match_can_use_email_when_a_legacy_row_lacks_account_id() {
    let previous = codex_account_for_match(None, Some("user-1"), Some("user@example.com"));
    let incoming = codex_account_for_match(
        Some("workspace-1"),
        Some("user-1"),
        Some("USER@example.com"),
    );
    let mut state = CodexAccountMatchState::default();
    state.consider("legacy", &previous, &incoming);

    assert_eq!(state.selected_id().as_deref(), Some("legacy"));
}

#[tokio::test]
async fn saving_distinct_and_reconnected_codex_accounts_keeps_existing_credentials() {
    let (database, state) = test_state().await;
    seed_provider(
        &database.db,
        ProviderSeed {
            id: "codex-provider",
            name: "Codex",
            base_url: codex_oauth::BASE_URL,
            adapter_id: CODEX_ADAPTER_ID,
            auth_type: "oauth",
            model_prefix: "codex-test",
            preferred_protocol: "responses",
            supported_protocols: &["responses"],
        },
    )
    .await
    .expect("seed Codex provider");

    for (user_id, access_token) in [("user-1", "token-1"), ("user-2", "token-2")] {
        let account = codex_account_for_match(
            Some("workspace-1"),
            Some(user_id),
            Some("shared@example.com"),
        );
        save_codex_account(
            &state,
            "codex-provider",
            AdapterOAuthAccount {
                payload: serde_json::to_value(CodexAccount {
                    access_token: access_token.to_owned(),
                    ..account
                })
                .expect("serialize Codex account"),
                display_name: user_id.to_owned(),
            },
        )
        .await
        .expect("save Codex account");
    }

    let account = codex_account_for_match(
        Some("workspace-1"),
        Some("user-1"),
        Some("shared@example.com"),
    );
    save_codex_account(
        &state,
        "codex-provider",
        AdapterOAuthAccount {
            payload: serde_json::to_value(CodexAccount {
                access_token: "token-1-refreshed".to_owned(),
                ..account
            })
            .expect("serialize refreshed Codex account"),
            display_name: "user-1 refreshed".to_owned(),
        },
    )
    .await
    .expect("refresh Codex account");

    let master_key = state.config.master_key;
    let keys = database
        .db
        .read(move |transaction| {
            let keys = transaction
                .scan_prefix::<Record>(Table::ProviderApiKeys, "", 128)?
                .into_iter()
                .map(|(_, record)| {
                    let secret = record.bytes("secret").expect("Codex key secret");
                    let serialized = security::decrypt_secret(master_key.as_ref(), Some(secret))
                        .expect("decrypt Codex key")
                        .expect("Codex key payload");
                    let account: CodexAccount =
                        serde_json::from_str(&serialized).expect("decode Codex key");
                    (record.text("id").expect("key id").to_owned(), account)
                })
                .collect::<Vec<_>>();
            Ok(keys)
        })
        .await
        .expect("read Codex key index");

    assert_eq!(keys.len(), 2);
    let (reconnected_id, _user_1) = keys
        .iter()
        .find(|(_, account)| account.chatgpt_user_id.as_deref() == Some("user-1"))
        .expect("user-1 Codex key")
        .clone();
    let (other_id, _user_2) = keys
        .iter()
        .find(|(_, account)| account.chatgpt_user_id.as_deref() == Some("user-2"))
        .expect("user-2 Codex key")
        .clone();
    assert_ne!(reconnected_id, other_id);
    database
        .db
        .write({
            let reconnected_id = reconnected_id.clone();
            move |transaction| {
                let mut key = transaction
                    .get::<Record>(Table::ProviderApiKeys, &reconnected_id)?
                    .ok_or(StorageError::NotFound)?;
                key.insert("invalid", Field::Bool(true));
                key.insert(
                    "last_error",
                    Field::Text("OpenAI Codex account needs to be reconnected".to_owned()),
                );
                key.insert("last_test_passed", Field::Bool(false));
                let provider_id = key.text("provider_id")?.to_owned();
                transaction.delete(
                    Table::ProviderApiKeyAvailabilityIndex,
                    &crate::infra::db::provider_api_key_index_key(
                        &provider_id,
                        &reconnected_id,
                        true,
                    )?,
                )?;
                transaction.put(Table::ProviderApiKeys, &reconnected_id, &key)?;
                let mut provider = transaction
                    .get::<Record>(Table::Providers, &provider_id)?
                    .ok_or(StorageError::NotFound)?;
                provider.insert(
                    "invalid_api_key_count",
                    Field::I64(provider.integer("invalid_api_key_count")?.saturating_add(1)),
                );
                transaction.put(Table::Providers, &provider_id, &provider)
            }
        })
        .await
        .expect("mark Codex account invalid");

    let account = codex_account_for_match(
        Some("workspace-1"),
        Some("user-1"),
        Some("shared@example.com"),
    );
    save_codex_account(
        &state,
        "codex-provider",
        AdapterOAuthAccount {
            payload: serde_json::to_value(CodexAccount {
                access_token: "token-1-reconnected".to_owned(),
                ..account
            })
            .expect("serialize reconnected Codex account"),
            display_name: "user-1 reconnected".to_owned(),
        },
    )
    .await
    .expect("reconnect invalid Codex account");

    let (provider, saved) = database
        .db
        .read(move |transaction| {
            let provider = transaction
                .get::<Record>(Table::Providers, "codex-provider")?
                .ok_or(StorageError::NotFound)?;
            let key = transaction
                .get::<Record>(Table::ProviderApiKeys, &reconnected_id)?
                .ok_or(StorageError::NotFound)?;
            Ok((provider, key))
        })
        .await
        .expect("read reconnected Codex account");
    assert_eq!(provider.integer("api_key_count").expect("key count"), 2);
    assert_eq!(
        provider
            .integer("invalid_api_key_count")
            .expect("invalid key count"),
        0
    );
    assert!(!saved.boolean("invalid").expect("invalid flag"));
    let serialized = security::decrypt_secret(
        state.config.master_key.as_ref(),
        Some(saved.bytes("secret").expect("secret")),
    )
    .expect("decrypt reconnected Codex account")
    .expect("Codex account payload");
    let saved_account: CodexAccount =
        serde_json::from_str(&serialized).expect("decode reconnected Codex account");
    assert_eq!(saved_account.access_token, "token-1-reconnected");
}

#[test]
fn unknown_adapter_ids_are_not_accepted() {
    assert!(adapter("unregistered").is_none());
    assert!(
        validate_adapter_config(
            "unregistered",
            "none",
            "https://example.com/v1",
            "chat_completions",
            &["chat_completions".to_owned()],
            false
        )
        .is_err()
    );
}
