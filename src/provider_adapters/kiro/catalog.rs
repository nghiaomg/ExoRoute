//! Kiro's built-in model list. The CodeWhisperer runtime does not expose a
//! catalog endpoint, so the preset ships the models Kiro is known to serve.
pub(super) fn default_models() -> Vec<String> {
    ["claude-sonnet-4.5", "claude-haiku-4.5"]
        .iter()
        .map(|model| (*model).to_owned())
        .collect()
}
