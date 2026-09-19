use super::*;
use crate::provider_adapters::{
    OPENCODE_GO_ADAPTER_ID, OPENCODE_ZEN_ADAPTER_ID, UpstreamAuthContext, UpstreamProtocol,
};
use http::{HeaderMap, HeaderValue};
use serde_json::json;

#[test]
fn endpoints_match_opencode_prefixes_and_google_actions() {
    for (adapter_id, base_url) in [
        (OPENCODE_GO_ADAPTER_ID, "https://opencode.ai/zen/go/v1"),
        (OPENCODE_ZEN_ADAPTER_ID, "https://opencode.ai/zen/v1"),
    ] {
        for (protocol, suffix) in [
            (UpstreamProtocol::ChatCompletions, "chat/completions"),
            (UpstreamProtocol::Responses, "responses"),
            (UpstreamProtocol::Messages, "messages"),
        ] {
            assert_eq!(
                super::super::upstream_endpoint(
                    adapter_id, base_url, protocol, "model", false, None,
                )
                .expect("documented OpenCode endpoint")
                .as_str(),
                format!("{base_url}/{suffix}")
            );
        }
    }
    let zen = OPENCODE_ZEN_ADAPTER
        .upstream_endpoint(
            "https://opencode.ai/zen/v1",
            UpstreamProtocol::GoogleGenerateContent,
            "gemini/3.7 flash",
            false,
            None,
        )
        .expect("Zen Google endpoint");
    assert_eq!(
        zen.as_str(),
        "https://opencode.ai/zen/v1/models/gemini%2F3.7%20flash:generateContent"
    );
    let zen_stream = OPENCODE_ZEN_ADAPTER
        .upstream_endpoint(
            "https://opencode.ai/zen/v1",
            UpstreamProtocol::GoogleGenerateContent,
            "gemini-3.7-flash",
            true,
            None,
        )
        .expect("Zen Google SSE endpoint");
    assert_eq!(
        zen_stream.as_str(),
        "https://opencode.ai/zen/v1/models/gemini-3.7-flash:streamGenerateContent?alt=sse"
    );
    assert!(
        super::super::upstream_endpoint(
            OPENCODE_GO_ADAPTER_ID,
            "https://opencode.ai/zen/go/v1",
            UpstreamProtocol::GoogleGenerateContent,
            "gemini-3.7-flash",
            false,
            None,
        )
        .is_err()
    );
    assert!(google_generate_content_endpoint("https://opencode.ai/zen/v1", "", true).is_err());
}

#[test]
fn static_model_metadata_is_exact_and_provider_scoped() {
    assert_eq!(
        model_protocol(OPENCODE_GO_ADAPTER_ID, "minimax-m3"),
        Some(UpstreamProtocol::Messages)
    );
    assert_eq!(
        model_protocol(OPENCODE_ZEN_ADAPTER_ID, "gemini-3.7-flash"),
        Some(UpstreamProtocol::GoogleGenerateContent)
    );
    assert_eq!(
        model_protocol(OPENCODE_GO_ADAPTER_ID, "gemini-3.7-flash"),
        None
    );
    assert_eq!(
        model_protocol(OPENCODE_ZEN_ADAPTER_ID, "brand-new-model"),
        None
    );
}

#[test]
fn opencode_go_replays_required_reasoning_content_without_overwriting_real_content() {
    let mut deepseek = json!({
        "messages":[
            {"role":"user","content":"hello"},
            {"role":"assistant","content":"tool result"},
            {"role":"assistant","content":"real","reasoning_content":"provider thought"}
        ]
    });
    inject_opencode_go_reasoning_content("deepseek-v4.1-flash", &mut deepseek);
    assert_eq!(deepseek["messages"][1]["reasoning_content"], " ");
    assert_eq!(
        deepseek["messages"][2]["reasoning_content"],
        "provider thought"
    );

    let mut kimi = json!({
        "messages":[
            {"role":"assistant","content":"plain"},
            {"role":"assistant","content":null,"tool_calls":[{"id":"call-1"}]}
        ]
    });
    inject_opencode_go_reasoning_content("kimi-k2.6", &mut kimi);
    assert!(kimi["messages"][0].get("reasoning_content").is_none());
    assert_eq!(kimi["messages"][1]["reasoning_content"], " ");

    let mut ordinary = json!({
        "messages":[{"role":"assistant","content":"plain"}]
    });
    inject_opencode_go_reasoning_content("glm-5.3-flash", &mut ordinary);
    assert!(ordinary["messages"][0].get("reasoning_content").is_none());
}

#[test]
fn quota_parser_rejects_partial_and_out_of_range_snapshots() {
    let valid = json!({"usage":{
        "rolling":{"status":"ok","percent":12.5,"resetsAt":"2026-09-15T01:00:00Z"},
        "weekly":{"status":"ok","percent":40.0,"resetsAt":"2026-09-20T00:00:00Z"},
        "monthly":{"status":"rate-limited","percent":3.0,"resetsAt":"2026-10-01T00:00:00Z"}
    }});
    let snapshot = parse_opencode_go_usage(&valid).expect("valid quota");
    assert_eq!(snapshot.quotas.len(), 3);
    assert_eq!(snapshot.quotas[2].used_percent, 100.0);
    assert!(snapshot.limit_reached);
    assert!(parse_opencode_go_usage(&json!({"usage":{"rolling":{"status":"ok","percent":101,"resetsAt":"2026-09-15T00:00:00Z"}}})).is_err());
}

