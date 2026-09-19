use super::*;

pub(crate) fn parse_content(value: Option<&Value>) -> Result<Vec<ContentBlock>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    match value {
        Value::String(text) => Ok(vec![ContentBlock::Text { text: text.clone() }]),
        Value::Array(parts) => parts.iter().map(parse_content_part).collect(),
        Value::Object(object) => {
            if object.contains_key("type") {
                Ok(vec![parse_content_part(value)?])
            } else if let Some(text) = object.get("text").and_then(Value::as_str) {
                Ok(vec![ContentBlock::Text {
                    text: text.to_owned(),
                }])
            } else {
                Ok(Vec::new())
            }
        }
        _ => Ok(Vec::new()),
    }
}

pub(crate) fn parse_content_part(part: &Value) -> Result<ContentBlock, String> {
    let kind = part.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "text" | "input_text" | "output_text" => Ok(ContentBlock::Text {
            text: part
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        }),
        "thinking" => {
            let thinking = part
                .get("thinking")
                .and_then(Value::as_str)
                .ok_or("thinking block is missing its thinking text")?
                .to_owned();
            let signature = match part.get("signature") {
                None | Some(Value::Null) => None,
                Some(Value::String(signature)) => Some(signature.clone()),
                Some(_) => return Err("thinking block signature must be a string".to_owned()),
            };
            let extra = part
                .as_object()
                .into_iter()
                .flat_map(|object| object.iter())
                .filter(|(key, _)| !matches!(key.as_str(), "type" | "thinking" | "signature"))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            Ok(ContentBlock::Thinking {
                thinking,
                signature,
                extra,
            })
        }
        "redacted_thinking" => {
            let data = part
                .get("data")
                .and_then(Value::as_str)
                .ok_or("redacted thinking block is missing its data")?
                .to_owned();
            let extra = part
                .as_object()
                .into_iter()
                .flat_map(|object| object.iter())
                .filter(|(key, _)| !matches!(key.as_str(), "type" | "data"))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            Ok(ContentBlock::RedactedThinking { data, extra })
        }
        "image_url" => {
            let image = part
                .get("image_url")
                .ok_or("image_url block is missing image_url")?;
            if part.get("file_id").is_some() || image.get("file_id").is_some() {
                return Err("provider-scoped image file_id references are unsupported; use a URL or data URI".to_owned());
            }
            let url = image
                .get("url")
                .or_else(|| image.as_str().map(|_| image))
                .and_then(Value::as_str)
                .ok_or("image_url must contain a URL or data URI")?;
            Ok(ContentBlock::Image {
                url: validate_image_url(url)?,
                detail: image
                    .get("detail")
                    .or_else(|| part.get("detail"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        }
        "input_image" => {
            if part.get("file_id").is_some() {
                return Err("provider-scoped image file_id references are unsupported; use a URL or data URI".to_owned());
            }
            let image = part
                .get("image_url")
                .ok_or("input_image is missing image_url")?;
            if image.get("file_id").is_some() {
                return Err("provider-scoped image file_id references are unsupported; use a URL or data URI".to_owned());
            }
            let url = image
                .as_str()
                .or_else(|| image.get("url").and_then(Value::as_str))
                .ok_or("input_image image_url must be a URL or data URI")?;
            Ok(ContentBlock::Image {
                url: validate_image_url(url)?,
                detail: part
                    .get("detail")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        }
        "image" => {
            let source = part.get("source").ok_or("image block is missing source")?;
            match source.get("type").and_then(Value::as_str).unwrap_or("") {
                "url" => Ok(ContentBlock::Image {
                    url: validate_image_url(source.get("url").and_then(Value::as_str).ok_or("image URL source is missing url")?)?,
                    detail: None,
                }),
                "base64" => {
                    let media_type = source.get("media_type").and_then(Value::as_str).ok_or("base64 image source is missing media_type")?;
                    let data = source.get("data").and_then(Value::as_str).ok_or("base64 image source is missing data")?;
                    Ok(ContentBlock::Image { url: validate_image_url(&format!("data:{media_type};base64,{data}"))?, detail: None })
                }
                "file" => Err("Anthropic provider-scoped image source.type=file is unsupported; use a URL or base64 source".to_owned()),
                other => Err(format!("unsupported image source type: {other}")),
            }
        }
        "input_file" => parse_responses_file(part),
        "document" => parse_anthropic_document(part),
        "tool_use" | "function_call" => {
            let args = part
                .get("input")
                .or_else(|| part.get("arguments"))
                .cloned()
                .unwrap_or_else(|| json!({}));
            let arguments = match args {
                Value::String(value) => {
                    serde_json::from_str(&value).unwrap_or(Value::String(value))
                }
                other => other,
            };
            Ok(ContentBlock::ToolCall {
                id: part
                    .get("id")
                    .or_else(|| part.get("call_id"))
                    .and_then(Value::as_str)
                    .unwrap_or("call")
                    .to_owned(),
                name: part
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                arguments,
            })
        }
        "tool_result" | "function_call_output" => Ok(ContentBlock::ToolResult {
            tool_call_id: part
                .get("tool_use_id")
                .or_else(|| part.get("call_id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            content: parse_content(part.get("content").or_else(|| part.get("output")))?,
        }),
        _ => Err(format!("unsupported content block type: {kind}")),
    }
}

pub(crate) fn parse_responses_file(part: &Value) -> Result<ContentBlock, String> {
    if part.get("file_id").is_some() {
        return Err("provider-scoped document file_id references are unsupported; use file_url or file_data".to_owned());
    }
    let filename = part
        .get("filename")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if part.get("file_url").is_some() && part.get("file_data").is_some() {
        return Err("input_file must provide exactly one of file_url or file_data".to_owned());
    }
    if let Some(url) = part.get("file_url").and_then(Value::as_str) {
        if url.trim().is_empty() {
            return Err("input_file file_url cannot be empty".to_owned());
        }
        return Ok(ContentBlock::Document {
            source: DocumentSource::Url {
                url: url.to_owned(),
            },
            filename,
        });
    }
    if let Some(data) = part.get("file_data").and_then(Value::as_str) {
        if data.is_empty() {
            return Err("input_file file_data cannot be empty".to_owned());
        }
        let (media_type, encoded_data) = if data.starts_with("data:") {
            let (media_type, encoded) =
                parse_data_uri(data).ok_or("input_file file_data must be a base64 data URI")?;
            (media_type, encoded.to_owned())
        } else {
            let media_type = filename
                .as_deref()
                .and_then(|name| mime_guess::from_path(name).first())
                .map(|mime| mime.to_string())
                .unwrap_or_else(|| "application/pdf".to_owned());
            (media_type, data.to_owned())
        };
        let decoded = decode_base64(&encoded_data)
            .map_err(|_| "input_file file_data is not valid base64".to_owned())?;
        if media_type.starts_with("text/")
            && let Ok(text) = String::from_utf8(decoded)
        {
            return Ok(ContentBlock::Document {
                source: DocumentSource::Text { text },
                filename,
            });
        }
        return Ok(ContentBlock::Document {
            source: DocumentSource::Base64 {
                media_type,
                data: encoded_data,
            },
            filename,
        });
    }
    Err("input_file requires file_url or file_data; file_id references are unsupported".to_owned())
}

pub(crate) fn parse_anthropic_document(part: &Value) -> Result<ContentBlock, String> {
    let source = part
        .get("source")
        .ok_or("document block is missing source")?;
    let filename = part.get("title").and_then(Value::as_str).map(str::to_owned);
    match source.get("type").and_then(Value::as_str).unwrap_or("") {
        "url" => {
            let url = source.get("url").and_then(Value::as_str).filter(|url| !url.trim().is_empty())
                .ok_or("document URL source is missing url")?;
            Ok(ContentBlock::Document { source: DocumentSource::Url { url: url.to_owned() }, filename })
        }
        "base64" => {
            let media_type = source.get("media_type").and_then(Value::as_str)
                .ok_or("base64 document source is missing media_type")?;
            let data = source.get("data").and_then(Value::as_str)
                .ok_or("base64 document source is missing data")?;
            if data.is_empty() || decode_base64(data).is_err() {
                return Err("base64 document source contains invalid base64 data".to_owned());
            }
            Ok(ContentBlock::Document { source: DocumentSource::Base64 { media_type: media_type.to_owned(), data: data.to_owned() }, filename })
        }
        "text" => {
            let media_type = source.get("media_type").and_then(Value::as_str)
                .ok_or("text document source is missing media_type")?;
            if media_type != "text/plain" {
                return Err("inline text document source must use media_type text/plain".to_owned());
            }
            let text = source.get("data").and_then(Value::as_str)
                .ok_or("text document source is missing data")?;
            Ok(ContentBlock::Document { source: DocumentSource::Text { text: text.to_owned() }, filename })
        }
        "file" => Err("Anthropic provider-scoped document source.type=file references are unsupported; use URL, base64, or text".to_owned()),
        other => Err(format!("unsupported document source type: {other}")),
    }
}

pub(crate) fn parse_data_uri(value: &str) -> Option<(String, &str)> {
    let (header, data) = value.strip_prefix("data:")?.split_once(',')?;
    let media_type = header.strip_suffix(";base64")?;
    if media_type.is_empty() {
        return None;
    }
    Some((media_type.to_owned(), data))
}

pub(crate) fn validate_image_url(value: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        return Err("image URL cannot be empty".to_owned());
    }
    if value.starts_with("data:") {
        let (_, data) = parse_data_uri(value).ok_or("image data URI must use base64 encoding")?;
        if data.is_empty() || decode_base64(data).is_err() {
            return Err("image data URI contains invalid base64 data".to_owned());
        }
    }
    Ok(value.to_owned())
}

pub(crate) fn decode_base64(value: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(value))
}

pub(crate) fn parse_role(value: &str) -> Result<Role, String> {
    match value {
        "system" => Ok(Role::System),
        "developer" => Ok(Role::Developer),
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        "tool" => Ok(Role::Tool),
        _ => Err(format!("unsupported message role: {value}")),
    }
}
pub(crate) fn string_field(value: &Value, key: &str) -> Result<String, String> {
    let result = value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("{key} is required"))?;
    if result.len() > 256 {
        return Err(format!("{key} exceeds the 256-byte limit"));
    }
    Ok(result)
}

pub(crate) fn optional_f32(
    value: Option<&Value>,
    name: &str,
    minimum: f32,
    maximum: f32,
) -> Result<Option<f32>, String> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let number = value
        .as_f64()
        .ok_or_else(|| format!("{name} must be a number"))? as f32;
    if !number.is_finite() || number < minimum || number > maximum {
        return Err(format!("{name} must be between {minimum} and {maximum}"));
    }
    Ok(Some(number))
}

pub(crate) fn optional_u32(value: Option<&Value>, name: &str) -> Result<Option<u32>, String> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let number = value
        .as_u64()
        .ok_or_else(|| format!("{name} must be a non-negative integer"))?;
    u32::try_from(number)
        .map(Some)
        .map_err(|_| format!("{name} exceeds the supported maximum"))
}

pub(crate) fn optional_bool(value: Option<&Value>, name: &str) -> Result<Option<bool>, String> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    value
        .as_bool()
        .map(Some)
        .ok_or_else(|| format!("{name} must be a boolean"))
}

pub(crate) const MAX_TOOLS_PER_REQUEST: usize = 128;

pub(crate) fn tool_list(value: Option<&Value>) -> Result<Vec<&Value>, String> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(tools)) if tools.len() <= MAX_TOOLS_PER_REQUEST => {
            Ok(tools.iter().collect())
        }
        Some(Value::Array(_)) => Err(format!(
            "a request may define at most {MAX_TOOLS_PER_REQUEST} tools"
        )),
        Some(_) => Err("tools must be an array".to_owned()),
    }
}

