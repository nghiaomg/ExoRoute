//! Canonical delta extraction from upstream streaming frames.
//!
//! Every supported upstream protocol sends one chat turn as a bounded SSE
//! event sequence. This module turns each frame into zero or one canonical
//! delta — a text fragment, a reasoning fragment, a usage snapshot, or the
//! terminal event with finish reason — without persisting anything. Unknown
//! event types and unknown shapes yield no delta and keep the stream alive,
//! matching the gateway's tolerant translation behavior.

use crate::protocol::UpstreamProtocol;
use serde_json::Value;

/// One canonical extraction result from a single upstream frame.
pub(super) enum UpstreamDelta {
    Text(String),
    Reasoning(String),
    /// A usage or model snapshot that does not end the turn.
    Usage {
        model: String,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    },
    /// The turn's terminal event; whatever arrives after it is ignored.
    Terminal {
        model: String,
        finish_reason: String,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    },
}

/// Extracts the canonical delta carried by one upstream frame, if any.
pub(super) fn extract_upstream_delta(
    protocol: UpstreamProtocol,
    value: &Value,
) -> Option<UpstreamDelta> {
    match protocol {
        UpstreamProtocol::ChatCompletions => chat_completions_delta(value),
        UpstreamProtocol::Responses => responses_delta(value),
        UpstreamProtocol::Messages => messages_delta(value),
        UpstreamProtocol::GoogleGenerateContent => google_delta(value),
    }
}

fn usage_of(value: &Value, input_key: &str, output_key: &str) -> (Option<u64>, Option<u64>) {
    let usage = value.get("usage");
    (
        usage
            .and_then(|usage| usage.get(input_key))
            .and_then(Value::as_u64),
        usage
            .and_then(|usage| usage.get(output_key))
            .and_then(Value::as_u64),
    )
}

/// Chat Completions: `choices[0].delta.content` per frame; the frame carrying
/// `finish_reason` is terminal and also carries the full usage object.
fn chat_completions_delta(value: &Value) -> Option<UpstreamDelta> {
    let choice = value.get("choices")?.as_array()?.first()?;
    if let Some(finish_reason) = choice.get("finish_reason").and_then(Value::as_str) {
        let finish_reason = match finish_reason {
            "length" | "max_tokens" => "length",
            "tool_calls" | "function_call" => "tool_use",
            other => other,
        };
        let (input_tokens, output_tokens) = usage_of(value, "prompt_tokens", "completion_tokens");
        return Some(UpstreamDelta::Terminal {
            model: value
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            finish_reason: finish_reason.to_owned(),
            input_tokens,
            output_tokens,
        });
    }
    let text = choice
        .get("delta")?
        .get("content")
        .and_then(Value::as_str)?;
    (!text.is_empty()).then(|| UpstreamDelta::Text(text.to_owned()))
}

/// Responses: typed output events; `response.completed` is terminal and
/// carries the full response object with usage and status.
fn responses_delta(value: &Value) -> Option<UpstreamDelta> {
    let event_type = value.get("type").and_then(Value::as_str)?;
    match event_type {
        "response.output_text.delta" => {
            let text = value.get("delta").and_then(Value::as_str)?;
            (!text.is_empty()).then(|| UpstreamDelta::Text(text.to_owned()))
        }
        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
            let text = value.get("delta").and_then(Value::as_str)?;
            (!text.is_empty()).then(|| UpstreamDelta::Reasoning(text.to_owned()))
        }
        "response.completed" => {
            let response = value.get("response")?;
            Some(UpstreamDelta::Terminal {
                model: response
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                finish_reason: responses_finish_reason(response),
                input_tokens: response
                    .pointer("/usage/input_tokens")
                    .and_then(Value::as_u64),
                output_tokens: response
                    .pointer("/usage/output_tokens")
                    .and_then(Value::as_u64),
            })
        }
        "response.incomplete" => Some(UpstreamDelta::Terminal {
            model: value
                .pointer("/response/model")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            finish_reason: "length".to_owned(),
            input_tokens: value
                .pointer("/response/usage/input_tokens")
                .and_then(Value::as_u64),
            output_tokens: value
                .pointer("/response/usage/output_tokens")
                .and_then(Value::as_u64),
        }),
        "response.failed" | "error" => Some(UpstreamDelta::Terminal {
            model: String::new(),
            finish_reason: String::new(),
            input_tokens: None,
            output_tokens: None,
        }),
        _ => None,
    }
}

fn responses_finish_reason(response: &Value) -> String {
    match response.get("status").and_then(Value::as_str) {
        Some("completed") => "stop".to_owned(),
        Some("incomplete") => "length".to_owned(),
        _ => String::new(),
    }
}

