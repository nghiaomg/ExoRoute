use super::translation::stream_keepalive_frame;
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

#[test]
fn keepalive_frames_match_the_client_protocol() {
    assert_eq!(
        stream_keepalive_frame(Protocol::Messages),
        "event: ping\ndata: {\"type\":\"ping\"}\n\n"
    );
    assert_eq!(
        stream_keepalive_frame(Protocol::ChatCompletions),
        ": keep-alive\n\n"
    );
    assert_eq!(
        stream_keepalive_frame(Protocol::Responses),
        ": keep-alive\n\n"
    );
}

#[tokio::test]
async fn slow_first_chunk_still_delivers_the_frame_after_a_keepalive() {
    let chunks: UpstreamChunkStream = Box::pin(async_stream::stream! {
      tokio::time::sleep(Duration::from_millis(20)).await;
      yield Ok::<Bytes, reqwest::Error>(Bytes::from_static(b"data: {\"type\":\"ping\"}\n\n"));
    });
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
    let first = tokio::time::timeout(Duration::from_secs(1), reader.next())
        .await
        .expect("first event before the delayed chunk");
    assert!(matches!(first, SseReadEvent::KeepAlive));
    let mut saw_frame = false;
    for _ in 0..32 {
        let next = tokio::time::timeout(Duration::from_secs(1), reader.next())
            .await
            .expect("delayed chunk arrives");
        if matches!(next, SseReadEvent::Frame(_)) {
            saw_frame = true;
            break;
        }
    }
    assert!(saw_frame, "delayed frame arrives after keep-alives");
}