pub(crate) fn parse_chat_tools(value: Option<&Value>) -> Result<Vec<Tool>, String> {
    tool_list(value)?
        .into_iter()
        .map(|tool| {
            let function = tool.get("function").unwrap_or(tool);
            let name = string_field(function, "name")?;
            let description = function
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let parameters = function
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| json!({"type":"object"}));
            if !parameters.is_object() {
                return Err(format!("tool '{name}' parameters must be a JSON object"));
            }
            Ok(Tool {
                name,
                description,
                parameters,
            })
        })
        .collect()
}

pub(crate) fn parse_response_tools(value: Option<&Value>) -> Result<Vec<Tool>, String> {
    tool_list(value)?
        .into_iter()
        .map(|tool| {
            if tool.get("type").and_then(Value::as_str) != Some("function") {
                return Err("only Responses function tools can be translated".to_owned());
            }
            let name = string_field(tool, "name")?;
            let parameters = tool
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| json!({"type":"object"}));
            if !parameters.is_object() {
                return Err(format!("tool '{name}' parameters must be a JSON object"));
            }
            Ok(Tool {
                name,
                description: tool
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                parameters,
            })
        })
        .collect()
}

pub(crate) fn parse_messages_tools(value: Option<&Value>) -> Result<Vec<Tool>, String> {
    tool_list(value)?
        .into_iter()
        .map(|tool| {
            let name = string_field(tool, "name")?;
            let parameters = tool
                .get("input_schema")
                .cloned()
                .unwrap_or_else(|| json!({"type":"object"}));
            if !parameters.is_object() {
                return Err(format!("tool '{name}' input_schema must be a JSON object"));
            }
            Ok(Tool {
                name,
                description: tool
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                parameters,
            })
        })
        .collect()
}

