use super::*;

pub(crate) fn extract_stream_text(
    protocol: UpstreamProtocol,
    event: &str,
    value: &Value,
) -> Option<String> {
    match protocol {
        UpstreamProtocol::ChatCompletions => value
            .get("choices")?
            .as_array()?
            .first()?
            .get("delta")?
            .get("content")?
            .as_str()
            .map(str::to_owned),
        UpstreamProtocol::Responses => {
            if event.ends_with("output_text.delta")
                || value.get("type").and_then(Value::as_str) == Some("response.output_text.delta")
            {
                value.get("delta")?.as_str().map(str::to_owned)
            } else {
                None
            }
        }
        UpstreamProtocol::Messages => {
            if event == "content_block_delta"
                || value.get("type").and_then(Value::as_str) == Some("content_block_delta")
            {
                value.get("delta")?.get("text")?.as_str().map(str::to_owned)
            } else {
                None
            }
        }
        UpstreamProtocol::GoogleGenerateContent => {
            let parts = value
                .get("candidates")?
                .as_array()?
                .first()?
                .get("content")?
                .get("parts")?
                .as_array()?;
            let text = parts
                .iter()
                .filter(|part| part.get("thought").and_then(Value::as_bool) != Some(true))
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<String>();
            (!text.is_empty()).then_some(text)
        }
    }
}

pub(crate) fn extract_chat_stream_tool_delta(value: &Value) -> Option<&Value> {
    let delta = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("delta"))?;
    let has_tool_calls = delta
        .get("tool_calls")
        .and_then(Value::as_array)
        .is_some_and(|calls| !calls.is_empty());
    let has_function_call = delta
        .get("function_call")
        .is_some_and(|call| !call.is_null());
    (has_tool_calls || has_function_call).then_some(delta)
}

pub(crate) fn extract_chat_stream_reasoning_delta(value: &Value) -> Option<&str> {
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("delta"))
        .and_then(|delta| delta.get("reasoning_content"))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

pub(crate) fn encode_chat_stream_reasoning_delta(id: &str, model: &str, text: &str) -> String {
    format!(
        "data: {}\n\n",
        json!({
            "id": id,
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"reasoning_content": text},
                "finish_reason": null
            }]
        })
    )
}

pub(crate) fn encode_chat_stream_tool_delta(id: &str, model: &str, delta: &Value) -> String {
    let mut output_delta = serde_json::Map::new();
    if let Some(tool_calls) = delta
        .get("tool_calls")
        .and_then(Value::as_array)
        .filter(|calls| !calls.is_empty())
    {
        output_delta.insert("tool_calls".to_owned(), Value::Array(tool_calls.clone()));
    }
    if let Some(function_call) = delta.get("function_call").filter(|call| !call.is_null()) {
        output_delta.insert("function_call".to_owned(), function_call.clone());
    }
    format!(
        "data: {}\n\n",
        json!({"id":id,"object":"chat.completion.chunk","model":model,"choices":[{"index":0,"delta":Value::Object(output_delta),"finish_reason":null}]})
    )
}

pub(crate) fn encode_responses_tool_event(
    upstream_protocol: UpstreamProtocol,
    client_protocol: Protocol,
    event: &str,
    value: &Value,
) -> Option<String> {
    if upstream_protocol != UpstreamProtocol::Responses || client_protocol != Protocol::Responses {
        return None;
    }
    let event_type = value.get("type").and_then(Value::as_str).unwrap_or(event);
    let is_function_call_event = matches!(
        event_type,
        "response.function_call_arguments.delta" | "response.function_call_arguments.done"
    ) || matches!(
        event_type,
        "response.output_item.added" | "response.output_item.done"
    ) && value
        .get("item")
        .and_then(|item| item.get("type"))
        .and_then(Value::as_str)
        == Some("function_call");
    if !is_function_call_event {
        return None;
    }
    let data = serde_json::to_string(value).ok()?;
    Some(format!("event: {event_type}\ndata: {data}\n\n"))
}

pub(crate) fn extract_responses_tool_calls(
    output: &[Value],
) -> Result<Vec<GoogleStreamToolCall>, String> {
    output
        .iter()
        .enumerate()
        .filter(|(_, item)| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .map(|(index, item)| {
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .ok_or("Responses function call has no name")?;
            let id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("call_{index}"));
            let arguments = match item.get("arguments") {
                None | Some(Value::Null) => json!({}),
                Some(Value::String(arguments)) if arguments.trim().is_empty() => json!({}),
                Some(Value::String(arguments)) => serde_json::from_str(arguments)
                    .map_err(|_| "Responses function call arguments are invalid JSON".to_owned())?,
                Some(arguments) => arguments.clone(),
            };
            Ok(GoogleStreamToolCall {
                id: id.clone(),
                source_id: Some(id),
                name: name.to_owned(),
                arguments,
            })
        })
        .collect()
}

