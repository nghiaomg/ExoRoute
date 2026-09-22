//! Responses-specific request option normalization.
//!
//! Keeping metadata validation apart from protocol payload/content encoding
//! prevents new request options from expanding the transport codec.

use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(crate) const MAX_TRANSLATION_OPTION_DIAGNOSTICS: usize = 8;
pub(crate) const MAX_TRANSLATION_OPTION_NAME_CHARS: usize = 128;

/// Upper bound for a safe-passthrough option value. `prompt_cache_key` is a
/// routing hint, not content, so anything past this length is a misuse rather
/// than a legitimate cache key.
pub(crate) const MAX_SAFE_PASSTHROUGH_VALUE_CHARS: usize = 256;

/// Chat Completions options with no Responses equivalent that provider adapters
/// strip in `prepare_body`. Translating a request carrying one of these to a
/// Responses provider is safe because the adapter removes the field before
/// dispatch. Keep this list in sync with the codex adapter's strip list; it
/// covers the Chat Completions-native fields on that list.
pub(crate) const CHAT_ONLY_DROPPED_OPTIONS: &[&str] = &[
    "n",
    "seed",
    "user",
    "metadata",
    "logprobs",
    "top_logprobs",
    "logit_bias",
    "frequency_penalty",
    "presence_penalty",
    "safety_identifier",
    "stream_options",
];

