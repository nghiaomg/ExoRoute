//! Workspace chat input validation: bounded extraction of the message text,
//! system prompt, thinking preference, and inline attachments the dashboard
//! sends with one chat turn.
//!
//! Everything here is read-only validation. Sizes and media types are checked
//! before any allocation-heavy work so a malformed request cannot buffer
//! unbounded data.

use super::*;
use crate::protocol::parse_thinking_handling;

const MAX_CHAT_MESSAGE_CHARS: usize = 128 * 1024;
const MAX_CHAT_SYSTEM_PROMPT_CHARS: usize = 32 * 1024;
const MAX_CHAT_ATTACHMENTS: usize = 8;
const MAX_CHAT_ATTACHMENT_BASE64_CHARS: usize = 6_000_000;
const MAX_CHAT_ATTACHMENT_TOTAL_DECODED_BYTES: usize = 8 * 1024 * 1024;
const MAX_ATTACHMENT_MEDIA_TYPE_BYTES: usize = 128;
const MAX_ATTACHMENT_FILENAME_BYTES: usize = 256;

/// One image or document attachment the dashboard uploaded with a message.
pub(super) struct ChatAttachment {
    pub(super) media_type: String,
    pub(super) base64_data: String,
    pub(super) filename: Option<String>,
    pub(super) is_image: bool,
}

pub(super) struct ChatTextInput {
    pub(super) message: String,
    pub(super) system_prompt: Option<String>,
    pub(super) thinking_mode: ChatThinkingMode,
    pub(super) thinking_override: Option<String>,
}

/// Per-request thinking preference for one workspace message.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ChatThinkingMode {
    Default,
    Preserve,
    Remove,
    Override,
}

pub(super) const MAX_PROVIDER_ID_BYTES: usize = 256;
pub(super) const MAX_MODEL_BYTES: usize = 256;

pub(super) fn fail_request(
    status: StatusCode,
    message: impl Into<String>,
) -> (StatusCode, Json<Value>) {
    fail(status, message)
}

/// Extracts the bounded provider/model identifiers.
pub(super) fn parse_chat_target_ids(
    input: &Value,
) -> Result<(String, String), (StatusCode, Json<Value>)> {
    let provider_id = input
        .get("provider_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= MAX_PROVIDER_ID_BYTES)
        .ok_or_else(|| fail_request(StatusCode::BAD_REQUEST, "provider_id is required"))?
        .to_owned();
    let model = input
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= MAX_MODEL_BYTES)
        .ok_or_else(|| fail_request(StatusCode::BAD_REQUEST, "model is required"))?
        .to_owned();
    Ok((provider_id, model))
}

/// Validates and extracts the bounded chat input fields.
pub(super) fn parse_chat_text_input(
    input: &Value,
) -> Result<ChatTextInput, (StatusCode, Json<Value>)> {
    let message = input
        .get("message")
        .and_then(Value::as_str)
        .ok_or_else(|| fail_request(StatusCode::BAD_REQUEST, "message must be a string"))?;
    if message.chars().count() > MAX_CHAT_MESSAGE_CHARS {
        return Err(fail_request(
            StatusCode::BAD_REQUEST,
            format!("message exceeds the {MAX_CHAT_MESSAGE_CHARS} character limit"),
        ));
    }
    if message.trim().is_empty() {
        return Err(fail_request(
            StatusCode::BAD_REQUEST,
            "message must not be empty",
        ));
    }
    let system_prompt = match input.get("system_prompt") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => {
            if text.chars().count() > MAX_CHAT_SYSTEM_PROMPT_CHARS {
                return Err(fail_request(
                    StatusCode::BAD_REQUEST,
                    format!(
                        "system prompt exceeds the {MAX_CHAT_SYSTEM_PROMPT_CHARS} character limit"
                    ),
                ));
            }
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        Some(_) => {
            return Err(fail_request(
                StatusCode::BAD_REQUEST,
                "system prompt must be a string",
            ));
        }
    };
    let thinking_mode = match input.get("thinking_mode") {
        None | Some(Value::Null) => ChatThinkingMode::Default,
        Some(Value::String(mode)) => match mode.as_str() {
            "default" => ChatThinkingMode::Default,
            "preserve" => ChatThinkingMode::Preserve,
            "remove" => ChatThinkingMode::Remove,
            "override" => ChatThinkingMode::Override,
            _ => {
                return Err(fail_request(
                    StatusCode::BAD_REQUEST,
                    "thinking mode must be default, preserve, remove or override",
                ));
            }
        },
        Some(_) => {
            return Err(fail_request(
                StatusCode::BAD_REQUEST,
                "thinking mode must be a string",
            ));
        }
    };
    let thinking_override = match input.get("thinking_override") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(_) => {
            return Err(fail_request(
                StatusCode::BAD_REQUEST,
                "thinking override must be a string",
            ));
        }
    };
    if thinking_mode == ChatThinkingMode::Override {
        // Reuse the protocol's canonical validation: bounded size and
        // non-empty replacement text.
        let validated = thinking_override.as_deref().ok_or_else(|| {
            fail_request(
                StatusCode::BAD_REQUEST,
                "thinking override text is required in override mode",
            )
        })?;
        parse_thinking_handling("override", Some(validated))
            .map_err(|message| fail_request(StatusCode::BAD_REQUEST, message))?;
    }
    Ok(ChatTextInput {
        message: message.to_owned(),
        system_prompt,
        thinking_mode,
        thinking_override,
    })
}

