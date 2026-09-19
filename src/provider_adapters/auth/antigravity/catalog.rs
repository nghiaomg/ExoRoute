use serde_json::Value;
use std::collections::HashSet;

const MAX_MODEL_ROWS: usize = 10_000;

// Known non-chat surfaces from the authenticated Antigravity catalog.
const ANTIGRAVITY_NON_CHAT_MODEL_IDS: &[&str] = &[
    "gemini-3-pro-image-preview",
    "gemini-3.1-flash-image",
    "gemini-3.1-flash-tts-preview",
    "gemini-2.5-flash-preview-tts",
    "tab_flash_lite_preview",
    "tab_jump_flash_lite_preview",
];

// Retired chat models that upstream may still report but must not be offered.
const ANTIGRAVITY_RETIRED_MODEL_IDS: &[&str] = &[
    "gemini-3-pro-preview",
    "gemini-3.1-pro",
    "gemini-3.6-flash-high",
    "gemini-3.6-flash-medium",
    "gemini-3.6-flash-low",
    "gemini-3-flash-agent",
    "gemini-3.5-flash",
    "gemini-3.5-flash-extra-low",
    "gemini-3.5-flash-low",
    "gemini-3.5-flash-high",
    "gemini-3.5-flash-medium",
    "gemini-3.5-flash-preview",
    "gemini-2.5-pro",
    "gemini-2.5-flash-thinking",
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite",
    "gemini-2.5-computer-use-preview-10-2025",
];

const ANTIGRAVITY_NON_CHAT_MODALITIES: &[&str] = &[
    "image",
    "imagen",
    "audio",
    "tts",
    "embedding",
    "embed",
    "video",
    "veo",
];

fn has_non_chat_modality_segment(lower_model: &str) -> bool {
    lower_model
        .split(['-', '_'])
        .any(|segment| ANTIGRAVITY_NON_CHAT_MODALITIES.contains(&segment))
}

fn has_tab_segment(lower_model: &str) -> bool {
    lower_model
        .split(['-', '_'])
        .any(|segment| segment == "tab")
}

pub(super) fn parse_model_list(value: &Value) -> Vec<String> {
    let mut models = Vec::new();
    let mut seen = HashSet::new();
    let container = value
        .get("models")
        .or_else(|| value.get("data"))
        .unwrap_or(value);
    if let Some(rows) = container.as_array() {
        for row in rows {
            if is_hidden_catalog_row(row) {
                continue;
            }
            if let Some(model) = discovered_model_id(row, None)
                && seen.insert(model.clone())
            {
                models.push(model);
            }
            if models.len() >= MAX_MODEL_ROWS {
                break;
            }
        }
    } else if let Some(object) = container.as_object() {
        for (id, row) in object {
            if is_hidden_catalog_row(row) {
                continue;
            }
            if let Some(model) = discovered_model_id(row, Some(id.as_str()))
                && seen.insert(model.clone())
            {
                models.push(model);
            }
            if models.len() >= MAX_MODEL_ROWS {
                break;
            }
        }
    }
    models
}

fn is_hidden_catalog_row(row: &Value) -> bool {
    row.get("isInternal").and_then(Value::as_bool) == Some(true)
        || row.get("is_internal").and_then(Value::as_bool) == Some(true)
        || row.get("hidden").and_then(Value::as_bool) == Some(true)
        || row.get("disabled").and_then(Value::as_bool) == Some(true)
}

/// Mirrors the reference quota views: a discovery entry without `quotaInfo`
/// is a catalog slot rather than an enabled model. String rows carry no
/// per-row metadata, so keep them and let the id filter below decide.
fn has_quota_signal(row: &Value) -> bool {
    if row.is_string() {
        return true;
    }
    let quota_info = row.get("quotaInfo").or_else(|| row.get("quota_info"));
    match quota_info {
        None => false,
        Some(info) if info.is_null() => false,
        Some(info) if info.is_object() => !info.as_object().is_some_and(|map| map.is_empty()),
        Some(_) => true,
    }
}

