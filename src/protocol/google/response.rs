use super::shared::{
    InlineKind, classify_inline_mime, normalize_finish_reason, parse_google_usage,
    validate_function_name, validate_inline_data,
};
use super::*;

pub(in crate::protocol) fn decode_response(
    value: &Value,
    requested_model: &str,
) -> Result<CanonicalResponse, String> {
    let candidates = value
        .get("candidates")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            if let Some(reason) = value
                .get("promptFeedback")
                .and_then(|feedback| feedback.get("blockReason"))
                .and_then(Value::as_str)
            {
                format!("Google provider blocked the prompt ({reason})")
            } else {
                "Google response has no candidates array".to_owned()
            }
        })?;
    if candidates.is_empty() {
        let reason = value
            .get("promptFeedback")
            .and_then(|feedback| feedback.get("blockReason"))
            .and_then(Value::as_str)
            .unwrap_or("unknown reason");
        return Err(format!("Google provider returned no candidates ({reason})"));
    }
    if candidates.len() != 1 {
        return Err(
            "Google response returned multiple candidates that cannot be represented".to_owned(),
        );
    }
    let candidate = &candidates[0];
    let parts = candidate
        .get("content")
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .ok_or("Google candidate has no content parts")?;

    let mut content = Vec::with_capacity(parts.len());
    for (index, part) in parts.iter().enumerate() {
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            content.push(ContentBlock::Text {
                text: text.to_owned(),
            });
            continue;
        }
        if let Some(function_call) = part.get("functionCall") {
            let name = function_call
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .ok_or("Google function call has no name")?;
            validate_function_name(name)?;
            let arguments = function_call
                .get("args")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if !arguments.is_object() {
                return Err(format!(
                    "arguments for Google function '{name}' are not a JSON object"
                ));
            }
            let id = function_call
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("google_call_{index}"));
            content.push(ContentBlock::ToolCall {
                id,
                name: name.to_owned(),
                arguments,
            });
            continue;
        }
        if let Some(inline_data) = part.get("inlineData") {
            let mime_type = inline_data
                .get("mimeType")
                .and_then(Value::as_str)
                .ok_or("Google inline response data has no MIME type")?;
            let data = inline_data
                .get("data")
                .and_then(Value::as_str)
                .ok_or("Google inline response data has no base64 data")?;
            match classify_inline_mime(mime_type) {
                Some(InlineKind::Image) => {
                    validate_inline_data(mime_type, data, InlineKind::Image)?;
                    content.push(ContentBlock::GeneratedImage {
                        data: data.to_owned(),
                        media_type: mime_type.to_ascii_lowercase(),
                    });
                }
                Some(InlineKind::Pdf) => {
                    validate_inline_data(mime_type, data, InlineKind::Pdf)?;
                    content.push(ContentBlock::Document {
                        source: DocumentSource::Base64 {
                            media_type: "application/pdf".to_owned(),
                            data: data.to_owned(),
                        },
                        filename: None,
                    });
                }
                None => {
                    return Err(format!(
                        "Google returned unsupported inline media type '{mime_type}'"
                    ));
                }
            }
            continue;
        }

        if part.get("fileData").is_some()
            || part.get("functionResponse").is_some()
            || part.get("executableCode").is_some()
            || part.get("codeExecutionResult").is_some()
            || part.get("toolCall").is_some()
            || part.get("toolResponse").is_some()
        {
            return Err("Google response contains an unsupported part type".to_owned());
        }
        return Err("Google response contains an unrecognized content part".to_owned());
    }

    let raw_finish = candidate
        .get("finishReason")
        .and_then(Value::as_str)
        .ok_or("Google candidate has no finishReason")?;
    let finish_reason = normalize_finish_reason(raw_finish, &content)?;
    let usage = parse_google_usage(value.get("usageMetadata"));
    let id = value
        .get("responseId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .unwrap_or("google-response")
        .to_owned();
    let model = value
        .get("modelVersion")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .unwrap_or(requested_model)
        .to_owned();

    Ok(CanonicalResponse {
        id,
        model,
        message: Message {
            role: Role::Assistant,
            content,
            name: None,
        },
        finish_reason,
        usage,
    })
}