#[test]
fn auth_header_selection_matches_open_code_contract() {
    let cases = [
        (
            OPENCODE_GO_ADAPTER_ID,
            UpstreamProtocol::ChatCompletions,
            Some("Bearer key-value"),
            None,
            None,
        ),
        (
            OPENCODE_GO_ADAPTER_ID,
            UpstreamProtocol::Responses,
            Some("Bearer key-value"),
            None,
            None,
        ),
        (
            OPENCODE_GO_ADAPTER_ID,
            UpstreamProtocol::Messages,
            None,
            Some("key-value"),
            None,
        ),
        (
            OPENCODE_ZEN_ADAPTER_ID,
            UpstreamProtocol::ChatCompletions,
            Some("Bearer key-value"),
            None,
            None,
        ),
        (
            OPENCODE_ZEN_ADAPTER_ID,
            UpstreamProtocol::Responses,
            None,
            Some("key-value"),
            None,
        ),
        (
            OPENCODE_ZEN_ADAPTER_ID,
            UpstreamProtocol::Messages,
            None,
            Some("key-value"),
            None,
        ),
        (
            OPENCODE_ZEN_ADAPTER_ID,
            UpstreamProtocol::GoogleGenerateContent,
            Some("Bearer key-value"),
            None,
            None,
        ),
    ];
    for (adapter_id, protocol, authorization, api_key, google_key) in cases {
        let request = super::super::apply_upstream_request_auth(
            adapter_id,
            reqwest::Client::new().post("https://example.test"),
            UpstreamAuthContext {
                auth_type: "bearer",
                auth_header: None,
                secret: Some("key-value"),
                oauth_auth: None,
                session_id: "ignored",
                protocol,
            },
        )
        .expect("upstream auth should be supported")
        .build()
        .expect("build authenticated request");
        assert_eq!(
            request
                .headers()
                .get(http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            authorization
        );
        assert_eq!(
            request
                .headers()
                .get("x-api-key")
                .and_then(|value| value.to_str().ok()),
            api_key
        );
        assert_eq!(
            request
                .headers()
                .get("x-goog-api-key")
                .and_then(|value| value.to_str().ok()),
            google_key
        );
        assert!(!request.url().as_str().contains("key-value"));
    }
}

#[test]
fn forwards_only_client_supplied_safe_opencode_session_ids() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-opencode-session",
        HeaderValue::from_static("session_01:stable"),
    );
    let request = apply_opencode_session_header(
        reqwest::Client::new().post("https://example.test"),
        &headers,
    )
    .build()
    .expect("build session request");
    assert_eq!(
        request
            .headers()
            .get("x-opencode-session")
            .and_then(|value| value.to_str().ok()),
        Some("session_01:stable")
    );

    headers.insert(
        "x-opencode-session",
        HeaderValue::from_static("invalid session"),
    );
    let request = apply_opencode_session_header(
        reqwest::Client::new().post("https://example.test"),
        &headers,
    )
    .build()
    .expect("build rejected session request");
    assert!(request.headers().get("x-opencode-session").is_none());
}

#[test]
fn open_code_go_adds_the_canonical_session_to_upstream_requests() {
    let mut client_headers = HeaderMap::new();
    client_headers.insert(
        "x-opencode-session",
        HeaderValue::from_static("client-session-01"),
    );
    let request = OPENCODE_GO_ADAPTER.apply_client_headers(
        reqwest::Client::new().post("https://example.test"),
        &client_headers,
    );
    let request = OPENCODE_GO_ADAPTER
        .apply_upstream_request_auth(
            request,
            UpstreamAuthContext {
                auth_type: "bearer",
                auth_header: None,
                secret: Some("key-value"),
                oauth_auth: None,
                session_id: "probe-session-01",
                protocol: UpstreamProtocol::Responses,
            },
        )
        .expect("OpenCode Go auth should be supported")
        .build()
        .expect("build OpenCode Go request");

    assert_eq!(
        request
            .headers()
            .get("x-opencode-session")
            .and_then(|value| value.to_str().ok()),
        Some("probe-session-01")
    );
    assert_eq!(
        request
            .headers()
            .get_all("x-opencode-session")
            .iter()
            .count(),
        1
    );
}

#[test]
fn open_code_go_rejects_invalid_canonical_sessions() {
    let result = OPENCODE_GO_ADAPTER.apply_upstream_request_auth(
        reqwest::Client::new().post("https://example.test"),
        UpstreamAuthContext {
            auth_type: "bearer",
            auth_header: None,
            secret: Some("key-value"),
            oauth_auth: None,
            session_id: "invalid session",
            protocol: UpstreamProtocol::ChatCompletions,
        },
    );

    assert_eq!(
        result.expect_err("invalid OpenCode Go session should be rejected"),
        "OpenCode Go provider session ID is invalid"
    );
}
