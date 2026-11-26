//! Stable identifiers for built-in provider presets and adapters.
//!
//! These values are part of the public configuration surface: stored provider
//! records, preset catalog entries, and dashboard requests use them verbatim.
//! Keep them in one place so an id is never defined twice with a drifted value.

pub const GENERIC_ADAPTER_ID: &str = "generic";
pub const DEVIN_CLI_ADAPTER_ID: &str = "devin_cli";
pub const DEVIN_DESKTOP_ADAPTER_ID: &str = "devin_desktop";
pub const CODEX_ADAPTER_ID: &str = "openai_codex";
pub const KILO_GATEWAY_ADAPTER_ID: &str = "kilo_gateway";
pub const KILOCODE_ADAPTER_ID: &str = "kilocode";
pub const COMMAND_CODE_ADAPTER_ID: &str = "command_code";
pub const OPENCODE_GO_ADAPTER_ID: &str = "opencode_go";
pub const OPENCODE_ZEN_ADAPTER_ID: &str = "opencode_zen";
pub const OPENROUTER_ADAPTER_ID: &str = "openrouter";
pub const CLINE_ADAPTER_ID: &str = "cline";
pub const CLINEPASS_ADAPTER_ID: &str = "clinepass";
pub const NVIDIA_NIM_ADAPTER_ID: &str = "nvidia_nim";
pub const ANTIGRAVITY_ADAPTER_ID: &str = "antigravity";
pub const FREEBUFF_ADAPTER_ID: &str = "freebuff";
pub const MUSE_ADAPTER_ID: &str = "muse";
pub const QODER_ADAPTER_ID: &str = "qoder";
pub const QODER_CN_ADAPTER_ID: &str = "qoder-cn";
pub const KIRO_ADAPTER_ID: &str = "kiro";
pub const OPENAI_ADAPTER_ID: &str = "openai";
pub const ANTHROPIC_ADAPTER_ID: &str = "anthropic";
pub const GEMINI_ADAPTER_ID: &str = "gemini";
pub const DEEPSEEK_ADAPTER_ID: &str = "deepseek";
pub const XAI_ADAPTER_ID: &str = "xai";
pub const MISTRAL_ADAPTER_ID: &str = "mistral";
pub const PERPLEXITY_ADAPTER_ID: &str = "perplexity";
pub const GROQ_ADAPTER_ID: &str = "groq";
pub const CEREBRAS_ADAPTER_ID: &str = "cerebras";
pub const DASHSCOPE_ADAPTER_ID: &str = "dashscope";
pub const TOGETHER_ADAPTER_ID: &str = "together";
pub const FIREWORKS_ADAPTER_ID: &str = "fireworks";
