use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MessagesStreamBlockKind {
    Text,
    Thinking,
    RedactedThinking,
    ToolUse,
}

#[derive(Debug)]
pub(crate) struct MessagesStreamBlock {
    pub(crate) upstream_index: usize,
    pub(crate) output_index: usize,
    pub(crate) kind: MessagesStreamBlockKind,
    pub(crate) id: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) text: String,
    pub(crate) arguments: String,
}

pub(crate) fn messages_stream_index(value: &Value) -> Result<usize, String> {
    value
        .get("index")
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok())
        .ok_or_else(|| "Anthropic stream content block has no valid index".to_owned())
}

pub(crate) fn messages_stream_block_position(
    blocks: &[MessagesStreamBlock],
    upstream_index: usize,
) -> Option<usize> {
    blocks
        .iter()
        .position(|block| block.upstream_index == upstream_index)
}

pub(crate) fn messages_stream_ensure_block(
    blocks: &mut Vec<MessagesStreamBlock>,
    upstream_index: usize,
    kind: MessagesStreamBlockKind,
    next_output_index: &mut usize,
    client_protocol: Protocol,
    open_message_blocks: &mut Vec<usize>,
) -> Result<usize, String> {
    if let Some(position) = messages_stream_block_position(blocks, upstream_index) {
        return Ok(position);
    }
    if blocks.len() >= MAX_MESSAGES_STREAM_BLOCKS {
        return Err("Anthropic stream exceeded the content block limit".to_owned());
    }
    let output_index = *next_output_index;
    *next_output_index = (*next_output_index).saturating_add(1);
    blocks.push(MessagesStreamBlock {
        upstream_index,
        output_index,
        kind,
        id: None,
        name: None,
        text: String::new(),
        arguments: String::new(),
    });
    if client_protocol == Protocol::Messages {
        open_message_blocks.push(output_index);
    }
    Ok(blocks.len().saturating_sub(1))
}