/// Messages: typed frames. `content_block_delta` carries text or thinking
/// fragments, `message_start` an input-usage snapshot, and `message_delta`
/// the terminal stop reason plus the output-token count.
fn messages_delta(value: &Value) -> Option<UpstreamDelta> {
    match value.get("type").and_then(Value::as_str)? {
        "content_block_delta" => {
            let delta = value.get("delta")?;
            match delta.get("type").and_then(Value::as_str)? {
                "text_delta" => {
                    let text = delta.get("text").and_then(Value::as_str)?;
                    (!text.is_empty()).then(|| UpstreamDelta::Text(text.to_owned()))
                }
                "thinking_delta" => {
                    let text = delta.get("thinking").and_then(Value::as_str)?;
                    (!text.is_empty()).then(|| UpstreamDelta::Reasoning(text.to_owned()))
                }
                _ => None,
            }
        }
        "message_start" => {
            let input_tokens = value
                .pointer("/message/usage/input_tokens")
                .and_then(Value::as_u64);
            let model = value
                .pointer("/message/model")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            (input_tokens.is_some() || !model.is_empty()).then_some(UpstreamDelta::Usage {
                model,
                input_tokens,
                output_tokens: None,
            })
        }
        "message_delta" => {
            let stop_reason = value.pointer("/delta/stop_reason").and_then(Value::as_str);
            let output_tokens = value
                .pointer("/usage/output_tokens")
                .and_then(Value::as_u64);
            match stop_reason {
                Some(stop_reason) => {
                    let finish_reason = match stop_reason {
                        "end_turn" | "stop_sequence" => "stop",
                        "max_tokens" => "length",
                        "tool_use" => "tool_use",
                        other => other,
                    };
                    Some(UpstreamDelta::Terminal {
                        model: String::new(),
                        finish_reason: finish_reason.to_owned(),
                        input_tokens: None,
                        output_tokens,
                    })
                }
                None => output_tokens.map(|output_tokens| UpstreamDelta::Usage {
                    model: String::new(),
                    input_tokens: None,
                    output_tokens: Some(output_tokens),
                }),
            }
        }
        _ => None,
    }
}

