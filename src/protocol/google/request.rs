use super::shared::{
    InlineKind, encode_tool_choice, encode_tool_result, image_data_uri, tool_call_names,
    validate_function_name, validate_inline_data,
};
use super::*;

/// Encodes ExoRoute's protocol-neutral request for Google's GenerateContent API.
///
/// Streaming is selected by the caller's endpoint (`streamGenerateContent`); the
/// `stream` flag is therefore intentionally not copied into the JSON body.
pub(in crate::protocol) fn encode_request(
    request: &CanonicalRequest,
    model: &str,
) -> Result<Value, String> {
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
