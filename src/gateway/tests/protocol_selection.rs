use super::*;

#[test]
fn codex_requests_force_safe_responses_streaming() {
    let mut body = json!({
        "model":"gpt-codex",
        "input":[],
        "instructions":"",
        "stream":false,
        "store":true,
        "temperature":0.2,
        "previous_response_id":"resp_old"
    });
    prepare_codex_request(&mut body, "session-test");
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    assert_eq!(body["prompt_cache_key"], "session-test");
    assert_eq!(body["instructions"], "You are a helpful assistant.");
    assert!(body.get("temperature").is_none());
    assert!(body.get("previous_response_id").is_none());
}

#[test]
fn codex_terminal_error_is_distinguished_from_an_incomplete_stream() {
    let terminal_failure = provider_adapters::parse_adapter_event_stream(
            b"event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"message\":\"model at capacity\"}}}\n\n",
        ).expect_err("failed terminal event");
    assert!(provider_adapters::adapter_sse_error_can_fail_over(
        false,
        &terminal_failure
    ));
    assert!(!provider_adapters::adapter_sse_error_can_fail_over(
        true,
        &terminal_failure
    ));

    let incomplete = provider_adapters::parse_adapter_event_stream(
            b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"status\":\"in_progress\"}}\n\n",
        ).expect_err("stream ended before completion");
    assert!(!provider_adapters::adapter_sse_error_can_fail_over(
        false,
        &incomplete
    ));
}

#[test]
fn provider_error_details_keep_diagnostics_without_credentials_or_request_content() {
    let detail = sanitize_provider_error_body(
            br#"{"error":{"message":"key expired","code":"invalid_api_key"},"api_key":"secret-key","request":{"messages":[{"content":"private prompt"}]}}"#,
        );
    assert!(detail.contains("key expired"));
    assert!(detail.contains("invalid_api_key"));
    assert!(detail.contains("[redacted]"));
    assert!(!detail.contains("secret-key"));
    assert!(!detail.contains("private prompt"));
}

#[test]
fn decode_failure_messages_keep_redacted_provider_diagnostics() {
    let detail = provider_decode_failure_message(
        "chat response has no choices",
        &json!({"error":{"message":"model 'gpt-x' is not available on this plan"}}),
    );
    assert_eq!(
        detail,
        "could not decode provider response: chat response has no choices (provider said: model 'gpt-x' is not available on this plan)"
    );

    // A provider that echoes credential material into its diagnostic keeps the
    // redaction every other provider error body gets.
    let redacted = provider_decode_failure_message(
        "chat response has no choices",
        &json!({"error":{"message":"bad key api_key=sk-live-1234"}}),
    );
    assert!(redacted.contains("api_key=[redacted]"), "{redacted}");
    assert!(!redacted.contains("sk-live-1234"), "{redacted}");

    // A payload that reports no diagnostic of its own keeps the plain summary.
    let plain = provider_decode_failure_message(
        "chat response has no choices",
        &json!({"success":true,"data":{}}),
    );
    assert_eq!(
        plain,
        "could not decode provider response: chat response has no choices"
    );

    // A long diagnostic is truncated and flattened to one line.
    let long = provider_decode_failure_message(
        "chat response has no choices",
        &json!({"detail": format!("first line\n{}", "a".repeat(600))}),
    );
    assert!(long.ends_with("...)"), "{long}");
    assert!(!long.contains('\n'), "{long}");
    // The diagnostic is capped at 300 characters, so even the longest provider
    // message keeps the decode failure to one short, client-visible line.
    assert!(
        long.len() < 400,
        "diagnostic must stay bounded: {}",
        long.len()
    );
}

#[test]
fn endpoint_url_respects_versioned_base_url() {
    assert_eq!(
        endpoint_url("https://example.com/v1/", Protocol::ChatCompletions)
            .expect("URL")
            .as_str(),
        "https://example.com/v1/chat/completions"
    );
    assert_eq!(
        endpoint_url("https://example.com", Protocol::Responses)
            .expect("URL")
            .as_str(),
        "https://example.com/v1/responses"
    );
}
