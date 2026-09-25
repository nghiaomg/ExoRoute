use super::*;
use serde_json::{Value, json};

const MAX_GOOGLE_FUNCTION_NAME_BYTES: usize = 128;

pub(super) fn tool_call_names(messages: &[Message]) -> Result<BTreeMap<String, String>, String> {
    let mut names = BTreeMap::new();
    for message in messages {
        for block in &message.content {
            if let ContentBlock::ToolCall { id, name, .. } = block {
                if !matches!(message.role, Role::Assistant) {
                    return Err(
                        "Google function calls are valid only in assistant messages".to_owned()
                    );
                }
                validate_function_name(name)?;
                if !id.is_empty() && names.insert(id.clone(), name.clone()).is_some() {
                    return Err(format!("Google conversation repeats tool call id '{id}'"));
                }
            }
        }
    }
    Ok(names)
}

pub(super) fn encode_tool_result(content: &[ContentBlock]) -> Result<Value, String> {
    let mut text = String::new();
    for block in content {
        match block {
            ContentBlock::Text { text: value } | ContentBlock::Reasoning { text: value } => {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(value);
            }
            ContentBlock::Thinking { thinking, .. } => {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(thinking);
            }
            ContentBlock::RedactedThinking { .. } => {
                return Err(
                    "Google function responses cannot preserve redacted thinking blocks".to_owned(),
                );
            }
            _ => {
                return Err("Google function responses can preserve text results only".to_owned());
            }
        }
    }
    if let Ok(parsed) = serde_json::from_str::<Value>(&text)
        && parsed.is_object()
    {
        return Ok(parsed);
    }
    Ok(json!({"content":text}))
}

pub(super) fn encode_tool_choice(choice: &Value, tools: &[Tool]) -> Result<Value, String> {
    let (mode, named) = if let Some(value) = choice.as_str() {
        match value {
            "auto" => ("AUTO", None),
            "none" => ("NONE", None),
            "required" | "any" => ("ANY", None),
            _ => return Err("unsupported tool_choice value for Google".to_owned()),
        }
    } else {
        let kind = choice
            .get("type")
            .and_then(Value::as_str)
            .ok_or("tool_choice must be a supported string or object")?;
        match kind {
            "auto" => ("AUTO", None),
            "none" => ("NONE", None),
            "required" | "any" => ("ANY", None),
            "function" | "tool" => {
                let name = choice
                    .get("function")
                    .and_then(|function| function.get("name"))
                    .or_else(|| choice.get("name"))
                    .and_then(Value::as_str)
                    .filter(|name| !name.trim().is_empty())
                    .ok_or("named tool_choice requires a function name")?;
                ("ANY", Some(name))
            }
            _ => return Err("unsupported tool_choice object for Google".to_owned()),
        }
    };

    let mut function_config = json!({"mode":mode});
    if let Some(name) = named {
        if !tools.iter().any(|tool| tool.name == name) {
            return Err(format!(
                "tool_choice references undeclared function '{name}'"
            ));
        }
        function_config["allowedFunctionNames"] = json!([name]);
    }
    Ok(json!({"functionCallingConfig":function_config}))
}

pub(super) fn validate_function_name(name: &str) -> Result<(), String> {
    let valid = !name.is_empty()
        && name.len() <= MAX_GOOGLE_FUNCTION_NAME_BYTES
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(format!(
            "Google function name must use ASCII letters, digits, underscore, dash, colon, or dot and be at most {MAX_GOOGLE_FUNCTION_NAME_BYTES} bytes"
        ))
    }
}

pub(super) fn image_data_uri(value: &str) -> Result<(&str, &str), String> {
    let uri = value.strip_prefix("data:").ok_or(
        "Google image input requires an inline base64 data URI; remote URLs are unsupported",
    )?;
    let (header, data) = uri
        .split_once(',')
        .ok_or("Google image data URI is malformed")?;
    let mime_type = header
        .strip_suffix(";base64")
        .ok_or("Google image data URI must use base64 encoding")?;
    let mime_type = mime_type.to_ascii_lowercase();
    validate_inline_data(&mime_type, data, InlineKind::Image)?;
    // The caller's URL is already resident in memory. Return slices for the
    // data and allocate only the normalized MIME type at the JSON boundary.
    // The MIME values accepted below are ASCII, so the lowercase conversion
    // above cannot change the byte length.
    let original_mime = &header[..mime_type.len()];
    Ok((original_mime, data))
}