/// Options whose field name and semantics are identical on Chat Completions and
/// Responses, so they can forward between the two OpenAI-family protocols by
/// name. `prompt_cache_key` is a cache-routing hint, not a credential: both
/// APIs treat it as an opaque string, and routing identical prompts with the
/// same key is the intended behavior on both. Membership is matched on the
/// exact option name, so look-alike names such as `prompt_cache_keys` still
/// fall through to the safety guard in the chat encoder.
pub(crate) const SAFE_PASSTHROUGH_OPTIONS: &[&str] = &["prompt_cache_key"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResponsesThinkingIntent {
    Enabled,
    Disabled,
    Effort(String),
}

pub(crate) fn apply_responses_metadata(
    metadata: &BTreeMap<String, Value>,
    target: &mut Value,
    model: &str,
) -> Result<(), String> {
    if metadata.is_empty() {
        return Ok(());
    }

    let mut unsupported = Vec::new();
    let mut supported = 0;
    let mut effort = None;
    let mut reasoning_effort = None;
    let mut reasoning = None;
    let mut thinking = None;
    let mut enable_thinking = None;
    let mut verbosity = None;
    let mut response_format = None;
    let mut store = None;
    let mut parallel_tool_calls = None;
    for (key, value) in metadata {
        if key.eq_ignore_ascii_case("effort") {
            supported += 1;
            if effort.is_some() {
                return Err("request option 'effort' was provided more than once".to_owned());
            }
            effort = Some(value);
        } else if key.eq_ignore_ascii_case("reasoning_effort") {
            supported += 1;
            if reasoning_effort.is_some() {
                return Err(
                    "request option 'reasoning_effort' was provided more than once".to_owned(),
                );
            }
            reasoning_effort = Some(value);
        } else if key.eq_ignore_ascii_case("reasoning") {
            supported += 1;
            if reasoning.is_some() {
                return Err("request option 'reasoning' was provided more than once".to_owned());
            }
            reasoning = Some(value);
        } else if key.eq_ignore_ascii_case("thinking") {
            supported += 1;
            if thinking.is_some() {
                return Err("request option 'thinking' was provided more than once".to_owned());
            }
            thinking = Some(value);
        } else if key.eq_ignore_ascii_case("enable_thinking") {
            supported += 1;
            if enable_thinking.is_some() {
                return Err(
                    "request option 'enable_thinking' was provided more than once".to_owned(),
                );
            }
            enable_thinking = Some(value);
        } else if CHAT_ONLY_DROPPED_OPTIONS
            .iter()
            .any(|name| key.eq_ignore_ascii_case(name))
        {
            supported += 1;
            // Chat Completions-only options with no Responses equivalent
            // (stream_options because usage is decoded from the Responses event
            // stream, and sampling options like n that Responses never accepted).
            // Provider adapters strip these in prepare_body before dispatch, so
            // dropping them here matches what the provider would receive.
        } else if let Some(canonical) = SAFE_PASSTHROUGH_OPTIONS
            .iter()
            .find(|name| key.eq_ignore_ascii_case(name))
        {
            supported += 1;
            // Identical field name and semantics on both OpenAI-family
            // protocols, so the value forwards to the Responses request
            // unchanged after the shared safety validation below.
            if let Some(text) = value.as_str() {
                if text.trim().is_empty() {
                    return Err(format!("request option '{key}' must be a non-empty string"));
                }
                if !text.is_ascii() {
                    return Err(format!("request option '{key}' must be an ASCII string"));
                }
                let len = text.chars().count();
                if len > MAX_SAFE_PASSTHROUGH_VALUE_CHARS {
                    return Err(format!(
                        "request option '{key}' must be at most {MAX_SAFE_PASSTHROUGH_VALUE_CHARS} characters"
                    ));
                }
            } else if !value.is_null() {
                return Err(format!(
                    "request option '{key}' must be a string when provided"
                ));
            }
            if let Some(text) = value.as_str() {
                target[*canonical] = json!(text);
            }
        } else if key.eq_ignore_ascii_case("verbosity") {
            supported += 1;
            if verbosity.is_some() {
                return Err("request option 'verbosity' was provided more than once".to_owned());
            }
            verbosity = Some(value);
        } else if key.eq_ignore_ascii_case("response_format") {
            supported += 1;
            if response_format.is_some() {
                return Err(
                    "request option 'response_format' was provided more than once".to_owned(),
                );
            }
            response_format = Some(value);
        } else if key.eq_ignore_ascii_case("store") {
            supported += 1;
            if store.is_some() {
                return Err("request option 'store' was provided more than once".to_owned());
            }
            store = Some(value);
        } else if key.eq_ignore_ascii_case("parallel_tool_calls") {
            supported += 1;
            if parallel_tool_calls.is_some() {
                return Err(
                    "request option 'parallel_tool_calls' was provided more than once".to_owned(),
                );
            }
            parallel_tool_calls = Some(value);
        } else if unsupported.len() < MAX_TRANSLATION_OPTION_DIAGNOSTICS {
            unsupported.push(bound_option_name(key));
        }
    }

    if !unsupported.is_empty() {
        let omitted = metadata.len().saturating_sub(unsupported.len() + supported);
        let mut message = format!(
            "request options cannot be translated to Responses: {}",
            unsupported.join(", ")
        );
        if omitted > 0 {
            message.push_str(&format!(", and {omitted} more"));
        }
        return Err(message);
    }

    let thinking_intent = match (thinking, enable_thinking) {
        (Some(thinking), Some(enable_thinking)) => {
            let thinking = normalize_responses_thinking_intent("thinking", thinking, model)?;
            let enable_thinking =
                normalize_responses_thinking_intent("enable_thinking", enable_thinking, model)?;
            if thinking != enable_thinking
                && reasoning.is_none()
                && reasoning_effort.is_none()
                && effort.is_none()
            {
                return Err("request options 'thinking' and 'enable_thinking' conflict".to_owned());
            }
            Some(thinking)
        }
        (Some(value), None) => Some(normalize_responses_thinking_intent(
            "thinking", value, model,
        )?),
        (None, Some(value)) => Some(normalize_responses_thinking_intent(
            "enable_thinking",
            value,
            model,
        )?),
        (None, None) => None,
    };

    // Match OmniRoute's precedence: an explicit Responses-shaped reasoning
    // object wins over a Chat-shaped effort hint, which wins over the generic
    // thinking aliases. Lower-priority aliases are still validated above so a
    // malformed client option cannot be silently accepted.
    if let Some(value) = reasoning {
        target["reasoning"] = normalize_responses_reasoning(value, model)?;
    } else if let Some(value) = reasoning_effort {
        target["reasoning"] = json!({
            "effort": normalize_responses_effort(value, "reasoning_effort", model)?
        });
    } else if let Some(value) = effort {
        target["reasoning"] = json!({
            "effort": normalize_responses_effort(value, "effort", model)?
        });
    } else if let Some(intent) = thinking_intent {
        match intent {
            ResponsesThinkingIntent::Disabled => {
                target["reasoning"] = json!({"effort": "none"});
            }
            ResponsesThinkingIntent::Effort(effort) => {
                target["reasoning"] = json!({"effort": effort});
            }
            // Responses has no boolean enable-thinking field. Omitting
            // reasoning lets the model use its configured/default effort.
            ResponsesThinkingIntent::Enabled => {}
        }
    }

    if let Some(value) = verbosity {
        let verbosity = value
            .as_str()
            .ok_or("request option 'verbosity' must be a string")?;
        if !matches!(verbosity, "low" | "medium" | "high") {
            return Err("request option 'verbosity' must be one of low, medium or high".to_owned());
        }
        target["text"] = json!({"verbosity": verbosity});
    }

    if let Some(value) = response_format
        && let Some(format) = normalize_responses_response_format(value)?
    {
        if let Some(text) = target.get_mut("text").and_then(Value::as_object_mut) {
            text.insert("format".to_owned(), format);
        } else {
            target["text"] = json!({"format": format});
        }
    }

    // Both fields are native to the Responses API with identical semantics, so
    // they pass through. store=false in particular must survive translation:
    // dropping it would let a Responses provider default to store=true and
    // retain client conversation data the sender explicitly declined.
    // Explicit null means "unset", matching the option handling in parse.rs.
    if let Some(value) = store.filter(|value| !value.is_null()) {
        let stored = value
            .as_bool()
            .ok_or("request option 'store' must be a boolean")?;
        target["store"] = json!(stored);
    }
    if let Some(value) = parallel_tool_calls.filter(|value| !value.is_null()) {
        let parallel = value
            .as_bool()
            .ok_or("request option 'parallel_tool_calls' must be a boolean")?;
        target["parallel_tool_calls"] = json!(parallel);
    }

    Ok(())
}

pub(crate) fn normalize_responses_response_format(value: &Value) -> Result<Option<Value>, String> {
    let response_format = value
        .as_object()
        .ok_or("request option 'response_format' must be an object")?;
    let format_type = response_format
        .get("type")
        .and_then(Value::as_str)
        .ok_or("request option 'response_format.type' must be a string")?
        .trim()
        .to_ascii_lowercase();

    match format_type.as_str() {
        // Text is the Responses default, so no additional field is needed.
        "text" => Ok(None),
        "json_object" => Ok(Some(json!({"type": "json_object"}))),
        "json_schema" => {
            let json_schema = response_format
                .get("json_schema")
                .and_then(Value::as_object)
                .ok_or("request option 'response_format.json_schema' must be an object")?;
            let schema = json_schema
                .get("schema")
                .ok_or("request option 'response_format.json_schema.schema' is required")?;
            let name = json_schema
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .unwrap_or("codex_output_schema");
            let mut format = json!({
                "type": "json_schema",
                "name": name,
                "schema": schema,
            });
            if let Some(description) = json_schema.get("description") {
                format["description"] = description.clone();
            }
            if let Some(strict) = json_schema.get("strict") {
                format["strict"] = strict.clone();
            }
            Ok(Some(format))
        }
        other => Err(format!(
            "request option 'response_format.type' is not supported: {other}"
        )),
    }
}

pub(crate) fn normalize_responses_effort(
    value: &Value,
    option: &str,
    model: &str,
) -> Result<String, String> {
    let effort = value
        .as_str()
        .ok_or_else(|| format!("request option '{option}' must be a string"))?
        .trim()
        .to_ascii_lowercase();
    if !matches!(
        effort.as_str(),
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
    ) {
        return Err(format!(
            "request option '{option}' must be one of none, minimal, low, medium, high, xhigh, max or ultra"
        ));
    }
    // "minimal" is only accepted by newer Responses models; older models list
    // exactly the universal levels, so degrade it to the closest supported
    // effort instead of failing the request upstream.
    if effort == "minimal" && !supports_native_reasoning_effort(model, "minimal") {
        return Ok("low".to_owned());
    }
    if matches!(effort.as_str(), "max" | "ultra") && !supports_native_reasoning_effort(model, "max")
    {
        return Ok("xhigh".to_owned());
    }
    if effort == "ultra" {
        return Ok("max".to_owned());
    }
    Ok(effort)
}

/// Returns whether the model accepts `effort` natively in the Responses API.
/// Effort support is model-dependent: the OpenAI Codex tier (gpt-5.6-*/gpt-6)
/// rejects "minimal" but adds native "max", while earlier OpenAI reasoning
/// models accept "minimal" and reject "max". Other model names forward the
/// documented value unchanged and rely on the provider as the authority.
/// Model suffixes such as gpt-5.6-luna-high name a variant, so they are
/// stripped before matching.
pub(crate) fn supports_native_reasoning_effort(model: &str, effort: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    let model = model
        .strip_prefix("codex/")
        .or_else(|| model.strip_prefix("cx/"))
        .unwrap_or(&model);
    let base_model = [
        "-none", "-minimal", "-low", "-medium", "-high", "-xhigh", "-max", "-ultra",
    ]
    .iter()
    .find_map(|suffix| model.strip_suffix(suffix))
    .unwrap_or(model);
    let codex_tier = matches!(
        base_model,
        "gpt-5.6-sol" | "gpt-5.6-terra" | "gpt-5.6-luna" | "gpt-6-astra"
    );
    match effort {
        "max" => codex_tier,
        "minimal" => !codex_tier,
        _ => true,
    }
}

pub(crate) fn normalize_responses_reasoning(value: &Value, model: &str) -> Result<Value, String> {
    match value {
        Value::String(_) => Ok(json!({
            "effort": normalize_responses_effort(value, "reasoning", model)?
        })),
        Value::Object(object) => {
            let mut normalized = object.clone();
            if let Some(effort) = object.get("effort") {
                normalized.insert(
                    "effort".to_owned(),
                    Value::String(normalize_responses_effort(
                        effort,
                        "reasoning.effort",
                        model,
                    )?),
                );
            }
            Ok(Value::Object(normalized))
        }
        _ => Err("request option 'reasoning' must be an object or string".to_owned()),
    }
}

pub(crate) fn normalize_responses_thinking_intent(
    option: &str,
    value: &Value,
    model: &str,
) -> Result<ResponsesThinkingIntent, String> {
    match value {
        Value::Bool(enabled) => Ok(if *enabled {
            ResponsesThinkingIntent::Enabled
        } else {
            ResponsesThinkingIntent::Disabled
        }),
        Value::Object(object) => {
            let explicit_type = object
                .get("type")
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| format!("request option '{option}.type' must be a string"))
                        .map(|value| value.trim().to_ascii_lowercase())
                })
                .transpose()?;

            let explicit_effort = object
                .get("effort")
                .map(|value| normalize_responses_effort(value, &format!("{option}.effort"), model))
                .transpose()?;

            let budget_effort = object
                .get("budget_tokens")
                .map(|value| {
                    let budget = value.as_u64().ok_or_else(|| {
                        format!(
                            "request option '{option}.budget_tokens' must be a non-negative integer"
                        )
                    })?;
                    Ok::<Option<String>, String>(if budget == 0 {
                        None
                    } else if budget <= 1_024 {
                        Some("low".to_owned())
                    } else if budget <= 10_240 {
                        Some("medium".to_owned())
                    } else if budget < 131_072 {
                        Some("high".to_owned())
                    } else {
                        Some("xhigh".to_owned())
                    })
                })
                .transpose()?
                .flatten();

            let requested_effort = explicit_effort.or(budget_effort);
            let disabled = matches!(
                explicit_type.as_deref(),
                Some("disabled" | "disable" | "off" | "none")
            );
            let enabled = matches!(
                explicit_type.as_deref(),
                Some("enabled" | "enable" | "on" | "adaptive" | "auto")
            );
            if explicit_type.is_some() && !disabled && !enabled {
                return Err(format!(
                    "request option '{option}.type' must be one of disabled, enabled or adaptive"
                ));
            }
            if disabled {
                if requested_effort
                    .as_deref()
                    .is_some_and(|effort| effort != "none")
                {
                    return Err(format!(
                        "request option '{option}' disables thinking but also specifies an active effort"
                    ));
                }
                return Ok(ResponsesThinkingIntent::Disabled);
            }
            if let Some(effort) = requested_effort {
                return Ok(ResponsesThinkingIntent::Effort(effort));
            }
            Ok(ResponsesThinkingIntent::Enabled)
        }
        _ => Err(format!(
            "request option '{option}' must be a boolean or object"
        )),
    }
}

pub(crate) fn bound_option_name(name: &str) -> String {
    if name.chars().count() <= MAX_TRANSLATION_OPTION_NAME_CHARS {
        return name.to_owned();
    }
    let mut bounded = name
        .chars()
        .take(MAX_TRANSLATION_OPTION_NAME_CHARS)
        .collect::<String>();
    bounded.push_str("...");
    bounded
}