/// Google GenerateContent: each streamed chunk is a JSON object with
/// candidates; the final chunk carries `finishReason` and usage metadata.
fn google_delta(value: &Value) -> Option<UpstreamDelta> {
    let candidate = value
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())?;
    if let Some(finish_reason) = candidate.get("finishReason").and_then(Value::as_str) {
        let finish_reason = match finish_reason {
            "STOP" => "stop",
            "MAX_TOKENS" => "length",
            "SAFETY" | "PROHIBITED_CONTENT" | "BLOCKLIST" => "content_filter",
            other => other,
        };
        return Some(UpstreamDelta::Terminal {
            model: value
                .get("modelVersion")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            finish_reason: finish_reason.to_owned(),
            input_tokens: value
                .pointer("/usageMetadata/promptTokenCount")
                .and_then(Value::as_u64),
            output_tokens: value
                .pointer("/usageMetadata/candidatesTokenCount")
                .and_then(Value::as_u64),
        });
    }
    let text = candidate
        .pointer("/content/parts/0/text")
        .and_then(Value::as_str)?;
    (!text.is_empty()).then(|| UpstreamDelta::Text(text.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chat_completions_frames_yield_text_then_terminal_with_usage() {
        let delta = extract_upstream_delta(
            UpstreamProtocol::ChatCompletions,
            &json!({"choices":[{"delta":{"content":"Hel"}}]}),
        );
        assert!(matches!(delta, Some(UpstreamDelta::Text(text)) if text == "Hel"));

        // Empty content fragments produce nothing.
        let empty = extract_upstream_delta(
            UpstreamProtocol::ChatCompletions,
            &json!({"choices":[{"delta":{"content":""}}]}),
        );
        assert!(empty.is_none());

        let terminal = extract_upstream_delta(
            UpstreamProtocol::ChatCompletions,
            &json!({
                "model": "sample-model",
                "choices":[{"delta":{},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":3,"completion_tokens":5}
            }),
        );
        let Some(UpstreamDelta::Terminal {
            model,
            finish_reason,
            input_tokens,
            output_tokens,
        }) = terminal
        else {
            panic!("finish_reason frame must be terminal");
        };
        assert_eq!(model, "sample-model");
        assert_eq!(finish_reason, "stop");
        assert_eq!(input_tokens, Some(3));
        assert_eq!(output_tokens, Some(5));

        let length = extract_upstream_delta(
            UpstreamProtocol::ChatCompletions,
            &json!({"choices":[{"delta":{},"finish_reason":"length"}]}),
        );
        assert!(matches!(
            length,
            Some(UpstreamDelta::Terminal { finish_reason, .. }) if finish_reason == "length"
        ));
    }

    #[test]
    fn messages_frames_yield_text_reasoning_usage_and_terminal() {
        let text = extract_upstream_delta(
            UpstreamProtocol::Messages,
            &json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"Hi"}}),
        );
        assert!(matches!(text, Some(UpstreamDelta::Text(t)) if t == "Hi"));

        let thinking = extract_upstream_delta(
            UpstreamProtocol::Messages,
            &json!({"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"hmm"}}),
        );
        assert!(matches!(thinking, Some(UpstreamDelta::Reasoning(t)) if t == "hmm"));

        let start = extract_upstream_delta(
            UpstreamProtocol::Messages,
            &json!({"type":"message_start","message":{"model":"claude-x","usage":{"input_tokens":11}}}),
        );
        let Some(UpstreamDelta::Usage {
            model,
            input_tokens,
            ..
        }) = start
        else {
            panic!("message_start must carry a usage snapshot");
        };
        assert_eq!(model, "claude-x");
        assert_eq!(input_tokens, Some(11));

        let terminal = extract_upstream_delta(
            UpstreamProtocol::Messages,
            &json!({"type":"message_delta","delta":{"stop_reason":"max_tokens"},"usage":{"output_tokens":7}}),
        );
        assert!(matches!(
            terminal,
            Some(UpstreamDelta::Terminal { finish_reason, output_tokens: Some(7), .. })
                if finish_reason == "length"
        ));
    }

    #[test]
    fn responses_frames_yield_text_reasoning_and_completed_terminal() {
        let text = extract_upstream_delta(
            UpstreamProtocol::Responses,
            &json!({"type":"response.output_text.delta","delta":"hi"}),
        );
        assert!(matches!(text, Some(UpstreamDelta::Text(t)) if t == "hi"));

        let reasoning = extract_upstream_delta(
            UpstreamProtocol::Responses,
            &json!({"type":"response.reasoning_summary_text.delta","delta":"because"}),
        );
        assert!(matches!(reasoning, Some(UpstreamDelta::Reasoning(t)) if t == "because"));

        let terminal = extract_upstream_delta(
            UpstreamProtocol::Responses,
            &json!({
                "type":"response.completed",
                "response":{
                    "model":"gpt-x",
                    "status":"completed",
                    "usage":{"input_tokens":4,"output_tokens":9}
                }
            }),
        );
        assert!(matches!(
            terminal,
            Some(UpstreamDelta::Terminal { model, finish_reason, input_tokens: Some(4), output_tokens: Some(9) })
                if model == "gpt-x" && finish_reason == "stop"
        ));

        let incomplete = extract_upstream_delta(
            UpstreamProtocol::Responses,
            &json!({"type":"response.incomplete","response":{"usage":{"output_tokens":2}}}),
        );
        assert!(matches!(
            incomplete,
            Some(UpstreamDelta::Terminal { finish_reason, .. }) if finish_reason == "length"
        ));
    }

    #[test]
    fn google_frames_yield_text_and_terminal() {
        let text = extract_upstream_delta(
            UpstreamProtocol::GoogleGenerateContent,
            &json!({"candidates":[{"content":{"parts":[{"text":"xin chao"}]}}]}),
        );
        assert!(matches!(text, Some(UpstreamDelta::Text(t)) if t == "xin chao"));

        let terminal = extract_upstream_delta(
            UpstreamProtocol::GoogleGenerateContent,
            &json!({
                "modelVersion":"gemini-x",
                "candidates":[{"finishReason":"STOP"}],
                "usageMetadata":{"promptTokenCount":6,"candidatesTokenCount":8}
            }),
        );
        assert!(matches!(
            terminal,
            Some(UpstreamDelta::Terminal { model, finish_reason, input_tokens: Some(6), output_tokens: Some(8) })
                if model == "gemini-x" && finish_reason == "stop"
        ));
    }

    #[test]
    fn unknown_frames_and_errors_are_tolerated() {
        assert!(
            extract_upstream_delta(
                UpstreamProtocol::ChatCompletions,
                &json!({"choices":[{"delta":{"role":"assistant"}}]}),
            )
            .is_none()
        );
        assert!(
            extract_upstream_delta(UpstreamProtocol::Messages, &json!({"type":"ping"}),).is_none()
        );
        assert!(
            extract_upstream_delta(
                UpstreamProtocol::Responses,
                &json!({"type":"response.output_item.added","item":{}}),
            )
            .is_none()
        );
    }
}
