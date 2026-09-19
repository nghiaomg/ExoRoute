use super::*;

use serde_json::{Value, json};
use std::collections::BTreeMap;

const MAX_GOOGLE_FUNCTION_NAME_BYTES: usize = 128;

/// Encodes ExoRoute's protocol-neutral request for Google's GenerateContent API.
///
/// Streaming is selected by the caller's endpoint (`streamGenerateContent`); the
/// `stream` flag is therefore intentionally not copied into the JSON body.
pub(super) fn encode_request(request: &CanonicalRequest, model: &str) -> Result<Value, String> {
    if !request.metadata.is_empty() {
        return Err(
            "Google Generate Content cannot preserve protocol-specific request metadata".to_owned(),
        );
    }
    if model.trim().is_empty() {
        return Err("Google model is required".to_owned());
    }
    if request.messages.is_empty() {
        return Err("at least one message is required".to_owned());
    }

    let call_names = tool_call_names(&request.messages)?;
    let mut system_parts = Vec::new();
    let mut contents = Vec::with_capacity(request.messages.len());

    for message in &request.messages {
        if message.name.is_some() {
            return Err("Google Generate Content cannot represent message names".to_owned());
        }

        if matches!(message.role, Role::System | Role::Developer) {
            for block in &message.content {
                match block {
                    ContentBlock::Text { text } | ContentBlock::Reasoning { text } => {
                        system_parts.push(json!({"text": text}));
                    }
                    ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => {
                        system_parts.push(super::internal_thinking_block(block));
                    }
                    ContentBlock::ToolCall { .. } | ContentBlock::ToolResult { .. } => {
                        return Err(
                            "Google system instructions cannot contain tool calls or results"
                                .to_owned(),
                        );
                    }
                    ContentBlock::Image { .. }
                    | ContentBlock::Document { .. }
                    | ContentBlock::GeneratedImage { .. } => {
                        return Err(
                            "Google system instructions support text only; move media to a user message"
                                .to_owned(),
                        );
                    }
                }
            }
            continue;
        }

        let is_tool_message = matches!(message.role, Role::Tool);
        let mut parts = Vec::new();
        for block in &message.content {
            match block {
                ContentBlock::Text { text } | ContentBlock::Reasoning { text } => {
                    if is_tool_message {
                        return Err(
                            "Google tool messages must contain tool results only".to_owned()
                        );
                    }
                    parts.push(json!({"text": text}));
                }
                ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => {
                    if is_tool_message {
                        return Err(
                            "Google tool messages must contain tool results only".to_owned()
                        );
                    }
                    parts.push(super::internal_thinking_block(block));
                }
                ContentBlock::Image { url, .. } => {
                    if is_tool_message {
                        return Err("Google tool messages cannot contain media".to_owned());
                    }
                    let (mime_type, data) = image_data_uri(url)?;
                    parts.push(json!({"inlineData":{"mimeType":mime_type,"data":data}}));
                }
                ContentBlock::GeneratedImage { data, media_type } => {
                    if is_tool_message {
                        return Err("Google tool messages cannot contain media".to_owned());
                    }
                    validate_inline_data(media_type, data, InlineKind::Image)?;
                    parts.push(json!({"inlineData":{"mimeType":media_type,"data":data}}));
                }
                ContentBlock::Document { source, filename } => {
                    if is_tool_message {
                        return Err("Google tool messages cannot contain documents".to_owned());
                    }
                    match source {
                        DocumentSource::Url { .. } => {
                            return Err(
                                "Google Generate Content cannot forward remote document URLs"
                                    .to_owned(),
                            );
                        }
                        DocumentSource::Text { text } => {
                            if filename.is_some() {
                                return Err(
                                    "Google Generate Content cannot preserve a filename for text documents"
                                        .to_owned(),
                                );
                            }
                            parts.push(json!({"text":text}));
                        }
                        DocumentSource::Base64 { media_type, data } => {
                            if !media_type.eq_ignore_ascii_case("application/pdf") {
                                return Err("Google inline document input supports PDF files only"
                                    .to_owned());
                            }
                            validate_inline_data(media_type, data, InlineKind::Pdf)?;
                            parts.push(json!({
                                "inlineData":{"mimeType":"application/pdf","data":data}
                            }));
                        }
                    }
                }
                ContentBlock::ToolCall {
                    id,
                    name,
                    arguments,
                } => {
                    if !matches!(message.role, Role::Assistant) {
                        return Err(
                            "Google function calls are valid only in assistant messages".to_owned()
                        );
                    }
                    validate_function_name(name)?;
                    if !arguments.is_object() {
                        return Err(format!(
                            "arguments for Google function '{name}' must be a JSON object"
                        ));
                    }
                    let mut call = json!({"name":name,"args":arguments});
                    if !id.is_empty() {
                        call["id"] = json!(id);
                    }
                    parts.push(json!({"functionCall":call}));
                }
                ContentBlock::ToolResult {
                    tool_call_id,
                    content,
                } => {
                    if !is_tool_message {
                        return Err(
                            "Google function responses must be in tool-role messages".to_owned()
                        );
                    }
                    let name = call_names.get(tool_call_id).ok_or_else(|| {
                        format!("Google tool result references unknown call '{tool_call_id}'")
                    })?;
                    let response = encode_tool_result(content)?;
                    let mut function_response = json!({"name":name,"response":response});
                    if !tool_call_id.is_empty() {
                        function_response["id"] = json!(tool_call_id);
                    }
                    parts.push(json!({"functionResponse":function_response}));
                }
            }
        }

        if !parts.is_empty() {
            let role = if is_tool_message || matches!(message.role, Role::User) {
                "user"
            } else {
                "model"
            };
            contents.push(json!({"role":role,"parts":parts}));
        }
    }

    if contents.is_empty() {
        return Err("Google Generate Content requires at least one non-system message".to_owned());
    }

    let mut body = json!({"contents":contents});
    if !system_parts.is_empty() {
        body["systemInstruction"] = json!({"parts":system_parts});
    }

    if !request.tools.is_empty() {
        let mut declarations = Vec::with_capacity(request.tools.len());
        for tool in &request.tools {
            validate_function_name(&tool.name)?;
            if !tool.parameters.is_object() {
                return Err(format!(
                    "parameters for Google function '{}' must be a JSON object",
                    tool.name
                ));
            }
            let mut declaration = json!({"name":tool.name,"parameters":tool.parameters});
            if let Some(description) = &tool.description {
                declaration["description"] = json!(description);
            }
            declarations.push(declaration);
        }
        body["tools"] = json!([{"functionDeclarations":declarations}]);
    }

    if let Some(choice) = &request.tool_choice {
        if request.tools.is_empty() {
            return Err("Google tool_choice requires at least one declared tool".to_owned());
        }
        body["toolConfig"] = encode_tool_choice(choice, &request.tools)?;
    }

    let mut generation_config = serde_json::Map::new();
    if let Some(value) = request.temperature {
        if !value.is_finite() || !(0.0..=2.0).contains(&value) {
            return Err("temperature must be between 0 and 2".to_owned());
        }
        generation_config.insert("temperature".to_owned(), json!(value));
    }
    if let Some(value) = request.top_p {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err("top_p must be between 0 and 1".to_owned());
        }
        generation_config.insert("topP".to_owned(), json!(value));
    }
    if let Some(value) = request.max_tokens {
        if value == 0 {
            return Err("max_tokens must be greater than zero".to_owned());
        }
        generation_config.insert("maxOutputTokens".to_owned(), json!(value));
    }
    if !request.stop.is_empty() {
        generation_config.insert("stopSequences".to_owned(), json!(request.stop));
    }
    if !generation_config.is_empty() {
        body["generationConfig"] = Value::Object(generation_config);
    }

    Ok(body)
}

