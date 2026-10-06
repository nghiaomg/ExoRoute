use crate::protocol::UpstreamProtocol;
use crate::provider_adapters::AdapterRequestContext;
use crate::provider_adapters::OAuthRequestAuth;
use crate::provider_adapters::ProviderAdapter;
use crate::provider_adapters::UpstreamAuthContext;
use crate::provider_adapters::antigravity::ANTIGRAVITY_ADAPTER;
use crate::provider_adapters::antigravity::antigravity_oauth;
use crate::provider_adapters::antigravity::probes::*;
use crate::state::AppState;
use http::StatusCode;
use serde_json::json;

#[test]
fn model_prefix_and_aliases_are_normalized() {
    assert_eq!(
        normalize_model_id("ag/gemini-3.7-flash"),
        "gemini-3.7-flash-tiered"
    );
    assert_eq!(
        normalize_model_id("antigravity/gpt-oss-120b"),
        "gpt-oss-120b-medium"
    );
    assert_eq!(normalize_model_id("claude-sonnet-4-6"), "claude-sonnet-4-6");
}

#[test]
fn tiered_model_alias_sets_native_thinking_level() {
    let mut request = json!({"generationConfig":{"maxOutputTokens":128}});
    apply_tiered_thinking_config(&mut request, "ag/gemini-3.7-flash-low");
    assert_eq!(
        request["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "low"
    );
    assert_eq!(
        request["generationConfig"]["thinkingConfig"]["includeThoughts"],
        false
    );
}

#[test]
fn google_sse_is_accumulated_into_a_terminal_response() {
    let bytes = b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hel\"}]}}]}\n\ndata: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"lo\"}]},\"finishReason\":\"STOP\"}]}\n\n";
    let response = parse_antigravity_event_stream(bytes).expect("Google SSE response");
    assert_eq!(
        response["candidates"][0]["content"]["parts"][0]["text"],
        "hello"
    );
    assert_eq!(response["candidates"][0]["finishReason"], "STOP");
}

#[test]
fn google_sse_rejects_missing_terminal_event() {
    let bytes = b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hello\"}]}}]}\n\n";
    assert!(parse_antigravity_event_stream(bytes).is_err());
}

#[test]
fn google_sse_rejects_thought_only_terminal_response_without_http_status() {
    let bytes = br#"data: {"candidates":[{"content":{"parts":[{"text":"internal","thought":true}]},"finishReason":"STOP"}]}

"#;
    let error = parse_antigravity_event_stream(bytes).expect_err("visible content required");
    assert_eq!(
        error.message,
        "Antigravity completed response has no visible text or tool call"
    );
}

#[tokio::test]
async fn wraps_google_request_with_cloud_code_envelope() {
    let database = crate::support::test_support::TestDatabase::open().await;
    let state = AppState::new(database.config(), database.db.clone());
    let auth = OAuthRequestAuth {
        access_token: "oauth-secret".to_owned(),
        account_id: None,
        project_id: Some("cloud-project".to_owned()),
    };
    let mut body = json!({
        "contents": [{"role":"user","parts":[{"text":"hello"}] }]
    });
    let preparation = ANTIGRAVITY_ADAPTER
        .prepare_upstream_request(
            AdapterRequestContext {
                state: &state,
                base_url: antigravity_oauth::RUNTIME_BASE_URL,
                adapter_base_url_override: None,
                model: "ag/gemini-3.7-flash",
                request_id: "request-1",
                streaming: true,
                auth: UpstreamAuthContext {
                    auth_type: "antigravity_oauth",
                    auth_header: None,
                    secret: None,
                    oauth_auth: Some(&auth),
                    session_id: "session-1",
                    protocol: UpstreamProtocol::GoogleGenerateContent,
                },
            },
            &mut body,
        )
        .await
        .expect("Cloud Code request envelope");

    assert!(preparation.headers.is_empty());
    assert_eq!(body["project"], "cloud-project");
    assert_eq!(body["model"], "gemini-3.7-flash-tiered");
    assert_eq!(body["userAgent"], "antigravity");
    assert_eq!(body["requestType"], "agent");
    assert_eq!(body["requestId"], "request-1");
    assert_eq!(body["request"]["sessionId"], "session-1");
    assert_eq!(
        ANTIGRAVITY_ADAPTER
            .upstream_endpoint(
                antigravity_oauth::RUNTIME_BASE_URL,
                UpstreamProtocol::GoogleGenerateContent,
                "gemini-3.7-flash",
                true,
                None,
            )
            .expect("Antigravity endpoint")
            .as_str(),
        "https://daily-cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse"
    );
    assert!(ANTIGRAVITY_ADAPTER.is_key_rejection_status(StatusCode::CONFLICT));
    assert!(ANTIGRAVITY_ADAPTER.is_key_rejection_status(StatusCode::TOO_MANY_REQUESTS));
    assert!(!ANTIGRAVITY_ADAPTER.is_key_rejection_status(StatusCode::INTERNAL_SERVER_ERROR));
}