/// Redacted discovery-shape log: ids only, no tokens, quota payloads, or
/// request bodies. Used to confirm which path still serves placeholder slots.
pub(super) fn log_discovery_shape(value: &Value, base: &str, path: &str) {
    let container = value
        .get("models")
        .or_else(|| value.get("data"))
        .unwrap_or(value);
    let (total, sample) = if let Some(rows) = container.as_array() {
        let sample = rows
            .iter()
            .take(8)
            .filter_map(|row| raw_model_id(row, None).map(str::to_owned))
            .collect::<Vec<_>>();
        (rows.len(), sample)
    } else if let Some(object) = container.as_object() {
        let sample = object
            .keys()
            .take(8)
            .map(String::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        (object.len(), sample)
    } else {
        (0, Vec::new())
    };
    tracing::debug!(
        antigravity_base = base,
        antigravity_path = path,
        catalog_entries = total,
        catalog_sample = ?sample,
        "Antigravity model discovery response shape"
    );
}

fn raw_model_id<'a>(value: &'a Value, fallback: Option<&'a str>) -> Option<&'a str> {
    value
        .as_str()
        .or_else(|| value.get("modelId").and_then(Value::as_str))
        .or_else(|| value.get("model_id").and_then(Value::as_str))
        .or_else(|| value.get("model").and_then(Value::as_str))
        .or_else(|| value.get("id").and_then(Value::as_str))
        .or_else(|| value.get("name").and_then(Value::as_str))
        .or(fallback)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn model_id(value: &Value, fallback: Option<&str>) -> Option<String> {
    raw_model_id(value, fallback).map(normalize_model_id)
}

fn discovered_model_id(value: &Value, fallback: Option<&str>) -> Option<String> {
    if !has_quota_signal(value) {
        return None;
    }
    let raw = raw_model_id(value, fallback)?;
    let model = normalize_discovered_model_id(raw)?;
    if !is_supported_chat_model_id(&model) {
        return None;
    }
    Some(model)
}

pub(crate) fn normalize_model_id(model: &str) -> String {
    let model = model.trim();
    let model = model
        .strip_prefix("antigravity/")
        .or_else(|| model.strip_prefix("ag/"))
        .unwrap_or(model);
    match model {
        "gemini-3.7-flash"
        | "gemini-3.7-flash-high"
        | "gemini-3.7-flash-medium"
        | "gemini-3.7-flash-low" => "gemini-3.7-flash-tiered".to_owned(),
        "gpt-oss-120b" => "gpt-oss-120b-medium".to_owned(),
        "gemini-3.1-pro-high" => "gemini-pro-agent".to_owned(),
        other => other.to_owned(),
    }
}

fn normalize_discovered_model_id(model: &str) -> Option<String> {
    let model = model
        .strip_prefix("antigravity/")
        .or_else(|| model.strip_prefix("ag/"))
        .unwrap_or(model)
        .trim();
    if model.is_empty() {
        return None;
    }
    let alias = match model {
        "gemini-3.7-flash" => "gemini-3.7-flash-tiered",
        "gpt-oss-120b" => "gpt-oss-120b-medium",
        "gemini-3.1-pro-high" => "gemini-pro-agent",
        "gemini-claude-sonnet-4-5" | "gemini-claude-sonnet-4-5-thinking" => "claude-sonnet-4-6",
        "gemini-claude-opus-4-5-thinking" => "claude-opus-4-6-thinking",
        "MODEL_OPENAI_GPT_OSS_120B_MEDIUM" => "gpt-oss-120b-medium",
        other => other,
    };
    // Upstream enum slots (`MODEL_*`) carry no lowercase chat id beyond the
    // explicitly mapped ones above; drop the rest instead of guessing.
    if alias
        .bytes()
        .any(|byte| byte.is_ascii_uppercase() || byte == b' ')
    {
        return None;
    }
    Some(alias.to_owned())
}

fn is_supported_chat_model_id(model: &str) -> bool {
    if model.is_empty() || model.len() > 256 {
        return false;
    }
    if ANTIGRAVITY_NON_CHAT_MODEL_IDS.contains(&model)
        || ANTIGRAVITY_RETIRED_MODEL_IDS.contains(&model)
    {
        return false;
    }
    let lower = model.to_ascii_lowercase();
    if lower.starts_with("model_")
        || lower.contains("placeholder")
        || lower.contains("internal")
        || lower.contains("retired")
        || has_tab_segment(&lower)
    {
        return false;
    }
    !has_non_chat_modality_segment(&lower)
}

pub(super) fn is_supported_model(model: &str) -> bool {
    // Shared with the quota path, which normalizes through normalize_model_id
    // rather than the discovery alias table.
    is_supported_chat_model_id(model)
}

/// Returns true for catalog rows that were already normalized and stored before
/// discovery learned to drop placeholders, retired, and non-chat surfaces.
/// Stored ids are validated with the request-time normalizer first so a row
/// such as `gemini-3.7-flash-high` keeps its live chat meaning.
pub(crate) fn is_stale_unsupported_model(model: &str) -> bool {
    let trimmed = model.trim();
    // Raw enum slots must never survive in a stored catalog, even the one
    // mapped id that discovery still accepts live.
    if trimmed
        .bytes()
        .any(|byte| byte.is_ascii_uppercase() || byte == b' ')
    {
        return true;
    }
    let normalized = normalize_model_id(trimmed);
    let Some(renormalized) = normalize_discovered_model_id(&normalized) else {
        return true;
    };
    !is_supported_chat_model_id(&renormalized)
}
