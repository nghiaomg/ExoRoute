use super::preflight::find_sse_boundary;
use super::{DynamicSemaphore, DynamicSemaphorePermit, GatewayResourceLimits, UpstreamChunkStream};
use crate::provider_adapters;
use bytes::Bytes;
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// A frame produced by the transport layer before protocol-specific decoding.
/// The permit stays attached to the frame so semantic processing owns its
/// lifetime and cancellation still releases the SSE processing slot.
pub(crate) struct RawSseFrame {
    pub(crate) event_name: String,
    pub(crate) data: String,
    pub(crate) processing_permit: Option<DynamicSemaphorePermit>,
}

pub(crate) enum SseReadEvent {
    KeepAlive,
    Frame(RawSseFrame),
    End,
    Error(String),
    Shutdown,
}

/// Reads bounded SSE frames from an upstream byte stream.
///
/// This type owns transport concerns only: upstream idle/continuity deadlines,
/// shutdown observation, keep-alive comments, buffering, frame limits and
/// UTF-8/SSE field extraction. Protocol semantics remain in `translate.rs`.
pub(crate) struct SseFrameReader {
    chunks: UpstreamChunkStream,
    buffer: Vec<u8>,
    idle_timeout: Duration,
    continuity_enabled: bool,
    overall_deadline: Option<tokio::time::Instant>,
    upstream_idle_deadline: tokio::time::Instant,
    resource_limits: GatewayResourceLimits,
    shutdown: watch::Receiver<bool>,
    processing_slots: Option<Arc<DynamicSemaphore>>,
    keepalive: tokio::time::Interval,
}

impl SseFrameReader {
    pub(crate) fn new(
        chunks: UpstreamChunkStream,
        idle_timeout: Duration,
        continuity_enabled: bool,
        overall_timeout: Option<Duration>,
        resource_limits: GatewayResourceLimits,
        shutdown: watch::Receiver<bool>,
        processing_slots: Option<Arc<DynamicSemaphore>>,
    ) -> Self {
        let now = tokio::time::Instant::now();
        let mut keepalive =
            tokio::time::interval_at(now + Duration::from_secs(15), Duration::from_secs(15));
        keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        Self {
            chunks,
            buffer: Vec::new(),
            idle_timeout,
            continuity_enabled,
            overall_deadline: overall_timeout.map(|timeout| now + timeout),
            upstream_idle_deadline: now + idle_timeout,
            resource_limits,
            shutdown,
            processing_slots,
            keepalive,
        }
    }

    pub(crate) async fn next(&mut self) -> SseReadEvent {
        loop {
            if *self.shutdown.borrow() {
                return SseReadEvent::Shutdown;
            }

            let next_chunk = if self.continuity_enabled {
                let (timeout_deadline, timeout_message) = match self.overall_deadline {
                    Some(deadline) if deadline <= self.upstream_idle_deadline => {
                        (deadline, "stream continuity exceeded its overall deadline")
                    }
                    _ => (self.upstream_idle_deadline, "upstream stream idle timeout"),
                };
                let timeout_sleep = tokio::time::sleep_until(timeout_deadline);
                tokio::pin!(timeout_sleep);
                tokio::select! {
                    biased;
                    _ = self.shutdown.changed() => return SseReadEvent::Shutdown,
                    chunk = self.chunks.next() => chunk,
                    _ = self.keepalive.tick() => return SseReadEvent::KeepAlive,
                    _ = &mut timeout_sleep => {
                        return SseReadEvent::Error(timeout_message.to_owned());
                    }
                }
            } else {
                tokio::select! {
                    biased;
                    _ = self.shutdown.changed() => return SseReadEvent::Shutdown,
                    result = tokio::time::timeout(self.idle_timeout, self.chunks.next()) => {
                        match result {
                            Ok(chunk) => chunk,
                            Err(_) => return SseReadEvent::Error("upstream stream idle timeout".to_owned()),
                        }
                    }
                }
            };

            let mut flush_final_frame = false;
            let chunk = match next_chunk {
                Some(Ok(value)) => {
                    if self.continuity_enabled {
                        self.upstream_idle_deadline =
                            tokio::time::Instant::now() + self.idle_timeout;
                    }
                    value
                }
                Some(Err(error)) => {
                    return SseReadEvent::Error(format!(
                        "upstream stream read failed: {}",
                        provider_adapters::upstream_transport_error_message(&error)
                    ));
                }
                None => {
                    if self.buffer.is_empty() {
                        return SseReadEvent::End;
                    }
                    if self.resource_limits.sse_buffer_limit_bytes != 0
                        && self.buffer.len().saturating_add(2)
                            > self.resource_limits.sse_buffer_limit_bytes
                    {
                        return SseReadEvent::Error(
                            "upstream SSE buffer exceeded the configured limit".to_owned(),
                        );
                    }
                    // Some providers close immediately after the final SSE
                    // payload and omit the blank-line delimiter.
                    self.buffer.extend_from_slice(b"\n\n");
                    flush_final_frame = true;
                    Bytes::new()
                }
            };

            if !flush_final_frame
                && self.resource_limits.sse_buffer_limit_bytes != 0
                && self.buffer.len().saturating_add(chunk.len())
                    > self.resource_limits.sse_buffer_limit_bytes
            {
                return SseReadEvent::Error(
                    "upstream SSE buffer exceeded the configured limit".to_owned(),
                );
            }
            if !flush_final_frame {
                self.buffer.extend_from_slice(&chunk);
            }

            while let Some((boundary, delimiter_len)) = find_sse_boundary(&self.buffer) {
                if self.resource_limits.sse_frame_limit_bytes != 0
                    && boundary.saturating_add(delimiter_len)
                        > self.resource_limits.sse_frame_limit_bytes
                {
                    return SseReadEvent::Error(
                        "upstream SSE frame exceeded the configured limit".to_owned(),
                    );
                }

                let processing_permit = match self.processing_slots.as_ref() {
                    Some(slots) => {
                        let slots = slots.clone();
                        Some(tokio::select! {
                            biased;
                            _ = self.shutdown.changed() => return SseReadEvent::Shutdown,
                            permit = slots.acquire_owned() => permit,
                        })
                    }
                    None => None,
                };
                let frame_bytes = self
                    .buffer
                    .drain(..boundary + delimiter_len)
                    .collect::<Vec<_>>();
                let frame = match String::from_utf8(frame_bytes) {
                    Ok(frame) => frame,
                    Err(_) => {
                        return SseReadEvent::Error(
                            "upstream sent invalid UTF-8 SSE data".to_owned(),
                        );
                    }
                };
                let mut event_name = String::new();
                let mut data = String::new();
                let mut saw_data = false;
                for line in frame.lines() {
                    if let Some(value) = line.strip_prefix("event:") {
                        event_name = value.trim().to_owned();
                    }
                    if let Some(value) = line.strip_prefix("data:") {
                        if saw_data {
                            data.push('\n');
                        }
                        data.push_str(value.trim_start());
                        saw_data = true;
                    }
                }
                if data.is_empty() {
                    drop(processing_permit);
                    continue;
                }
                return SseReadEvent::Frame(RawSseFrame {
                    event_name,
                    data,
                    processing_permit,
                });
            }
        }
    }
}