pub(crate) fn encode_messages_stream_block_start(
    client_protocol: Protocol,
    value: &Value,
    blocks: &mut Vec<MessagesStreamBlock>,
    next_output_index: &mut usize,
    open_message_blocks: &mut Vec<usize>,
    response_id: &str,
    model: &str,
) -> Result<(Vec<Bytes>, bool), String> {
    let upstream_index = messages_stream_index(value)?;
    if messages_stream_block_position(blocks, upstream_index).is_some() {
        return Ok((Vec::new(), false));
    }
    let content_block = value.get("content_block").ok_or_else(|| {
        "Anthropic stream content block start is missing content_block".to_owned()
    })?;
    let block_type = content_block
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "Anthropic stream content block has no type".to_owned())?;
    let kind = match block_type {
        "text" => MessagesStreamBlockKind::Text,
        "thinking" => MessagesStreamBlockKind::Thinking,
        "redacted_thinking" => MessagesStreamBlockKind::RedactedThinking,
        "tool_use" | "server_tool_use" => MessagesStreamBlockKind::ToolUse,
        _ => return Ok((Vec::new(), false)),
    };
    let position = messages_stream_ensure_block(
        blocks,
        upstream_index,
        kind,
        next_output_index,
        client_protocol,
        open_message_blocks,
    )?;
    let block = &mut blocks[position];
    if kind == MessagesStreamBlockKind::ToolUse {
        let id = content_block
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| "Anthropic tool_use block has no id".to_owned())?;
        let name = content_block
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| "Anthropic tool_use block has no name".to_owned())?;
        if id.len() > 256 || name.len() > 128 {
            return Err("Anthropic tool_use block metadata is too large".to_owned());
        }
        block.id = Some(id.to_owned());
        block.name = Some(name.to_owned());
    }

    let mut events = Vec::with_capacity(2);
    match (client_protocol, kind) {
        (Protocol::Messages, MessagesStreamBlockKind::Text) => events.push(Bytes::from(format!(
            "event: content_block_start\ndata: {}\n\n",
            json!({"type":"content_block_start","index":block.output_index,"content_block":{"type":"text","text":""}})
        ))),
        (Protocol::Messages, MessagesStreamBlockKind::Thinking) => events.push(Bytes::from(
            format!(
                "event: content_block_start\ndata: {}\n\n",
                json!({"type":"content_block_start","index":block.output_index,"content_block":{"type":"thinking","thinking":""}})
            ),
        )),
        (Protocol::Messages, MessagesStreamBlockKind::RedactedThinking) => events.push(Bytes::from(
            format!(
                "event: content_block_start\ndata: {}\n\n",
                json!({"type":"content_block_start","index":block.output_index,"content_block":{"type":"redacted_thinking","data":content_block.get("data").cloned().unwrap_or(Value::String(String::new()))}})
            ),
        )),
        (Protocol::Messages, MessagesStreamBlockKind::ToolUse) => events.push(Bytes::from(
            format!(
                "event: content_block_start\ndata: {}\n\n",
                json!({"type":"content_block_start","index":block.output_index,"content_block":{"type":"tool_use","id":block.id.as_deref().unwrap_or(""),"name":block.name.as_deref().unwrap_or(""),"input":{}}})
            ),
        )),
        (Protocol::ChatCompletions, MessagesStreamBlockKind::ToolUse) => events.push(Bytes::from(
            format!(
                "data: {}\n\n",
                json!({"id":response_id,"object":"chat.completion.chunk","model":model,"choices":[{"index":0,"delta":{"tool_calls":[{"index":block.output_index,"id":block.id.as_deref().unwrap_or(""),"type":"function","function":{"name":block.name.as_deref().unwrap_or(""),"arguments":""}}]},"finish_reason":null}]})
            ),
        )),
        (Protocol::Responses, MessagesStreamBlockKind::Thinking) => events.push(Bytes::from(
            format!(
                "event: response.output_item.added\ndata: {}\n\n",
                json!({"type":"response.output_item.added","output_index":block.output_index,"item":{"id":format!("rs_{}",block.output_index),"type":"reasoning","status":"in_progress","summary":[]}})
            ),
        )),
        (Protocol::Responses, MessagesStreamBlockKind::ToolUse) => events.push(Bytes::from(
            format!(
                "event: response.output_item.added\ndata: {}\n\n",
                json!({"type":"response.output_item.added","output_index":block.output_index,"item":{"id":format!("fc_{}",block.output_index),"type":"function_call","status":"in_progress","call_id":block.id.as_deref().unwrap_or(""),"name":block.name.as_deref().unwrap_or(""),"arguments":""}})
            ),
        )),
        _ => {}
    }
    Ok((
        events,
        matches!(
            kind,
            MessagesStreamBlockKind::Thinking
                | MessagesStreamBlockKind::RedactedThinking
                | MessagesStreamBlockKind::ToolUse
        ),
    ))
}

pub(crate) fn append_messages_stream_text(
    block: &mut MessagesStreamBlock,
    text: &str,
    limit: usize,
) -> Result<(), String> {
    let size = block.text.len().checked_add(text.len()).ok_or_else(|| {
        "Anthropic streamed text exceeded the configured response limit".to_owned()
    })?;
    if limit != 0 && size > limit {
        return Err("Anthropic streamed text exceeded the configured response limit".to_owned());
    }
    block.text.push_str(text);
    Ok(())
}

pub(crate) fn append_messages_stream_arguments(
    block: &mut MessagesStreamBlock,
    arguments: &str,
    limit: usize,
) -> Result<(), String> {
    let size = block
        .arguments
        .len()
        .checked_add(arguments.len())
        .ok_or_else(|| {
            "Anthropic streamed tool arguments exceeded the configured response limit".to_owned()
        })?;
    if limit != 0 && size > limit {
        return Err(
            "Anthropic streamed tool arguments exceeded the configured response limit".to_owned(),
        );
    }
    block.arguments.push_str(arguments);
    Ok(())
}

pub(crate) struct MessagesStreamDeltaContext<'a> {
    pub(crate) client_protocol: Protocol,
    pub(crate) blocks: &'a mut Vec<MessagesStreamBlock>,
    pub(crate) next_output_index: &'a mut usize,
    pub(crate) open_message_blocks: &'a mut Vec<usize>,
    pub(crate) response_id: &'a str,
    pub(crate) model: &'a str,
    pub(crate) response_limit: usize,
}

