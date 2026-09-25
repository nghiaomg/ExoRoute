use super::*;

use serde_json::Value;

/// How the frame loop ended for this stream.
#[derive(Clone, PartialEq, Eq)]
pub(in crate::gateway::streaming) enum StreamTerminal {
    /// A frame completed the upstream stream; finalization runs normally.
    Completed,
    /// The loop ended without an error (EOF or a rescued transport error);
    /// recovery logic decides whether this is a completion.
    CleanEnd,
    /// The loop ended on a failure carrying this message.
    Error(String),
    /// Server shutdown; nothing is emitted or logged.
    Shutdown,
}

/// A failure captured inside a frame handler; the driver owns the single
/// error-emission path.
pub(in crate::gateway::streaming) struct StreamError {
    pub(in crate::gateway::streaming) message: String,
}

impl StreamError {
    pub(in crate::gateway::streaming) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// One decoded upstream SSE frame with its raw event name.
pub(in crate::gateway::streaming) struct StreamFrame {
    pub(in crate::gateway::streaming) event_name: String,
    pub(in crate::gateway::streaming) value: Value,
}

impl StreamFrame {
    pub(in crate::gateway::streaming) fn event_type(&self) -> &str {
        self.value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    /// Payload `type` field, falling back to the SSE `event:` name.
    pub(in crate::gateway::streaming) fn response_event_type(&self) -> &str {
        let event_type = self.event_type();
        if event_type.is_empty() {
            self.event_name.as_str()
        } else {
            event_type
        }
    }
}

/// Accumulated translation state for one upstream stream. Frame handlers
/// mutate it; the driver's finalization pass reads it afterwards.
#[derive(Default)]
pub(in crate::gateway::streaming) struct TranslationState {
    pub(in crate::gateway::streaming) upstream_id: String,
    pub(in crate::gateway::streaming) sent_start: bool,
    pub(in crate::gateway::streaming) completed: bool,
    pub(in crate::gateway::streaming) saw_finish_signal: bool,
    pub(in crate::gateway::streaming) saw_output: bool,
    pub(in crate::gateway::streaming) input_tokens: u64,
    pub(in crate::gateway::streaming) output_tokens: u64,
    pub(in crate::gateway::streaming) cached_tokens: u64,
    pub(in crate::gateway::streaming) cache_input_tokens: u64,
    pub(in crate::gateway::streaming) cost_micro_usd: Option<i64>,
    pub(in crate::gateway::streaming) has_usage: bool,
    pub(in crate::gateway::streaming) finish_reason: String,
    pub(in crate::gateway::streaming) response_output: Option<Vec<Value>>,
    pub(in crate::gateway::streaming) responses_accumulator: ResponsesStreamAccumulator,
    pub(in crate::gateway::streaming) forwarded_response_tool_events: bool,
    pub(in crate::gateway::streaming) google_tool_calls: Vec<GoogleStreamToolCall>,
    pub(in crate::gateway::streaming) google_message_text_started: bool,
    pub(in crate::gateway::streaming) google_responses_text: String,
    pub(in crate::gateway::streaming) messages_blocks: Vec<MessagesStreamBlock>,
    pub(in crate::gateway::streaming) messages_next_output_index: usize,
    pub(in crate::gateway::streaming) messages_open_blocks: Vec<usize>,
    pub(in crate::gateway::streaming) messages_text_started: bool,
    pub(in crate::gateway::streaming) chat_stream_text_sent: bool,
    pub(in crate::gateway::streaming) chat_tool_calls: ChatStreamToolCallAccumulator,
}

impl TranslationState {
    pub(in crate::gateway::streaming) fn new(upstream_id: String) -> Self {
        Self {
            upstream_id,
            finish_reason: "stop".to_owned(),
            ..Self::default()
        }
    }

    /// Records extracted token/cost usage on the state and analytics.
    pub(in crate::gateway::streaming) fn note_usage(
        &mut self,
        analytics: &Option<RequestAnalytics>,
        input: Option<u64>,
        output: Option<u64>,
        cached: Option<u64>,
        cache_input: Option<u64>,
    ) {
        if let Some(tokens) = input {
            self.input_tokens = tokens;
            self.has_usage = true;
        }
        if let Some(tokens) = output {
            self.output_tokens = tokens;
            self.has_usage = true;
        }
        if let Some(tokens) = cached {
            self.cached_tokens = tokens;
        }
        if let Some(tokens) = cache_input {
            self.cache_input_tokens = tokens;
        }
        if let Some(analytics) = analytics {
            analytics.set_token_usage(input, output);
            analytics.set_cached_token_usage(cached, cache_input);
        }
    }
}
