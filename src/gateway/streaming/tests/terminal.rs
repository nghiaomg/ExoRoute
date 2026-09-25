use super::support::*;
use super::*;

#[tokio::test]
async fn terminal_stream_without_output_is_failed_instead_of_completed() {
    let (address, server) =
        spawn_mock_http_server(vec![MockResponse::sse("data: [DONE]\n\n")]).await;

    let upstream = reqwest::Client::new()
        .get(format!("http://{address}/stream"))
        .send()
        .await
        .expect("empty upstream stream response");
    let (_shutdown_sender, shutdown) = watch::channel(false);
    let outcome = StreamOutcome::default();
    let response = stream_translation(
        upstream,
        StreamTranslationConfig {
            upstream_protocol: UpstreamProtocol::ChatCompletions,
            client_protocol: Protocol::ChatCompletions,
            request_id: "empty-request".to_owned(),
            model: "empty-model".to_owned(),
            idle_timeout: Duration::from_secs(1),
            continuity_enabled: false,
            overall_timeout: None,
            outcome: outcome.clone(),
            analytics: None,
            resource_limits: GatewayResourceLimits::default(),
            shutdown,
            log: None,
        },
    );
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("translated empty stream body");
    server.await.expect("empty stream test server");

    let body = String::from_utf8(body.to_vec()).expect("translated SSE text");
    assert!(body.contains("\"type\":\"upstream_error\""), "{body}");
    // The failure event must still be followed by the Chat Completions
    // terminal sentinel so client SSE parsers end on a deliberate
    // terminator instead of treating the failure as a dropped connection.
    assert!(body.ends_with("data: [DONE]\n\n"), "{body}");
    assert!(outcome.failed());
    assert!(!outcome.completed());
}

#[test]
fn provider_stream_error_keeps_bounded_redacted_diagnostics() {
    let error = extract_stream_error(
        "error",
        &json!({
            "type":"error",
            "error":{
                "message":"upstream rejected the request",
                "code":"provider_overloaded",
                "api_key":"must-not-leak"
            }
        }),
    );
    assert!(error.contains("upstream rejected the request"));
    assert!(error.contains("provider_overloaded"));
    assert!(!error.contains("must-not-leak"));
    assert!(
        sanitize_stream_error_detail(&"x".repeat(MAX_STREAM_ERROR_CHARS + 100)).len()
            <= MAX_STREAM_ERROR_CHARS
    );
}

#[tokio::test]
async fn continuity_timeout_is_not_hidden_by_local_heartbeats() {
    let (body, outcome) = run_stream_translation_test(
        "event: response.created\ndata: {\"type\":\"response.created\"}\n\n",
        true,
        Duration::from_secs(1),
        Some(Duration::from_millis(50)),
        Some(Duration::from_millis(200)),
    )
    .await;
    assert!(
        body.contains("stream continuity exceeded its overall deadline"),
        "{body}"
    );
    assert!(body.contains("upstream_error"), "{body}");
    assert!(outcome.failed());
    assert!(!outcome.completed());
}
