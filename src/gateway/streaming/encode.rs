use super::*;

pub(crate) fn encode_stream_start(protocol: Protocol, id: &str, model: &str) -> String {
    match protocol {
        Protocol::ChatCompletions => format!(
            "data: {}\n\n",
            json!({"id":id,"object":"chat.completion.chunk","model":model,"choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]})
        ),
        Protocol::Responses => format!(
            "event: response.created\ndata: {}\n\n",
            json!({"type":"response.created","response":{"id":id,"object":"response","status":"in_progress","model":model,"output":[]}})
        ),
        Protocol::Messages => String::new(),
    }
}

pub(crate) fn encode_stream_text(protocol: Protocol, id: &str, model: &str, text: &str) -> String {
    match protocol {
        Protocol::ChatCompletions => format!(
            "data: {}\n\n",
            json!({"id":id,"object":"chat.completion.chunk","model":model,"choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]})
        ),
        Protocol::Responses => format!(
            "event: response.output_text.delta\ndata: {}\n\n",
            json!({"type":"response.output_text.delta","item_id":"msg_0","output_index":0,"content_index":0,"delta":text})
        ),
        Protocol::Messages => format!(
            "event: content_block_delta\ndata: {}\n\n",
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}})
        ),
    }
}

pub(crate) fn encode_stream_finish(
    protocol: Protocol,
    id: &str,
    model: &str,
    finish_reason: &str,
    usage: Option<(u64, u64, u64, u64)>,
    cost_micro_usd: Option<i64>,
    response_output: Option<&[Value]>,
) -> String {
    match protocol {
        Protocol::ChatCompletions => format!(
            "data: {}\n\n",
            json!({"id":id,"object":"chat.completion.chunk","model":model,"choices":[{"index":0,"delta":{},"finish_reason":chat_stream_finish_reason(finish_reason)}]})
        ),
        Protocol::Responses => format!(
            "event: response.completed\ndata: {}\n\n",
            json!({"type":"response.completed","response":{"id":id,"object":"response","status":if finish_reason=="length" {"incomplete"} else {"completed"},"model":model,"finish_reason":finish_reason,"output":response_output.unwrap_or(&[]),"usage":usage.map(|(input,output,cached,cache_input)|{
                // The cached subset sits inside the input total, mirroring the
                // upstream Responses API usage shape.
                let input_tokens = inclusive_input_tokens(input, cache_input);
                json!({"input_tokens":input_tokens,"output_tokens":output,"total_tokens":input_tokens.saturating_add(output),"input_tokens_details":{"cached_tokens":cached},"cost":cost_micro_usd.map(|value| value as f64 / 1_000_000.0)})
            }).unwrap_or(Value::Null)}})
        ),
        Protocol::Messages => {
            let (input, output, cached, cache_input) = usage.unwrap_or((0, 0, 0, 0));
            messages_stream_finish(
                finish_reason,
                input,
                output,
                cached,
                cache_input,
                cost_micro_usd,
                &[],
            )
        }
    }
}

pub(crate) fn chat_stream_finish_reason(reason: &str) -> &'static str {
    if reason.contains("tool") || reason.contains("function_call") {
        "tool_calls"
    } else if reason.contains("length") || reason.contains("max_tokens") {
        "length"
    } else if reason.contains("content_filter") {
        "content_filter"
    } else {
        "stop"
    }
}

pub(crate) fn encode_stream_usage(
    input_tokens: u64,
    output_tokens: u64,
    cached_tokens: u64,
    cache_input_tokens: u64,
    cost_micro_usd: Option<i64>,
) -> String {
    // The final Chat Completions chunk carries the same usage shape as the
    // non-streaming response: the cached subset sits inside prompt_tokens.
    let prompt_tokens = inclusive_input_tokens(input_tokens, cache_input_tokens);
    format!(
        "data: {}\n\n",
        json!({"choices":[],"usage":{"prompt_tokens":prompt_tokens,"completion_tokens":output_tokens,"total_tokens":prompt_tokens.saturating_add(output_tokens),"prompt_tokens_details":{"cached_tokens":cached_tokens},"cost":cost_micro_usd.map(|value| value as f64 / 1_000_000.0)}})
    )
}

pub(crate) fn messages_stream_start(
    id: &str,
    model: &str,
    input_tokens: u64,
    cached_tokens: u64,
    cache_input_tokens: u64,
) -> String {
    format!(
        "event: message_start\ndata: {}\n\n",
        json!({"type":"message_start","message":{"id":id,"type":"message","role":"assistant","model":model,"content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":uncached_input_tokens(input_tokens, cache_input_tokens, cached_tokens),"output_tokens":0,"cache_read_input_tokens":cached_tokens}}})
    )
}

pub(crate) fn messages_text_start(index: usize) -> String {
    format!(
        "event: content_block_start\ndata: {}\n\n",
        json!({"type":"content_block_start","index":index,"content_block":{"type":"text","text":""}})
    )
}

pub(crate) fn messages_stream_finish(
    reason: &str,
    input_tokens: u64,
    output_tokens: u64,
    cached_tokens: u64,
    cache_input_tokens: u64,
    cost_micro_usd: Option<i64>,
    open_blocks: &[usize],
) -> String {
    let stop_reason = if reason.contains("tool") {
        "tool_use"
    } else if reason.contains("length") || reason.contains("max_tokens") {
        "max_tokens"
    } else if reason.contains("stop_sequence") {
        "stop_sequence"
    } else {
        "end_turn"
    };
    let mut output = String::new();
    for index in open_blocks {
        output.push_str(&format!(
            "event: content_block_stop\ndata: {}\n\n",
            json!({"type":"content_block_stop","index":index})
        ));
    }
    output.push_str(&format!(
        "event: message_delta\ndata: {}\n\nevent: message_stop\ndata: {}\n\n",
        json!({"type":"message_delta","delta":{"stop_reason":stop_reason,"stop_sequence":null},"usage":{"input_tokens":uncached_input_tokens(input_tokens, cache_input_tokens, cached_tokens),"output_tokens":output_tokens,"cache_read_input_tokens":cached_tokens,"cost":cost_micro_usd.map(|value| value as f64 / 1_000_000.0)}}),
        json!({"type":"message_stop"})
    ));
    output
}
pub(crate) fn encode_stream_error(_protocol: Protocol, message: &str) -> String {
    format!(
        "event: error\ndata: {}\n\n",
        json!({"type":"error","error":{"type":"upstream_error","message":message}})
    )
}