#[derive(Clone, Copy)]
pub(super) enum InlineKind {
    Image,
    Pdf,
}

pub(super) fn classify_inline_mime(mime_type: &str) -> Option<InlineKind> {
    if mime_type.eq_ignore_ascii_case("application/pdf") {
        return Some(InlineKind::Pdf);
    }
    if [
        "image/png",
        "image/jpeg",
        "image/webp",
        "image/heic",
        "image/heif",
    ]
    .iter()
    .any(|allowed| mime_type.eq_ignore_ascii_case(allowed))
    {
        return Some(InlineKind::Image);
    }
    None
}

pub(super) fn validate_inline_data(
    mime_type: &str,
    data: &str,
    expected: InlineKind,
) -> Result<(), String> {
    let accepted = match expected {
        InlineKind::Image => matches!(classify_inline_mime(mime_type), Some(InlineKind::Image)),
        InlineKind::Pdf => matches!(classify_inline_mime(mime_type), Some(InlineKind::Pdf)),
    };
    if !accepted {
        return Err(match expected {
            InlineKind::Image => format!("unsupported Google inline image MIME type '{mime_type}'"),
            InlineKind::Pdf => {
                format!("unsupported Google inline document MIME type '{mime_type}'")
            }
        });
    }
    if !valid_base64(data) {
        return Err("Google inline media must contain valid base64 data".to_owned());
    }
    Ok(())
}

/// Performs a bounded, allocation-free syntax check for standard base64.
pub(super) fn valid_base64(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() % 4 == 1 {
        return false;
    }
    let first_padding = bytes.iter().position(|byte| *byte == b'=');
    let data_len = first_padding.unwrap_or(bytes.len());
    if first_padding.is_some() {
        let padding_len = bytes.len() - data_len;
        if padding_len > 2
            || !bytes.len().is_multiple_of(4)
            || bytes[data_len..].iter().any(|b| *b != b'=')
        {
            return false;
        }
        if (padding_len == 1 && data_len % 4 != 3) || (padding_len == 2 && data_len % 4 != 2) {
            return false;
        }
    }
    bytes[..data_len]
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'+' | b'/'))
}

pub(super) fn normalize_finish_reason(
    reason: &str,
    content: &[ContentBlock],
) -> Result<String, String> {
    match reason {
        "STOP" => Ok(
            if content
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolCall { .. }))
            {
                "tool_calls".to_owned()
            } else {
                "stop".to_owned()
            },
        ),
        "MAX_TOKENS" => Ok("length".to_owned()),
        "SAFETY"
        | "RECITATION"
        | "BLOCKLIST"
        | "PROHIBITED_CONTENT"
        | "SPII"
        | "IMAGE_SAFETY"
        | "IMAGE_PROHIBITED_CONTENT"
        | "IMAGE_RECITATION"
        | "MODEL_ARMOR"
        | "LANGUAGE" => Ok("content_filter".to_owned()),
        "MALFORMED_FUNCTION_CALL" | "UNEXPECTED_TOOL_CALL" => {
            Err(format!("Google provider returned finish reason '{reason}'"))
        }
        "OTHER" | "NO_IMAGE" | "IMAGE_OTHER" => Ok("stop".to_owned()),
        _ => Err(format!(
            "Google provider returned unknown finish reason '{reason}'"
        )),
    }
}

pub(super) fn parse_google_usage(value: Option<&Value>) -> Option<Usage> {
    let usage = value?;
    let input_tokens = usage.get("promptTokenCount")?.as_u64()?;
    let candidate_tokens = usage.get("candidatesTokenCount").and_then(Value::as_u64);
    let thought_tokens = usage
        .get("thoughtsTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = candidate_tokens
        .map(|count| count.saturating_add(thought_tokens))
        .or_else(|| {
            usage
                .get("totalTokenCount")
                .and_then(Value::as_u64)
                .map(|total| total.saturating_sub(input_tokens))
        })?;
    Some(Usage {
        input_tokens,
        output_tokens,
        cached_tokens: crate::protocol::parse_cached_input_tokens(usage).unwrap_or(0),
        cost_micro_usd: None,
        cache_input_tokens: input_tokens,
    })
}