pub(crate) enum CanonicalToolChoice {
    Auto,
    None,
    Required,
    Named(String),
}

pub(crate) fn parse_tool_choice(value: &Value) -> Result<CanonicalToolChoice, String> {
    if let Some(value) = value.as_str() {
        return match value {
            "auto" => Ok(CanonicalToolChoice::Auto),
            "none" => Ok(CanonicalToolChoice::None),
            "required" | "any" => Ok(CanonicalToolChoice::Required),
            _ => Err("unsupported tool_choice value".to_owned()),
        };
    }
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or("tool_choice must be a supported string or object")?;
    match kind {
        "auto" => Ok(CanonicalToolChoice::Auto),
        "none" => Ok(CanonicalToolChoice::None),
        "required" | "any" => Ok(CanonicalToolChoice::Required),
        "function" => {
            let name = value
                .get("function")
                .and_then(|function| function.get("name"))
                .or_else(|| value.get("name"))
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty() && name.len() <= 256)
                .ok_or("function tool_choice requires a name of at most 256 bytes")?;
            Ok(CanonicalToolChoice::Named(name.to_owned()))
        }
        "tool" => {
            let name = value
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty() && name.len() <= 256)
                .ok_or("tool tool_choice requires a name of at most 256 bytes")?;
            Ok(CanonicalToolChoice::Named(name.to_owned()))
        }
        _ => Err("unsupported tool_choice object".to_owned()),
    }
}

