//! Freebuff catalog: model list, agent mapping, and config constants.
//!
//! The adapter serves a fixed public catalog; upstream model discovery is a
//! static list by design, not a network call.

pub(super) const FREEBUFF_BASE_URL: &str = "https://www.codebuff.com/api/v1";
pub(super) const FREEBUFF_CHAT_COMPLETIONS_PATH: &str = "chat/completions";
pub(super) const FREEBUFF_SESSION_PATH: &str = "freebuff/session";
pub(super) const FREEBUFF_AGENT_RUNS_PATH: &str = "agent-runs";
pub(super) const FREEBUFF_MODELS_PATH: &str = "models";
pub(super) const FREEBUFF_PROMPT: &str = "You are Buffy, the strategic coding assistant.";
pub(super) const FREEBUFF_SESSION_USER_AGENT: &str = "codebuff/0.1.0 (darwin-arm64)";
pub(super) const FREEBUFF_COMPLETION_USER_AGENT: &str = "ai-sdk/openai-compatible/1.0.25/codebuff";
pub(super) const FREEBUFF_COMPLETION_ACCEPT: &str = "application/json, text/event-stream";
pub(super) const MAX_FREEBUFF_ID_BYTES: usize = 256;
pub(super) const FREEBUFF_GLM_V53_FLASH_MODEL_ID: &str = "z-ai/glm-5.3-flash";

pub(super) const FREEBUFF_MODELS: &[&str] = &[
    "deepseek/deepseek-v4-flash",
    "deepseek/deepseek-v4-pro",
    "openai/gpt-5.6-luna",
    "minimax/minimax-m3",
    "mimo/mimo-v2.5",
    "z-ai/glm-5.2",
    FREEBUFF_GLM_V53_FLASH_MODEL_ID,
    "crof/kimi-k3-eco",
    "anthropic/claude-fable-5",
    "meta/muse-spark-1.2-contributor",
];

pub(super) const MODEL_TO_AGENT: &[(&str, &str)] = &[
    ("deepseek/deepseek-v4-flash", "base2-free-deepseek-flash"),
    ("deepseek/deepseek-v4-pro", "base2-free-deepseek"),
    ("openai/gpt-5.6-luna", "base2-free-luna"),
    ("minimax/minimax-m3", "base2-free-minimax-m3"),
    ("mimo/mimo-v2.5", "base2-free-mimo"),
    ("z-ai/glm-5.2", "base2-free-glm"),
    (FREEBUFF_GLM_V53_FLASH_MODEL_ID, "base2-free-glm-5-3-flash"),
    ("crof/kimi-k3-eco", "base2-free-kimi-k3-eco"),
    ("anthropic/claude-fable-5", "base2-free-fable"),
    ("meta/muse-spark-1.2-contributor", "base2-free-muse-spark"),
];

pub(super) fn requested_model(model: &str) -> &str {
    model
        .strip_prefix("fb/")
        .or_else(|| model.strip_prefix("freebuff/"))
        .unwrap_or(model)
}

pub(super) fn agent_for_model(model: &str) -> &'static str {
    MODEL_TO_AGENT
        .iter()
        .find_map(|(known_model, agent)| (*known_model == model).then_some(*agent))
        .unwrap_or("base2-free")
}

pub(super) fn catalog_models() -> Vec<String> {
    FREEBUFF_MODELS
        .iter()
        .map(|model| (*model).to_owned())
        .collect()
}
