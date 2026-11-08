use super::{
    CLINE_ADAPTER_ID, CLINEPASS_ADAPTER_ID, DEVIN_CLI_ADAPTER_ID, ProviderAdapter, antigravity,
    azure, cloud_api, command_code, compat, devin_cli, freebuff, generic, kilo, kilocode, muse,
    nvidia_nim, opencode, openrouter, qoder, vertex,
};
use serde_json::Value;

pub(crate) struct ClineAdapter {
    pub(crate) adapter_id: &'static str,
}

static CLINE_ADAPTER: ClineAdapter = ClineAdapter {
    adapter_id: CLINE_ADAPTER_ID,
};

static CLINEPASS_ADAPTER: ClineAdapter = ClineAdapter {
    adapter_id: CLINEPASS_ADAPTER_ID,
};

static ADAPTERS: [&'static dyn ProviderAdapter; 42] = [
    &generic::GENERIC_ADAPTER,
    &devin_cli::DEVIN_CLI_ADAPTER,
    &cloud_api::OPENAI_ADAPTER,
    &cloud_api::ANTHROPIC_ADAPTER,
    &cloud_api::GEMINI_ADAPTER,
    &cloud_api::DEEPSEEK_ADAPTER,
    &cloud_api::XAI_ADAPTER,
    &cloud_api::MISTRAL_ADAPTER,
    &cloud_api::PERPLEXITY_ADAPTER,
    &cloud_api::GROQ_ADAPTER,
    &cloud_api::CEREBRAS_ADAPTER,
    &cloud_api::DASHSCOPE_ADAPTER,
    &cloud_api::TOGETHER_ADAPTER,
    &cloud_api::FIREWORKS_ADAPTER,
    &kilo::KILO_GATEWAY_ADAPTER,
    &kilocode::KILOCODE_ADAPTER,
    &super::codex::OPENAI_CODEX_ADAPTER,
    &CLINE_ADAPTER,
    &CLINEPASS_ADAPTER,
    &command_code::COMMAND_CODE_ADAPTER,
    &muse::MUSE_ADAPTER,
    &opencode::OPENCODE_GO_ADAPTER,
    &opencode::OPENCODE_ZEN_ADAPTER,
    &openrouter::OPENROUTER_ADAPTER,
    &nvidia_nim::NVIDIA_NIM_ADAPTER,
    &freebuff::FREEBUFF_ADAPTER,
    &antigravity::ANTIGRAVITY_ADAPTER,
    &qoder::QODER_ADAPTER,
    &qoder::QODER_CN_ADAPTER,
    &compat::MOONSHOT_ADAPTER,
    &compat::ZHIPU_ADAPTER,
    &compat::MINIMAX_ADAPTER,
    &compat::SAMBANOVA_ADAPTER,
    &compat::HYPERBOLIC_ADAPTER,
    &compat::DEEPINFRA_ADAPTER,
    &compat::NOVITA_ADAPTER,
    &compat::GITHUB_MODELS_ADAPTER,
    &compat::OLLAMA_ADAPTER,
    &compat::LM_STUDIO_ADAPTER,
    &compat::VLLM_ADAPTER,
    &azure::AZURE_ADAPTER,
    &vertex::VERTEX_ADAPTER,
];

pub(super) fn adapter(adapter_id: &str) -> Option<&'static dyn ProviderAdapter> {
    ADAPTERS
        .iter()
        .copied()
        .find(|adapter| adapter.adapter_id() == adapter_id)
}

pub(super) fn adapters() -> &'static [&'static dyn ProviderAdapter] {
    &ADAPTERS
}

/// Whether an adapter's upstream is a local process transport instead of an
/// HTTP endpoint. Local transports never reach egress: target preparation
/// skips URL validation and the send step owns the child-process lifecycle.
pub fn uses_local_transport(adapter_id: &str) -> bool {
    adapter_id == DEVIN_CLI_ADAPTER_ID
}

/// Placeholder HTTP client for local-transport attempts. It is never used for
/// I/O — the send path spawns the provider's own process — but keeps the
/// attempt context shape so classification and streaming treat a local reply
/// exactly like an HTTP reply.
pub(crate) fn local_transport_client() -> reqwest::Client {
    static PLACEHOLDER: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    PLACEHOLDER.get_or_init(reqwest::Client::new).clone()
}

/// Sends one request through an adapter's local process transport and returns
/// a synthesized HTTP-shaped response (SSE or JSON by client streaming mode).
/// Preflight failures — missing CLI, handshake refusal, concurrency cap —
/// return an error before any response exists.
pub async fn send_local_upstream_request(
    adapter_id: &str,
    upstream_body: &Value,
    model: &str,
    request_id: &str,
    streaming: bool,
    request_timeout: std::time::Duration,
) -> Result<reqwest::Response, String> {
    match adapter_id {
        DEVIN_CLI_ADAPTER_ID => {
            devin_cli::send_local_upstream(
                upstream_body,
                model,
                request_id,
                streaming,
                request_timeout,
            )
            .await
        }
        other => Err(format!(
            "adapter '{other}' does not provide a local transport"
        )),
    }
}