pub(crate) fn parse_string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}
pub(crate) fn parse_cached_input_tokens(value: &Value) -> Option<u64> {
    value
        .get("prompt_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .or_else(|| {
            value
                .get("input_tokens_details")
                .and_then(|details| details.get("cached_tokens"))
        })
        .or_else(|| value.get("cache_read_input_tokens"))
        .or_else(|| value.get("cachedContentTokenCount"))
        .or_else(|| value.get("cached_tokens"))
        .and_then(Value::as_u64)
}

pub(crate) fn cache_input_token_total(value: &Value, input_key: &str, input_tokens: u64) -> u64 {
    if input_key != "input_tokens" {
        return input_tokens;
    }
    input_tokens
        .saturating_add(
            value
                .get("cache_read_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        )
        .saturating_add(
            value
                .get("cache_creation_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        )
}

pub(crate) fn parse_usage(
    value: Option<&Value>,
    input_key: &str,
    output_key: &str,
) -> Option<Usage> {
    let v = value?;
    let input_tokens = v.get(input_key)?.as_u64()?;
    Some(Usage {
        input_tokens,
        output_tokens: v.get(output_key)?.as_u64()?,
        cached_tokens: parse_cached_input_tokens(v).unwrap_or(0),
        cost_micro_usd: v.get("cost").and_then(parse_cost_micro_usd),
        cache_input_tokens: cache_input_token_total(v, input_key, input_tokens),
    })
}

pub fn parse_cost_micro_usd(value: &Value) -> Option<i64> {
    let amount = value.as_f64()?;
    if !amount.is_finite() || amount < 0.0 {
        return None;
    }
    let micros = (amount * 1_000_000.0).round();
    if !micros.is_finite() || micros > i64::MAX as f64 {
        return None;
    }
    Some(micros as i64)
}