/// Tolerant base64 decoder matching the protocol codec: padded or unpadded
/// standard alphabet, so a browser `FileReader` data URL decodes unchanged.
fn decode_attachment_base64(value: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(value))
}

/// Decodes the bounded attachment list. Every attachment must carry an inline
/// base64 payload; URL references are rejected because the relay must not
/// fetch remote content the operator did not upload.
pub(super) fn parse_chat_attachments(
    input: &Value,
) -> Result<Vec<ChatAttachment>, (StatusCode, Json<Value>)> {
    let Some(entries) = input.get("attachments").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    if entries.len() > MAX_CHAT_ATTACHMENTS {
        return Err(fail_request(
            StatusCode::BAD_REQUEST,
            format!("a chat message can include at most {MAX_CHAT_ATTACHMENTS} attachments"),
        ));
    }
    let mut attachments = Vec::with_capacity(entries.len());
    let mut total_decoded_bytes = 0usize;
    for entry in entries {
        let is_image = match entry.get("kind").and_then(Value::as_str) {
            Some("image") => true,
            Some("document") => false,
            _ => {
                return Err(fail_request(
                    StatusCode::BAD_REQUEST,
                    "attachment kind must be image or document",
                ));
            }
        };
        let media_type = entry
            .get("media_type")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.len() <= MAX_ATTACHMENT_MEDIA_TYPE_BYTES)
            .ok_or_else(|| {
                fail_request(
                    StatusCode::BAD_REQUEST,
                    "attachment media_type is required and must be short",
                )
            })?;
        if is_image && !media_type.starts_with("image/") {
            return Err(fail_request(
                StatusCode::BAD_REQUEST,
                "image attachments must use an image/ media type",
            ));
        }
        if !is_image && media_type.starts_with("image/") {
            return Err(fail_request(
                StatusCode::BAD_REQUEST,
                "document attachments must not use an image/ media type",
            ));
        }
        let base64_data = entry
            .get("data")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| fail_request(StatusCode::BAD_REQUEST, "attachment data is required"))?;
        if base64_data.len() > MAX_CHAT_ATTACHMENT_BASE64_CHARS {
            return Err(fail_request(
                StatusCode::BAD_REQUEST,
                "attachment data exceeds the size limit",
            ));
        }
        let decoded = decode_attachment_base64(base64_data).map_err(|_| {
            fail_request(
                StatusCode::BAD_REQUEST,
                "attachment data must be valid base64",
            )
        })?;
        total_decoded_bytes = total_decoded_bytes.saturating_add(decoded.len());
        if total_decoded_bytes > MAX_CHAT_ATTACHMENT_TOTAL_DECODED_BYTES {
            return Err(fail_request(
                StatusCode::BAD_REQUEST,
                "attachments exceed the total size limit",
            ));
        }
        let filename = match entry.get("filename") {
            None | Some(Value::Null) => None,
            Some(Value::String(name)) => {
                let name = name.trim();
                if name.len() > MAX_ATTACHMENT_FILENAME_BYTES {
                    return Err(fail_request(
                        StatusCode::BAD_REQUEST,
                        "attachment filename is too long",
                    ));
                }
                (!name.is_empty()).then(|| name.to_owned())
            }
            Some(_) => {
                return Err(fail_request(
                    StatusCode::BAD_REQUEST,
                    "attachment filename must be a string",
                ));
            }
        };
        attachments.push(ChatAttachment {
            media_type: media_type.to_owned(),
            base64_data: base64_data.to_owned(),
            filename,
            is_image,
        });
    }
    Ok(attachments)
}
