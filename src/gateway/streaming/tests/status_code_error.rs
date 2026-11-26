use super::*;

/// Feeds one upstream SSE data frame through the real translation driver
/// (no network hop) and returns the emitted client SSE text and outcome.
async fn translate_status_frame(frame: &str) -> (String, StreamOutcome) {
    let body = format!("data: {frame}\n\n");
    let (_shutdown_sender, shutdown) = watch::channel(false);
    let outcome = StreamOutcome::default();
    let response = stream_translation_from_chunks(
        Box::pin(futures_util::stream::iter(vec![
            Ok::<Bytes, reqwest::Error>(Bytes::from(body)),
        ])),
        StreamTranslationConfig {
            upstream_protocol: UpstreamProtocol::ChatCompletions,
            client_protocol: Protocol::ChatCompletions,
            request_id: "status-code-error".to_owned(),
            model: "test-model".to_owned(),
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
    let translated = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("translated stream body");
    (
        String::from_utf8(translated.to_vec()).expect("translated SSE text"),
        outcome,
    )
}

/// Pulls the `error.message` out of every `event: error` data line so the
/// assertions cover the immediate error event and the final failure event.
/// The latter is built from the same `final_error` value passed to
/// `log_failed`, so it also proves what is persisted to request logs.
fn error_messages(sse: &str) -> Vec<String> {
    sse.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .filter_map(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

#[tokio::test]
async fn status_code_value_error_redacts_credentials_from_client_events() {
    let frame = json!({
        "statusCodeValue": 500,
        "body": "Authorization: Bearer sk-live-1234567890"
    })
    .to_string();
    let (sse, outcome) = translate_status_frame(&frame).await;
    let messages = error_messages(&sse);
    assert!(!messages.is_empty(), "no error event emitted: {sse}");
    for message in &messages {
        assert!(!message.contains("sk-live"), "{message}");
        assert!(!message.contains("Bearer"), "{message}");
        assert!(message.contains("[redacted]"), "{message}");
        assert!(message.contains("Authorization=[redacted]"), "{message}");
        assert!(message.contains("upstream status 500"), "{message}");
    }
    assert!(outcome.failed());
    assert!(!outcome.completed());
}

#[tokio::test]
async fn status_code_value_error_is_bounded_and_utf8_safe() {
    // Multi-byte characters make a byte-based truncation split a code point;
    // the cap must be expressed in chars and stay at or below 2048.
    let oversized = "é".repeat(4096);
    let frame = json!({"statusCodeValue": 500, "body": oversized}).to_string();
    let (sse, outcome) = translate_status_frame(&frame).await;
    let messages = error_messages(&sse);
    assert!(!messages.is_empty(), "no error event emitted: {sse}");
    for message in &messages {
        assert!(
            message.chars().count() <= MAX_STREAM_ERROR_CHARS,
            "message length {} exceeds {MAX_STREAM_ERROR_CHARS}: {message}",
            message.chars().count()
        );
        assert!(
            !message.contains('\u{fffd}'),
            "invalid UTF-8 boundary: {message}"
        );
    }
    assert!(outcome.failed());
}

#[tokio::test]
async fn status_code_value_error_handles_empty_null_and_non_string_body() {
    let cases = [
        (
            json!({"statusCodeValue": 500, "body": ""}),
            "empty string body",
        ),
        (json!({"statusCodeValue": 500, "body": null}), "null body"),
        (
            json!({"statusCodeValue": 500, "body": {"Authorization": "Bearer sk-live-1234567890"}}),
            "non-string object body",
        ),
    ];
    for (frame, label) in cases {
        let (sse, outcome) = translate_status_frame(&frame.to_string()).await;
        let messages = error_messages(&sse);
        assert!(!messages.is_empty(), "{label}: no error event: {sse}");
        for message in &messages {
            assert!(!message.contains("sk-live"), "{label}: {message}");
            assert!(!message.contains("Bearer"), "{label}: {message}");
        }
        assert!(outcome.failed(), "{label}");
    }
}

#[tokio::test]
async fn missing_status_code_value_does_not_echo_upstream_body() {
    let frame = json!({
        "body": "Authorization: Bearer sk-live-1234567890",
        "choices": []
    })
    .to_string();
    let (sse, _outcome) = translate_status_frame(&frame).await;
    assert!(!sse.contains("sk-live"), "{sse}");
    assert!(!sse.contains("Bearer"), "{sse}");
}

#[tokio::test]
async fn concurrent_status_code_value_errors_do_not_leak_across_streams() {
    let secret_frame = json!({
        "statusCodeValue": 500,
        "body": "Authorization: Bearer sk-live-should-never-appear"
    })
    .to_string();
    let plain_frame = json!({"statusCodeValue": 503, "body": "upstream unavailable"}).to_string();
    let (secret_sse, plain_sse) = tokio::join!(
        translate_status_frame(&secret_frame),
        translate_status_frame(&plain_frame)
    );
    assert!(!secret_sse.0.contains("sk-live"), "{}", secret_sse.0);
    assert!(!secret_sse.0.contains("Bearer"), "{}", secret_sse.0);
    assert!(
        error_messages(&secret_sse.0)
            .iter()
            .all(|message| message.contains("[redacted]"))
    );
    assert!(!plain_sse.0.contains("sk-live"), "{}", plain_sse.0);
    assert!(
        error_messages(&plain_sse.0)
            .iter()
            .all(|message| message.contains("upstream status 503"))
    );
}
