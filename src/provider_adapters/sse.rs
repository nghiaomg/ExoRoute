use super::{UpstreamProtocol, adapter};
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub(crate) struct AdapterSseError {
    pub message: String,
    pub safe_to_fail_over: bool,
}

impl AdapterSseError {
    pub fn new(message: impl Into<String>, safe_to_fail_over: bool) -> Self {
        Self {
            message: message.into(),
            safe_to_fail_over,
        }
    }
}

pub(crate) async fn read_adapter_event_stream(
    adapter_id: &str,
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Value, AdapterSseError> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err(AdapterSseError::new(
            "provider adapter is not registered",
            false,
        ));
    };
    adapter.read_event_stream(response, max_bytes).await
}
pub(crate) async fn read_codex_event_stream(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Value, AdapterSseError> {
    let bytes = read_limited_response(response, max_bytes)
        .await
        .map_err(|error| AdapterSseError::new(error, false))?;
    parse_adapter_event_stream(&bytes)
}

pub(crate) fn adapter_sse_error_can_fail_over(
    stream_requested: bool,
    error: &AdapterSseError,
) -> bool {
    !stream_requested && error.safe_to_fail_over
}

pub(crate) fn is_event_stream_content_type(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .split(';')
        .next()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"))
}

pub(crate) fn accepts_event_stream_response(
    response: &reqwest::Response,
    allows_missing_content_type: bool,
) -> bool {
    is_event_stream_content_type(response)
        || (allows_missing_content_type
            && response.headers().get(http::header::CONTENT_TYPE).is_none())
}

pub(crate) async fn read_limited_response(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Bytes, String> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(format!("response exceeds the {max_bytes}-byte limit"));
    }
    let mut body = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|_| "upstream response body could not be read".to_owned())?;
        if body.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!("response exceeds the {max_bytes}-byte limit"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(body))
}

#[derive(Default)]
pub(crate) struct ResponsesStreamAccumulator {
    output_items: BTreeMap<usize, Value>,
    text_deltas: BTreeMap<(usize, usize), String>,
    text_done: BTreeMap<(usize, usize), String>,
    function_call_arguments: BTreeMap<usize, String>,
    function_call_arguments_done: BTreeMap<usize, String>,
    item_ids: BTreeMap<usize, String>,
}