pub(crate) fn encode_messages_stream_delta(
    value: &Value,
    context: &mut MessagesStreamDeltaContext<'_>,
) -> Result<(Vec<Bytes>, bool), String> {
    let upstream_index = messages_stream_index(value)?;
    let delta = value
        .get("delta")
        .ok_or_else(|| "Anthropic stream content block delta is missing delta".to_owned())?;
    let delta_type = delta
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "Anthropic stream content block delta has no type".to_owned())?;
    let kind = match delta_type {
        "text_delta" => MessagesStreamBlockKind::Text,
        "thinking_delta" | "signature_delta" => MessagesStreamBlockKind::Thinking,
        "input_json_delta" => MessagesStreamBlockKind::ToolUse,
        _ => return Ok((Vec::new(), false)),
    };
    let position = messages_stream_ensure_block(
        context.blocks,
        upstream_index,
        kind,
        context.next_output_index,
        context.client_protocol,
        context.open_message_blocks,
    )?;
    let block = &mut context.blocks[position];
    let mut events = Vec::with_capacity(1);
    let meaningful = match delta_type {
        "text_delta" => {
            let text = delta
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| "Anthropic text delta has no text".to_owned())?;
            append_messages_stream_text(block, text, context.response_limit)?;
            if context.client_protocol == Protocol::Messages {
                events.push(Bytes::from(format!(
                    "event: content_block_delta\ndata: {}\n\n",
                    json!({"type":"content_block_delta","index":block.output_index,"delta":{"type":"text_delta","text":text}})
                )));
            } else if !text.is_empty() {
                events.push(Bytes::from(encode_stream_text(
                    context.client_protocol,
                    context.response_id,
                    context.model,
                    text,
                )));
            }
            !text.trim().is_empty()
        }
        "thinking_delta" => {
            let text = delta
                .get("thinking")
                .and_then(Value::as_str)
                .ok_or_else(|| "Anthropic thinking delta has no thinking text".to_owned())?;
            append_messages_stream_text(block, text, context.response_limit)?;
            match context.client_protocol {
                Protocol::Messages => events.push(Bytes::from(format!(
                    "event: content_block_delta\ndata: {}\n\n",
                    json!({"type":"content_block_delta","index":block.output_index,"delta":{"type":"thinking_delta","thinking":text}})
                ))),
                Protocol::ChatCompletions => events.push(Bytes::from(
                    encode_chat_stream_reasoning_delta(context.response_id, context.model, text),
                )),
                Protocol::Responses => events.push(Bytes::from(format!(
                    "event: response.reasoning_summary_text.delta\ndata: {}\n\n",
                    json!({"type":"response.reasoning_summary_text.delta","item_id":format!("rs_{}",block.output_index),"output_index":block.output_index,"summary_index":0,"delta":text})
                ))),
            }
            !text.trim().is_empty()
        }
        "signature_delta" => {
            let signature = delta
                .get("signature")
                .and_then(Value::as_str)
                .ok_or_else(|| "Anthropic signature delta has no signature".to_owned())?;
            if context.client_protocol == Protocol::Messages {
                events.push(Bytes::from(format!(
                    "event: content_block_delta\ndata: {}\n\n",
                    json!({"type":"content_block_delta","index":block.output_index,"delta":{"type":"signature_delta","signature":signature}})
                )));
            }
            !signature.is_empty()
        }
        "input_json_delta" => {
            let arguments = delta
                .get("partial_json")
                .and_then(Value::as_str)
                .ok_or_else(|| "Anthropic input JSON delta has no partial_json".to_owned())?;
            if block.kind != MessagesStreamBlockKind::ToolUse {
                return Err("Anthropic input JSON delta references a non-tool block".to_owned());
            }
            append_messages_stream_arguments(block, arguments, context.response_limit)?;
            match context.client_protocol {
                Protocol::Messages => events.push(Bytes::from(format!(
                    "event: content_block_delta\ndata: {}\n\n",
                    json!({"type":"content_block_delta","index":block.output_index,"delta":{"type":"input_json_delta","partial_json":arguments}})
                ))),
                Protocol::ChatCompletions => events.push(Bytes::from(format!(
                    "data: {}\n\n",
                        json!({"id":context.response_id,"object":"chat.completion.chunk","model":context.model,"choices":[{"index":0,"delta":{"tool_calls":[{"index":block.output_index,"function":{"arguments":arguments}}]},"finish_reason":null}]})
                ))),
                Protocol::Responses => events.push(Bytes::from(format!(
                    "event: response.function_call_arguments.delta\ndata: {}\n\n",
                    json!({"type":"response.function_call_arguments.delta","item_id":format!("fc_{}",block.output_index),"output_index":block.output_index,"delta":arguments})
                ))),
            }
            !arguments.is_empty()
        }
        _ => false,
    };
    Ok((events, meaningful))
}

