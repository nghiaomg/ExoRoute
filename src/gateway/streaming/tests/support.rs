use super::*;

pub(super) use crate::support::mock_upstream::{MockResponse, spawn_mock_http_server};

pub(super) async fn run_preflight_test(
    body: &str,
    upstream_protocol: UpstreamProtocol,
    client_protocol: Protocol,
    resource_limits: GatewayResourceLimits,
) -> Result<String, String> {
    let (address, server) = spawn_mock_http_server(vec![MockResponse::sse(body)]).await;

    let upstream = reqwest::Client::new()
        .get(format!("http://{address}/stream"))
        .send()
        .await
        .expect("preflight test upstream response");
    let result = preflight_stream(
        upstream,
        upstream_protocol,
        client_protocol,
        "test-model",
        Duration::from_secs(1),
        resource_limits,
    )
    .await;
    let result = match result {
        Ok(mut chunks) => {
            let mut output = Vec::new();
            while let Some(chunk) = chunks.next().await {
                output.extend_from_slice(&chunk.expect("preflight replay chunk"));
            }
            Ok(String::from_utf8(output).expect("preflight replay body"))
        }
        Err(error) => Err(error),
    };
    server.await.expect("preflight test server");
    result
}

pub(super) async fn run_stream_translation_test_for(
    upstream_protocol: UpstreamProtocol,
    client_protocol: Protocol,
    body: &str,
) -> (String, StreamOutcome) {
    let (address, server) = spawn_mock_http_server(vec![MockResponse::sse(body)]).await;

    let upstream = reqwest::Client::new()
        .get(format!("http://{address}/stream"))
        .send()
        .await
        .expect("stream test upstream response");
    let (_shutdown_sender, shutdown) = watch::channel(false);
    let outcome = StreamOutcome::default();
    let response = stream_translation(
        upstream,
        StreamTranslationConfig {
            upstream_protocol,
            client_protocol,
            request_id: "stream-test".to_owned(),
            model: "cmc/model".to_owned(),
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
    let translated = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("translated stream body");
    server.await.expect("stream test server");
    (
        String::from_utf8(translated.to_vec()).expect("translated SSE text"),
        outcome,
    )
}

pub(super) async fn run_stream_translation_test(
    body: &str,
    continuity_enabled: bool,
    idle_timeout: Duration,
    overall_timeout: Option<Duration>,
    delay_before_body: Option<Duration>,
) -> (String, StreamOutcome) {
    let mut mock = MockResponse::sse(body);
    if let Some(delay) = delay_before_body {
        mock = mock.delay_before_body(delay);
    }
    let (address, server) = spawn_mock_http_server(vec![mock]).await;

    let upstream = reqwest::Client::new()
        .get(format!("http://{address}/stream"))
        .send()
        .await
        .expect("Responses stream test upstream response");
    let (_shutdown_sender, shutdown) = watch::channel(false);
    let outcome = StreamOutcome::default();
    let response = stream_translation(
        upstream,
        StreamTranslationConfig {
            upstream_protocol: UpstreamProtocol::Responses,
            client_protocol: Protocol::Responses,
            request_id: "responses-stream-test".to_owned(),
            model: "cmc/model".to_owned(),
            idle_timeout,
            continuity_enabled,
            overall_timeout,
            outcome: outcome.clone(),
            analytics: None,
            resource_limits: GatewayResourceLimits::default(),
            shutdown,
            log: None,
        },
    );
    let translated = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("translated Responses stream body");
    server.await.expect("Responses stream test server");
    (
        String::from_utf8(translated.to_vec()).expect("translated Responses SSE text"),
        outcome,
    )
}

/// Streams `body` over a real TCP upstream and then holds the connection
/// open (without closing) for `hold_after_body` so the reader observes a
/// transport idle timeout after the sent frames, or closes cleanly when
/// `None`. Returns the translated client SSE text and the stream outcome.
pub(super) async fn run_truncated_chat_stream_test(
    body: &str,
    hold_after_body: Option<Duration>,
    idle_timeout: Duration,
) -> (String, StreamOutcome) {
    let mut mock = MockResponse::sse(body);
    if let Some(hold) = hold_after_body {
        // Keep the socket open and silent so the gateway reader hits its
        // idle timeout while the connection is still established.
        mock = mock.hold_after_body(hold);
    }
    let (address, server) = spawn_mock_http_server(vec![mock]).await;

    let upstream = reqwest::Client::new()
        .get(format!("http://{address}/stream"))
        .send()
        .await
        .expect("truncated stream test upstream response");
    let (_shutdown_sender, shutdown) = watch::channel(false);
    let outcome = StreamOutcome::default();
    let response = stream_translation(
        upstream,
        StreamTranslationConfig {
            upstream_protocol: UpstreamProtocol::ChatCompletions,
            client_protocol: Protocol::ChatCompletions,
            request_id: "truncated-stream-test".to_owned(),
            model: "command-code/model".to_owned(),
            idle_timeout,
            continuity_enabled: false,
            overall_timeout: None,
            outcome: outcome.clone(),
            analytics: None,
            resource_limits: GatewayResourceLimits::default(),
            shutdown,
            log: None,
        },
    );
    let translated = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("truncated stream test body");
    server.await.expect("truncated stream test server");
    (
        String::from_utf8(translated.to_vec()).expect("truncated stream test SSE text"),
        outcome,
    )
}