/// Decodes the terminal JSON response from Google's GenerateContent API.
pub(super) fn decode_response(
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

fn tool_call_names(messages: &[Message]) -> Result<BTreeMap<String, String>, String> {
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

fn encode_tool_result(content: &[ContentBlock]) -> Result<Value, String> {
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

fn encode_tool_choice(choice: &Value, tools: &[Tool]) -> Result<Value, String> {
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

fn validate_function_name(name: &str) -> Result<(), String> {
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

fn image_data_uri(value: &str) -> Result<(&str, &str), String> {
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
enum InlineKind {
    Image,
    Pdf,
}

fn classify_inline_mime(mime_type: &str) -> Option<InlineKind> {
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

fn validate_inline_data(mime_type: &str, data: &str, expected: InlineKind) -> Result<(), String> {
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
fn valid_base64(value: &str) -> bool {
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

fn normalize_finish_reason(reason: &str, content: &[ContentBlock]) -> Result<String, String> {
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

fn parse_google_usage(value: Option<&Value>) -> Option<Usage> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn request(messages: Vec<Message>) -> CanonicalRequest {
        CanonicalRequest {
            source_protocol: Protocol::ChatCompletions,
            model: "opencode-model".to_owned(),
            messages,
            tools: Vec::new(),
            tool_choice: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stop: Vec::new(),
            stream: false,
            metadata: BTreeMap::new(),
            output_styles_applied: false,
        }
    }

    fn message(role: Role, content: Vec<ContentBlock>) -> Message {
        Message {
            role,
            content,
            name: None,
        }
    }

    #[test]
    fn encodes_roles_generation_options_and_tool_declarations() {
        let mut request = request(vec![
            message(
                Role::System,
                vec![ContentBlock::Text {
                    text: "Be concise".to_owned(),
                }],
            ),
            message(
                Role::User,
                vec![ContentBlock::Text {
                    text: "Add these".to_owned(),
                }],
            ),
            message(
                Role::Assistant,
                vec![ContentBlock::ToolCall {
                    id: "call-1".to_owned(),
                    name: "sum".to_owned(),
                    arguments: json!({"a":2,"b":3}),
                }],
            ),
            message(
                Role::Tool,
                vec![ContentBlock::ToolResult {
                    tool_call_id: "call-1".to_owned(),
                    content: vec![ContentBlock::Text {
                        text: "5".to_owned(),
                    }],
                }],
            ),
        ]);
        request.tools.push(Tool {
            name: "sum".to_owned(),
            description: Some("Add two numbers".to_owned()),
            parameters: json!({"type":"object","properties":{"a":{"type":"number"}}}),
        });
        request.tool_choice = Some(json!({"type":"function","function":{"name":"sum"}}));
        request.temperature = Some(0.3);
        request.top_p = Some(0.8);
        request.max_tokens = Some(128);
        request.stop = vec!["END".to_owned()];
        request.stream = true;

        let encoded = encode_request(&request, "google-model").expect("request should encode");
        assert_eq!(
            encoded["systemInstruction"]["parts"][0]["text"],
            "Be concise"
        );
        assert_eq!(encoded["contents"][0]["role"], "user");
        assert_eq!(
            encoded["contents"][1]["parts"][0]["functionCall"]["id"],
            "call-1"
        );
        assert_eq!(encoded["contents"][2]["role"], "user");
        assert_eq!(
            encoded["contents"][2]["parts"][0]["functionResponse"]["name"],
            "sum"
        );
        assert_eq!(
            encoded["tools"][0]["functionDeclarations"][0]["name"],
            "sum"
        );
        assert_eq!(
            encoded["toolConfig"]["functionCallingConfig"]["mode"],
            "ANY"
        );
        assert_eq!(
            encoded["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"][0],
            "sum"
        );
        assert!(
            encoded["generationConfig"]["temperature"]
                .as_f64()
                .is_some_and(|value| (value - 0.3).abs() < 1e-6)
        );
        assert!(
            encoded["generationConfig"]["topP"]
                .as_f64()
                .is_some_and(|value| (value - 0.8).abs() < 1e-6)
        );
        assert_eq!(encoded["generationConfig"]["maxOutputTokens"], 128);
        assert_eq!(encoded["generationConfig"]["stopSequences"][0], "END");
        assert!(encoded.get("stream").is_none());
    }

    #[test]
    fn encodes_inline_image_and_pdf_but_not_remote_media() {
        let media_request = request(vec![message(
            Role::User,
            vec![
                ContentBlock::Image {
                    url: "data:image/png;base64,aGVsbG8=".to_owned(),
                    detail: Some("high".to_owned()),
                },
                ContentBlock::Document {
                    source: DocumentSource::Base64 {
                        media_type: "application/pdf".to_owned(),
                        data: "JVBERi0=".to_owned(),
                    },
                    filename: Some("document.pdf".to_owned()),
                },
            ],
        )]);
        let encoded = encode_request(&media_request, "google-model").expect("media should encode");
        assert_eq!(
            encoded["contents"][0]["parts"][0]["inlineData"]["mimeType"],
            "image/png"
        );
        assert_eq!(
            encoded["contents"][0]["parts"][1]["inlineData"]["mimeType"],
            "application/pdf"
        );

        let remote_image = request(vec![message(
            Role::User,
            vec![ContentBlock::Image {
                url: "https://example.com/image.png".to_owned(),
                detail: None,
            }],
        )]);
        assert!(encode_request(&remote_image, "m").is_err());

        let remote_document = request(vec![message(
            Role::User,
            vec![ContentBlock::Document {
                source: DocumentSource::Url {
                    url: "https://example.com/file.pdf".to_owned(),
                },
                filename: None,
            }],
        )]);
        assert!(encode_request(&remote_document, "m").is_err());
    }

    #[test]
    fn decodes_text_tools_finish_reason_and_usage() {
        let response = json!({
            "responseId":"resp-1",
            "modelVersion":"gemini-test",
            "candidates":[{
                "content":{"parts":[
                    {"text":"hello"},
                    {"functionCall":{"id":"call-9","name":"lookup","args":{"q":"x"}}}
                ]},
                "finishReason":"STOP"
            }],
            "usageMetadata":{"promptTokenCount":7,"cachedContentTokenCount":3,"candidatesTokenCount":4,"thoughtsTokenCount":2,"totalTokenCount":13}
        });
        let decoded = decode_response(&response, "requested").expect("response should decode");
        assert_eq!(decoded.id, "resp-1");
        assert_eq!(decoded.model, "gemini-test");
        assert_eq!(decoded.finish_reason, "tool_calls");
        assert_eq!(
            decoded.usage.as_ref().map(|usage| usage.input_tokens),
            Some(7)
        );
        assert_eq!(
            decoded.usage.as_ref().map(|usage| usage.output_tokens),
            Some(6)
        );
        assert_eq!(
            decoded.usage.as_ref().map(|usage| usage.cached_tokens),
            Some(3)
        );
        assert!(matches!(
            decoded.message.content.get(1),
            Some(ContentBlock::ToolCall { id, name, arguments })
                if id == "call-9" && name == "lookup" && arguments["q"] == "x"
        ));
    }

    #[test]
    fn decodes_image_document_and_finish_reasons() {
        let response = json!({
            "candidates":[{
                "content":{"parts":[
                    {"inlineData":{"mimeType":"image/png","data":"aGVsbG8="}},
                    {"inlineData":{"mimeType":"application/pdf","data":"JVBERi0="}}
                ]},
                "finishReason":"MAX_TOKENS"
            }],
            "usageMetadata":{"promptTokenCount":3,"totalTokenCount":9}
        });
        let decoded = decode_response(&response, "requested").expect("response should decode");
        assert_eq!(decoded.finish_reason, "length");
        assert_eq!(
            decoded.usage.as_ref().map(|usage| usage.output_tokens),
            Some(6)
        );
        assert!(matches!(
            &decoded.message.content[0],
            ContentBlock::GeneratedImage { .. }
        ));
        assert!(matches!(
            &decoded.message.content[1],
            ContentBlock::Document { .. }
        ));

        let blocked = json!({"candidates":[{"content":{"parts":[]},"finishReason":"SAFETY"}]});
        assert_eq!(
            decode_response(&blocked, "m")
                .expect("safety response")
                .finish_reason,
            "content_filter"
        );
    }

    #[test]
    fn rejects_unrepresentable_metadata_media_and_tool_results() {
        let mut with_metadata = request(vec![message(
            Role::User,
            vec![ContentBlock::Text {
                text: "hi".to_owned(),
            }],
        )]);
        with_metadata
            .metadata
            .insert("response_format".to_owned(), json!({"type":"json_object"}));
        assert!(encode_request(&with_metadata, "m").is_err());

        let bad_pdf = request(vec![message(
            Role::User,
            vec![ContentBlock::Document {
                source: DocumentSource::Base64 {
                    media_type: "application/msword".to_owned(),
                    data: "YQ==".to_owned(),
                },
                filename: None,
            }],
        )]);
        assert!(encode_request(&bad_pdf, "m").is_err());

        let invalid_base64 = request(vec![message(
            Role::User,
            vec![ContentBlock::Image {
                url: "data:image/png;base64,aGVs!G8=".to_owned(),
                detail: None,
            }],
        )]);
        assert!(encode_request(&invalid_base64, "m").is_err());

        let unknown_tool_result = request(vec![message(
            Role::Tool,
            vec![ContentBlock::ToolResult {
                tool_call_id: "missing".to_owned(),
                content: vec![ContentBlock::Text {
                    text: "done".to_owned(),
                }],
            }],
        )]);
        assert!(encode_request(&unknown_tool_result, "m").is_err());
    }

    #[test]
    fn rejects_unsupported_response_parts_and_multiple_candidates() {
        let remote = json!({"candidates":[{"content":{"parts":[{"fileData":{"fileUri":"https://example.com/a.png","mimeType":"image/png"}}]},"finishReason":"STOP"}]});
        assert!(decode_response(&remote, "m").is_err());

        let multiple = json!({"candidates":[
            {"content":{"parts":[{"text":"one"}]},"finishReason":"STOP"},
            {"content":{"parts":[{"text":"two"}]},"finishReason":"STOP"}
        ]});
        assert!(decode_response(&multiple, "m").is_err());

        let malformed_call = json!({"candidates":[{"content":{"parts":[{"functionCall":{"name":"lookup","args":[]}}]},"finishReason":"STOP"}]});
        assert!(decode_response(&malformed_call, "m").is_err());
    }
}
