use super::{
    CLINE_ADAPTER_ID, CLINEPASS_ADAPTER_ID, ProviderAdapter, antigravity, command_code, freebuff,
    generic, kilo, kilocode, muse, nvidia_nim, opencode, openrouter, qoder,
};

pub(crate) struct ClineAdapter {
    pub(crate) adapter_id: &'static str,
}

static CLINE_ADAPTER: ClineAdapter = ClineAdapter {
    adapter_id: CLINE_ADAPTER_ID,
};

static CLINEPASS_ADAPTER: ClineAdapter = ClineAdapter {
    adapter_id: CLINEPASS_ADAPTER_ID,
};

static ADAPTERS: [&'static dyn ProviderAdapter; 16] = [
    &generic::GENERIC_ADAPTER,
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
