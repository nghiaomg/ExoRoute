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
    apply_google_metadata(&request.metadata, &mut generation_config)?;
    if !generation_config.is_empty() {
        body["generationConfig"] = Value::Object(generation_config);
    }

    Ok(body)
}

fn apply_google_metadata(
    metadata: &BTreeMap<String, Value>,
    generation_config: &mut serde_json::Map<String, Value>,
) -> Result<(), String> {
    if metadata.is_empty() {
        return Ok(());
    }

    let mut unsupported = Vec::new();
    for (key, value) in metadata {
        let lower = key.to_ascii_lowercase();
        match lower.as_str() {
            // Chat-only and client-level options that have no Google Generate Content equivalent
            // or are handled out-of-band (e.g. streaming usage).
            "n"
            | "seed"
            | "user"
            | "metadata"
            | "logprobs"
            | "top_logprobs"
            | "logit_bias"
            | "safety_identifier"
            | "stream_options"
            | "parallel_tool_calls"
            | "store"
            | "service_tier"
            | "prompt_cache_key"
            | "prompt_cache_keys"
            | "echo"
            | "verbosity" => {
                // Safely dropped
            }

            "frequency_penalty" | "frequencypenalty" => {
                if let Some(num) = value.as_f64() {
                    let num = num as f32;
                    if !num.is_finite() || !(-2.0..=2.0).contains(&num) {
                        return Err("frequency_penalty must be between -2 and 2".to_owned());
                    }
                    generation_config.insert("frequencyPenalty".to_owned(), json!(num));
                } else if !value.is_null() {
                    return Err("frequency_penalty must be a number".to_owned());
                }
            }

            "presence_penalty" | "presencepenalty" => {
                if let Some(num) = value.as_f64() {
                    let num = num as f32;
                    if !num.is_finite() || !(-2.0..=2.0).contains(&num) {
                        return Err("presence_penalty must be between -2 and 2".to_owned());
                    }
                    generation_config.insert("presencePenalty".to_owned(), json!(num));
                } else if !value.is_null() {
                    return Err("presence_penalty must be a number".to_owned());
                }
            }

            "top_k" | "topk" => {
                if let Some(k) = value.as_u64()
                    && k > 0
                {
                    generation_config.insert("topK".to_owned(), json!(k));
                }
            }

            "response_format" | "responseformat" => {
                if let Some(text) = value.as_str() {
                    let text = text.trim().to_ascii_lowercase();
                    match text.as_str() {
                        "text" => {}
                        "json" | "json_object" => {
                            generation_config
                                .insert("responseMimeType".to_owned(), json!("application/json"));
                        }
                        other => {
                            return Err(format!(
                                "unsupported response_format type for Google: {other}"
                            ));
                        }
                    }
                } else if let Some(obj) = value.as_object() {
                    let format_type = obj
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("text")
                        .trim()
                        .to_ascii_lowercase();
                    match format_type.as_str() {
                        "text" => {}
                        "json_object" => {
                            generation_config
                                .insert("responseMimeType".to_owned(), json!("application/json"));
                        }
                        "json_schema" => {
                            generation_config
                                .insert("responseMimeType".to_owned(), json!("application/json"));
                            if let Some(schema) =
                                obj.get("json_schema").and_then(|js| js.get("schema"))
                            {
                                generation_config
                                    .insert("responseSchema".to_owned(), schema.clone());
                            }
                        }
                        other => {
                            return Err(format!(
                                "unsupported response_format type for Google: {other}"
                            ));
                        }
                    }
                } else if !value.is_null() {
                    return Err(
                        "request option 'response_format' must be an object or string".to_owned(),
                    );
                }
            }

            "reasoning_effort" | "reasoningeffort" | "effort" => {
                if let Some(effort) = value.as_str() {
                    let effort = effort.trim().to_ascii_lowercase();
                    match effort.as_str() {
                        "none" => {
                            generation_config
                                .insert("thinkingConfig".to_owned(), json!({"thinkingBudget": 0}));
                        }
                        "low" | "medium" | "high" => {
                            let thinking_config = generation_config
                                .entry("thinkingConfig".to_owned())
                                .or_insert_with(|| json!({}));
                            if let Some(tc) = thinking_config.as_object_mut() {
                                tc.insert("thinkingLevel".to_owned(), json!(effort));
                            }
                        }
                        _ => {}
                    }
                }
            }

            "enable_thinking" | "enablethinking" => {
                if let Some(enabled) = value.as_bool()
                    && !enabled
                {
                    generation_config
                        .insert("thinkingConfig".to_owned(), json!({"thinkingBudget": 0}));
                }
            }

            "thinking" => {
                if let Some(obj) = value.as_object() {
                    if let Some(budget) = obj
                        .get("budget_tokens")
                        .or_else(|| obj.get("budget"))
                        .and_then(Value::as_u64)
                    {
                        generation_config.insert(
                            "thinkingConfig".to_owned(),
                            json!({"thinkingBudget": budget}),
                        );
                    } else if obj.get("type").and_then(Value::as_str) == Some("disabled") {
                        generation_config
                            .insert("thinkingConfig".to_owned(), json!({"thinkingBudget": 0}));
                    }
                } else if let Some(enabled) = value.as_bool()
                    && !enabled
                {
                    generation_config
                        .insert("thinkingConfig".to_owned(), json!({"thinkingBudget": 0}));
                }
            }

            _ => {
                unsupported.push(key.as_str());
            }
        }
    }

    if !unsupported.is_empty() {
        return Err(format!(
            "Google Generate Content cannot preserve protocol-specific request metadata: {}",
            unsupported.join(", ")
        ));
    }

    Ok(())
}
