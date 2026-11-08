use super::{
    CLINE_ADAPTER_ID, CLINEPASS_ADAPTER_ID, DEVIN_CLI_ADAPTER_ID, DEVIN_DESKTOP_ADAPTER_ID,
    KIRO_ADAPTER_ID, OAuthRequestAuth, ProviderAdapter, antigravity, azure, cloud_api,
    command_code, compat, devin_cli, devin_desktop, freebuff, generic, kilo, kilocode, kiro, muse,
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

static ADAPTERS: [&'static dyn ProviderAdapter; 44] = [
    &generic::GENERIC_ADAPTER,
    &devin_cli::DEVIN_CLI_ADAPTER,
    &devin_desktop::DEVIN_DESKTOP_ADAPTER,
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
    &kiro::KIRO_ADAPTER,
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

/// Whether an adapter owns its whole transport instead of going through the
/// gateway's shared HTTP send path. Adapter-owned transports build their own
/// egress-validated clients where they use HTTP (Devin Desktop, Kiro's binary
/// EventStream upstream) or spawn a local process (Devin CLI); the send step
/// only consumes the synthesized response these adapters return.
pub fn uses_local_transport(adapter_id: &str) -> bool {
    adapter_id == DEVIN_CLI_ADAPTER_ID
        || adapter_id == DEVIN_DESKTOP_ADAPTER_ID
        || adapter_id == KIRO_ADAPTER_ID
}

/// Placeholder HTTP client for local-transport attempts. It is never used for
/// I/O — the send path spawns the provider's own process — but keeps the
/// attempt context shape so classification and streaming treat a local reply
/// exactly like an HTTP reply.
pub(crate) fn local_transport_client() -> reqwest::Client {
    static PLACEHOLDER: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    PLACEHOLDER.get_or_init(reqwest::Client::new).clone()
}

/// Sends one request through an adapter-owned transport and returns a
/// synthesized HTTP-shaped response (SSE or JSON by client streaming mode).
/// Preflight failures — missing CLI, rejected key, egress refusal — return an
/// error before any response exists.
#[allow(clippy::too_many_arguments)]
pub async fn send_local_upstream_request(
    state: &crate::state::AppState,
    adapter_id: &str,
    upstream_body: &Value,
    model: &str,
    request_id: &str,
    streaming: bool,
    request_timeout: std::time::Duration,
    credential_secret: Option<&str>,
    oauth_auth: Option<&OAuthRequestAuth>,
) -> Result<reqwest::Response, String> {
    match adapter_id {
        DEVIN_CLI_ADAPTER_ID => {
            let _ = oauth_auth;
            devin_cli::send_local_upstream(
                state,
                upstream_body,
                model,
                request_id,
                streaming,
                request_timeout,
                credential_secret,
            )
            .await
        }
        DEVIN_DESKTOP_ADAPTER_ID => {
            let _ = oauth_auth;
            devin_desktop::send_local_upstream(
                state,
                upstream_body,
                model,
                request_id,
                streaming,
                request_timeout,
                credential_secret,
            )
            .await
        }
        KIRO_ADAPTER_ID => {
            kiro::transport::send_local_upstream(
                state,
                upstream_body,
                model,
                request_id,
                streaming,
                request_timeout,
                oauth_auth,
            )
            .await
        }
        other => Err(format!(
            "adapter '{other}' does not provide an adapter-owned transport"
        )),
    }
}
