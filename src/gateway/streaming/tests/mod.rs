use super::*;

mod anthropic;
mod chat_stream;
mod google_stream;
mod responses;
mod status_code_error;
mod stream_preflight;
mod support;
mod terminal;
mod usage;

#[tokio::test]
async fn non_continuity_stream_emits_client_keepalive_during_a_silent_pause() {
    // Claude Code treats a silent SSE connection after partial output as a
    // malformed stream. Keep-alive comments must not depend on continuity.
    let chunks: UpstreamChunkStream = Box::pin(futures_util::stream::pending());
    let (_shutdown_sender, shutdown) = watch::channel(false);
    let mut reader = SseFrameReader::with_keepalive_interval(
        chunks,
        SseReaderTiming {
            idle_timeout: Duration::from_secs(60),
            overall_timeout: None,
            keepalive_interval: Duration::from_millis(5),
        },
        false,
        GatewayResourceLimits::default(),
        shutdown,
        None,
    );
    let event = tokio::time::timeout(Duration::from_secs(1), reader.next())
        .await
        .expect("keepalive arrives before any upstream chunk");
    assert!(matches!(event, SseReadEvent::KeepAlive));
}