pub(crate) fn extract_stream_finish(
    protocol: UpstreamProtocol,
    event: &str,
    value: &Value,
) -> Option<String> {
    match protocol {
        UpstreamProtocol::ChatCompletions => value
            .get("choices")?
            .as_array()?
            .first()?
            .get("finish_reason")?
            .as_str()
            .map(str::to_owned),
        UpstreamProtocol::Responses if is_responses_terminal_event(event, value) => {
            let response = value.get("response").unwrap_or(value);
            if responses_terminal_state(event, value) == Some(ResponsesTerminal::Incomplete)
                || response
                    .get("incomplete_details")
                    .and_then(|details| details.get("reason"))
                    .and_then(Value::as_str)
                    == Some("max_output_tokens")
            {
                Some("length".to_owned())
            } else if response
                .get("output")
                .and_then(Value::as_array)
                .is_some_and(|items| {
                    items.iter().any(|item| {
                        item.get("type").and_then(Value::as_str) == Some("function_call")
                    })
                })
            {
                Some("tool_calls".to_owned())
            } else {
                Some("stop".to_owned())
            }
        }
        UpstreamProtocol::Messages => value
            .get("delta")?
            .get("stop_reason")?
            .as_str()
            .map(str::to_owned),
        UpstreamProtocol::GoogleGenerateContent => None,
        _ => None,
    }
}

pub(crate) fn extract_stream_usage(
    protocol: UpstreamProtocol,
    event: &str,
    value: &Value,
) -> (Option<u64>, Option<u64>) {
    match protocol {
        UpstreamProtocol::ChatCompletions => {
            let usage = value.get("usage");
            (
                usage
                    .and_then(|u| u.get("prompt_tokens"))
                    .and_then(Value::as_u64),
                usage
                    .and_then(|u| u.get("completion_tokens"))
                    .and_then(Value::as_u64),
            )
        }
        UpstreamProtocol::Responses => {
            let usage = value
                .get("response")
                .and_then(|r| r.get("usage"))
                .or_else(|| value.get("usage"));
            (
                usage
                    .and_then(|u| u.get("input_tokens"))
                    .and_then(Value::as_u64),
                usage
                    .and_then(|u| u.get("output_tokens"))
                    .and_then(Value::as_u64),
            )
        }
        UpstreamProtocol::Messages if event == "message_start" => {
            let usage = value.get("message").and_then(|m| m.get("usage"));
            (
                usage
                    .and_then(|u| u.get("input_tokens"))
                    .and_then(Value::as_u64),
                usage
                    .and_then(|u| u.get("output_tokens"))
                    .and_then(Value::as_u64),
            )
        }
        UpstreamProtocol::Messages => (
            None,
            value
                .get("usage")
                .and_then(|u| u.get("output_tokens"))
                .and_then(Value::as_u64),
        ),
        UpstreamProtocol::GoogleGenerateContent => {
            let usage = value.get("usageMetadata");
            let input = usage
                .and_then(|usage| usage.get("promptTokenCount"))
                .and_then(Value::as_u64);
            let output = usage
                .and_then(|usage| usage.get("candidatesTokenCount"))
                .and_then(Value::as_u64)
                .map(|count| {
                    count.saturating_add(
                        usage
                            .and_then(|usage| usage.get("thoughtsTokenCount"))
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                    )
                })
                .or_else(|| {
                    let total = usage
                        .and_then(|usage| usage.get("totalTokenCount"))
                        .and_then(Value::as_u64)?;
                    Some(total.saturating_sub(input.unwrap_or(0)))
                });
            (input, output)
        }
    }
}

pub(crate) fn extract_stream_cache_usage(
    protocol: UpstreamProtocol,
    event: &str,
    value: &Value,
) -> (Option<u64>, Option<u64>) {
    let (usage, input_key) = match protocol {
        UpstreamProtocol::ChatCompletions => (value.get("usage"), "prompt_tokens"),
        UpstreamProtocol::Responses => (
            value
                .get("response")
                .and_then(|response| response.get("usage"))
                .or_else(|| value.get("usage")),
            "input_tokens",
        ),
        UpstreamProtocol::Messages if event == "message_start" => (
            value
                .get("message")
                .and_then(|message| message.get("usage")),
            "input_tokens",
        ),
        UpstreamProtocol::Messages => (value.get("usage"), "input_tokens"),
        UpstreamProtocol::GoogleGenerateContent => (value.get("usageMetadata"), "promptTokenCount"),
    };
    let cached = usage.and_then(crate::protocol::parse_cached_input_tokens);
    let input_tokens = usage
        .and_then(|usage| usage.get(input_key))
        .and_then(Value::as_u64);
    let cache_input_tokens = match (protocol, usage, input_tokens) {
        (UpstreamProtocol::Messages, Some(usage), Some(input_tokens)) => Some(
            crate::protocol::cache_input_token_total(usage, "input_tokens", input_tokens),
        ),
        (_, _, Some(input_tokens)) => Some(input_tokens),
        _ => None,
    };
    (cached, cache_input_tokens)
}

pub(crate) fn extract_stream_cost(
    protocol: UpstreamProtocol,
    event: &str,
    value: &Value,
) -> Option<i64> {
    let usage = match protocol {
        UpstreamProtocol::Responses => value
            .get("response")
            .and_then(|response| response.get("usage"))
            .or_else(|| value.get("usage")),
        UpstreamProtocol::Messages if event == "message_start" => value
            .get("message")
            .and_then(|message| message.get("usage")),
        UpstreamProtocol::GoogleGenerateContent => None,
        _ => value.get("usage"),
    }?;
    usage
        .get("cost")
        .and_then(crate::protocol::parse_cost_micro_usd)
}