impl ResponsesStreamAccumulator {
    pub(crate) fn observe(
        &mut self,
        event_type: &str,
        event: &Value,
        max_bytes: usize,
    ) -> Result<(), String> {
        match event_type {
            "response.output_item.added" | "response.output_item.done" => {
                let Some(output_index) = event_index(event, "output_index") else {
                    return Ok(());
                };
                let Some(item) = event.get("item").filter(|item| item.is_object()) else {
                    return Ok(());
                };
                if max_bytes != 0
                    && serde_json::to_vec(item)
                        .map(|encoded| encoded.len() > max_bytes)
                        .unwrap_or(true)
                {
                    return Err(
                        "Responses output item exceeded the configured response limit".to_owned(),
                    );
                }
                if let Some(id) = item.get("id").and_then(Value::as_str) {
                    self.item_ids.insert(output_index, id.to_owned());
                }
                self.output_items.insert(output_index, item.clone());
            }
            "response.output_text.delta" => {
                let Some(delta) = event.get("delta").and_then(Value::as_str) else {
                    return Ok(());
                };
                let output_index = event_index(event, "output_index").unwrap_or(0);
                let content_index = event_index(event, "content_index").unwrap_or(0);
                if let Some(item_id) = event.get("item_id").and_then(Value::as_str) {
                    self.item_ids
                        .entry(output_index)
                        .or_insert_with(|| item_id.to_owned());
                }
                append_bounded(
                    self.text_deltas
                        .entry((output_index, content_index))
                        .or_default(),
                    delta,
                    max_bytes,
                    "Responses streamed text",
                )?;
            }
            "response.output_text.done" => {
                let Some(text) = event.get("text").and_then(Value::as_str) else {
                    return Ok(());
                };
                let output_index = event_index(event, "output_index").unwrap_or(0);
                let content_index = event_index(event, "content_index").unwrap_or(0);
                if max_bytes != 0 && text.len() > max_bytes {
                    return Err(
                        "Responses completed text exceeded the configured response limit"
                            .to_owned(),
                    );
                }
                self.text_done
                    .insert((output_index, content_index), text.to_owned());
            }
            "response.content_part.done" => {
                let text = event
                    .get("part")
                    .and_then(|part| part.get("text"))
                    .and_then(Value::as_str)
                    .or_else(|| event.get("text").and_then(Value::as_str));
                let Some(text) = text else {
                    return Ok(());
                };
                let output_index = event_index(event, "output_index").unwrap_or(0);
                let content_index = event_index(event, "content_index").unwrap_or(0);
                if max_bytes != 0 && text.len() > max_bytes {
                    return Err(
                        "Responses completed content exceeded the configured response limit"
                            .to_owned(),
                    );
                }
                self.text_done
                    .insert((output_index, content_index), text.to_owned());
            }
            "response.function_call_arguments.delta" => {
                let Some(delta) = event.get("delta").and_then(Value::as_str) else {
                    return Ok(());
                };
                let Some(output_index) = self.event_output_index(event) else {
                    return Ok(());
                };
                append_bounded(
                    self.function_call_arguments
                        .entry(output_index)
                        .or_default(),
                    delta,
                    max_bytes,
                    "Responses function-call arguments",
                )?;
            }
            "response.function_call_arguments.done" => {
                let Some(arguments) = event.get("arguments").and_then(Value::as_str) else {
                    return Ok(());
                };
                let Some(output_index) = self.event_output_index(event) else {
                    return Ok(());
                };
                if max_bytes != 0 && arguments.len() > max_bytes {
                    return Err(
                        "Responses completed function-call arguments exceeded the configured response limit"
                            .to_owned(),
                    );
                }
                self.function_call_arguments_done
                    .insert(output_index, arguments.to_owned());
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn reconstruct_response(&self, response: &Value) -> Option<Value> {
        let mut output_items = BTreeMap::<usize, Value>::new();
        if let Some(items) = response.get("output").and_then(Value::as_array) {
            for (index, item) in items.iter().enumerate() {
                output_items.insert(index, item.clone());
            }
        }
        for (index, item) in &self.output_items {
            if let Some(existing) = output_items.get_mut(index) {
                merge_response_output_item(existing, item);
            } else {
                output_items.insert(*index, item.clone());
            }
        }

        let mut text_by_position = self.text_deltas.clone();
        for (position, text) in &self.text_done {
            if !text.trim().is_empty() {
                text_by_position.insert(*position, text.clone());
            }
        }
        if let Some(text) = response.get("output_text").and_then(Value::as_str)
            && !text.trim().is_empty()
        {
            text_by_position
                .entry((0, 0))
                .or_insert_with(|| text.to_owned());
        }

        for ((output_index, _content_index), text) in text_by_position {
            if text.trim().is_empty() {
                continue;
            }
            let item_id = self
                .item_ids
                .get(&output_index)
                .map(String::as_str)
                .unwrap_or("msg_0");
            let item = output_items.entry(output_index).or_insert_with(|| {
                serde_json::json!({
                    "id": item_id,
                    "type": "message",
                    "role": "assistant",
                    "content": []
                })
            });
            merge_codex_message_text(item, &text);
        }

        let mut function_call_arguments = self.function_call_arguments.clone();
        for (output_index, arguments) in &self.function_call_arguments_done {
            if !arguments.is_empty() {
                function_call_arguments.insert(*output_index, arguments.clone());
            }
        }
        for (output_index, arguments) in function_call_arguments {
            if arguments.is_empty() {
                continue;
            }
            if let Some(item) = output_items.get_mut(&output_index)
                && item.get("type").and_then(Value::as_str) == Some("function_call")
            {
                item["arguments"] = Value::String(arguments);
            }
        }

        if output_items.is_empty() {
            return None;
        }
        let mut response = response.clone();
        response["output"] = Value::Array(output_items.into_values().collect());
        Some(response)
    }

    fn event_output_index(&self, event: &Value) -> Option<usize> {
        event_index(event, "output_index").or_else(|| {
            let item_id = event.get("item_id").and_then(Value::as_str)?;
            self.item_ids
                .iter()
                .find_map(|(index, id)| (id == item_id).then_some(*index))
        })
    }
}

fn append_bounded(
    target: &mut String,
    value: &str,
    max_bytes: usize,
    label: &str,
) -> Result<(), String> {
    let next_len = target
        .len()
        .checked_add(value.len())
        .ok_or_else(|| format!("{label} exceeded the configured response limit"))?;
    if max_bytes != 0 && next_len > max_bytes {
        return Err(format!("{label} exceeded the configured response limit"));
    }
    target.push_str(value);
    Ok(())
}

fn merge_response_output_item(target: &mut Value, source: &Value) {
    let (Some(target), Some(source)) = (target.as_object_mut(), source.as_object()) else {
        return;
    };
    for (key, value) in source {
        let should_replace = target.get(key).is_none_or(|existing| {
            existing.is_null() || existing.as_str().is_some_and(str::is_empty)
        });
        if should_replace {
            target.insert(key.clone(), value.clone());
        }
    }
}

fn event_index(event: &Value, field: &str) -> Option<usize> {
    event
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn merge_codex_message_text(item: &mut Value, text: &str) {
    if item.get("type").and_then(Value::as_str) != Some("message") {
        return;
    }
    let Some(object) = item.as_object_mut() else {
        return;
    };
    let content = object
        .entry("content".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(parts) = content.as_array_mut() else {
        return;
    };
    if let Some(part) = parts
        .iter_mut()
        .find(|part| part.get("type").and_then(Value::as_str) == Some("output_text"))
    {
        if part
            .get("text")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            part["text"] = Value::String(text.to_owned());
        }
    } else {
        parts.push(serde_json::json!({
            "type": "output_text",
            "text": text,
            "annotations": []
        }));
    }
}

fn decode_codex_completed_response(
    response: Value,
    accumulator: &ResponsesStreamAccumulator,
) -> Result<Value, AdapterSseError> {
    if let Some(reconstructed) = accumulator.reconstruct_response(&response)
        && crate::protocol::decode_upstream_response(
            UpstreamProtocol::Responses,
            &reconstructed,
            "codex",
        )
        .is_ok()
    {
        return Ok(reconstructed);
    }
    let decode_error = match crate::protocol::decode_upstream_response(
        UpstreamProtocol::Responses,
        &response,
        "codex",
    ) {
        Ok(_) => return Ok(response),
        Err(error) => error,
    };
    Err(AdapterSseError::new(
        format!("OpenAI Codex completed response is invalid: {decode_error}"),
        true,
    ))
}

pub(crate) fn parse_adapter_event_stream(bytes: &[u8]) -> Result<Value, AdapterSseError> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        AdapterSseError::new("OpenAI Codex returned an invalid event stream", false)
    })?;
    let mut data_lines = Vec::new();
    let mut event_name = String::new();
    let mut accumulator = ResponsesStreamAccumulator::default();
    for line in text.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if !data_lines.is_empty() {
                let data = data_lines.join("\n");
                data_lines.clear();
                if data == "[DONE]" {
                    continue;
                }
                let event: Value = serde_json::from_str(&data).map_err(|_| {
                    AdapterSseError::new("OpenAI Codex returned an invalid event", false)
                })?;
                let event_type = event
                    .get("type")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .unwrap_or(&event_name);
                if event_type == "response.failed" || event_type == "error" {
                    return Err(AdapterSseError::new(
                        "OpenAI Codex reported a failed response",
                        true,
                    ));
                }
                accumulator
                    .observe(event_type, &event, bytes.len().max(1))
                    .map_err(|error| AdapterSseError::new(error, true))?;
                if event_type == "response.completed" || event_type == "response.done" {
                    let response = event.get("response").cloned().unwrap_or(event);
                    return decode_codex_completed_response(response, &accumulator);
                }
                if event.get("output").is_some() && event.get("status").is_some() {
                    return decode_codex_completed_response(event, &accumulator);
                }
            }
            event_name.clear();
        } else if let Some(value) = line.strip_prefix("event:") {
            event_name = value.trim().to_owned();
        } else if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.trim_start().to_owned());
        }
    }
    Err(AdapterSseError::new(
        "OpenAI Codex event stream ended without a completed response",
        false,
    ))
}