pub(crate) fn encode_messages_stream_block_stop(
    client_protocol: Protocol,
    value: &Value,
    blocks: &mut [MessagesStreamBlock],
    open_message_blocks: &mut Vec<usize>,
    response_id: &str,
    model: &str,
) -> Vec<Bytes> {
    let Ok(upstream_index) = messages_stream_index(value) else {
        return Vec::new();
    };
    let Some(block) = blocks
        .iter()
        .find(|block| block.upstream_index == upstream_index)
    else {
        return Vec::new();
    };
    let output_index = block.output_index;
    let kind = block.kind;
    let arguments = block.arguments.clone();
    let id = block.id.clone().unwrap_or_default();
    let name = block.name.clone().unwrap_or_default();
    open_message_blocks.retain(|index| *index != output_index);
    match client_protocol {
        Protocol::Messages => vec![Bytes::from(format!(
            "event: content_block_stop\ndata: {}\n\n",
            json!({"type":"content_block_stop","index":output_index})
        ))],
        Protocol::Responses if kind == MessagesStreamBlockKind::ToolUse => {
            vec![Bytes::from(format!(
                "event: response.function_call_arguments.done\ndata: {}\n\nevent: response.output_item.done\ndata: {}\n\n",
                json!({"type":"response.function_call_arguments.done","item_id":format!("fc_{}",output_index),"output_index":output_index,"arguments":arguments}),
                json!({"type":"response.output_item.done","output_index":output_index,"item":{"id":format!("fc_{}",output_index),"type":"function_call","status":"completed","call_id":id,"name":name,"arguments":arguments}})
            ))]
        }
        Protocol::Responses if kind == MessagesStreamBlockKind::Thinking => {
            vec![Bytes::from(format!(
                "event: response.output_item.done\ndata: {}\n\n",
                json!({"type":"response.output_item.done","output_index":output_index,"item":{"id":format!("rs_{}",output_index),"type":"reasoning","status":"completed","summary":[{"type":"summary_text","text":block.text}]}})
            ))]
        }
        _ => {
            let _ = (response_id, model, id, name);
            Vec::new()
        }
    }
}

pub(crate) fn messages_response_output(blocks: &[MessagesStreamBlock]) -> Vec<Value> {
    blocks
        .iter()
        .filter_map(|block| match block.kind {
            MessagesStreamBlockKind::Text if !block.text.is_empty() => Some(json!({
                "id": format!("msg_{}", block.output_index),
                "type": "message",
                "role": "assistant",
                "content": [{"type":"output_text","text":block.text,"annotations":[]}]
            })),
            MessagesStreamBlockKind::Thinking => Some(json!({
                "id": format!("rs_{}", block.output_index),
                "type": "reasoning",
                "status": "completed",
                "summary": if block.text.is_empty() { Vec::<Value>::new() } else { vec![json!({"type":"summary_text","text":block.text})] }
            })),
            MessagesStreamBlockKind::ToolUse => Some(json!({
                "id": format!("fc_{}", block.output_index),
                "type": "function_call",
                "status": "completed",
                "call_id": block.id.as_deref().unwrap_or(""),
                "name": block.name.as_deref().unwrap_or(""),
                "arguments": block.arguments
            })),
            MessagesStreamBlockKind::RedactedThinking => None,
            _ => None,
        })
        .collect()
}
