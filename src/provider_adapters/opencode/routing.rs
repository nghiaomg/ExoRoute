use super::{OPENCODE_GO_ADAPTER_ID, OPENCODE_ZEN_ADAPTER_ID};
use crate::protocol::UpstreamProtocol;
pub(super) fn open_code_root(base_url: &str) -> Result<(reqwest::Url, String), String> {
    let url =
        reqwest::Url::parse(base_url).map_err(|_| "OpenCode provider URL is invalid".to_owned())?;
    let path = url.path().trim_end_matches('/');
    let root = path
        .strip_suffix("/chat/completions")
        .or_else(|| path.strip_suffix("/responses"))
        .or_else(|| path.strip_suffix("/messages"))
        .or_else(|| path.strip_suffix("/models"))
        .unwrap_or(path);
    let root = root.trim_end_matches('/').to_owned();
    Ok((url, root))
}

pub(super) fn open_code_aux_endpoint(
    base_url: &str,
    endpoint: &str,
) -> Result<reqwest::Url, String> {
    let (mut url, root) = open_code_root(base_url)?;
    url.set_path(&format!("{root}/{endpoint}"));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

pub(super) fn open_code_endpoint(
    base_url: &str,
    protocol: UpstreamProtocol,
) -> Result<reqwest::Url, String> {
    let suffix = match protocol {
        UpstreamProtocol::ChatCompletions => "chat/completions",
        UpstreamProtocol::Responses => "responses",
        UpstreamProtocol::Messages => "messages",
        UpstreamProtocol::GoogleGenerateContent => {
            return Err("Google Generate Content requires an OpenCode Zen model ID".to_owned());
        }
    };
    let (mut url, root) = open_code_root(base_url)?;
    url.set_path(&format!("{root}/{suffix}"));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

pub(super) fn google_generate_content_endpoint(
    base_url: &str,
    model: &str,
    streaming: bool,
) -> Result<reqwest::Url, String> {
    if model.is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
        return Err("OpenCode Zen model ID is invalid".to_owned());
    }
    let (mut url, root) = open_code_root(base_url)?;
    url.set_path(&format!("{root}/models"));
    url.path_segments_mut()
        .map_err(|_| "OpenCode Zen model URL cannot be constructed".to_owned())?
        .push(model);
    let path = format!(
        "{}:{}",
        url.path().trim_end_matches('/'),
        if streaming {
            "streamGenerateContent"
        } else {
            "generateContent"
        }
    );
    url.set_path(&path);
    if streaming {
        url.query_pairs_mut().append_pair("alt", "sse");
    } else {
        url.set_query(None);
    }
    Ok(url)
}

pub(crate) fn model_protocol(adapter_id: &str, model: &str) -> Option<UpstreamProtocol> {
    match adapter_id {
        OPENCODE_GO_ADAPTER_ID => go_model_protocol(model),
        OPENCODE_ZEN_ADAPTER_ID => zen_model_protocol(model),
        _ => None,
    }
}

fn go_model_protocol(model: &str) -> Option<UpstreamProtocol> {
    use UpstreamProtocol::{ChatCompletions, Messages, Responses};
    let protocol = match model {
        "grok-4.6"
        | "gpt-5.6-luna"
        | "muse-spark-1.3-contributor"
        | "muse-spark-1.2-contributor" => Responses,
        "minimax-m3" | "minimax-m2.7" | "minimax-m2.5" | "qwen3.8-max" | "qwen3.8-flash"
        | "qwen3.7-max" | "qwen3.7-plus" | "qwen3.6-plus" => Messages,
        "glm-5.3-flash"
        | "glm-5.3"
        | "glm-5.2"
        | "glm-5.1"
        | "kimi-k3"
        | "kimi-k2.7-code"
        | "kimi-k2.6"
        | "longcat-2.0"
        | "deepseek-v4.1-flash"
        | "deepseek-v4-pro"
        | "deepseek-v4-flash"
        | "deepseek-v4-flash-vision-exp"
        | "mimo-v2.5"
        | "mimo-v2.5-pro"
        | "hy4-preview"
        | "hy3" => ChatCompletions,
        _ => return None,
    };
    Some(protocol)
}

fn zen_model_protocol(model: &str) -> Option<UpstreamProtocol> {
    use UpstreamProtocol::{ChatCompletions, GoogleGenerateContent, Messages, Responses};
    let protocol = match model {
        "gpt-6-astra"
        | "gpt-5.6-sol"
        | "gpt-5.6-terra"
        | "gpt-5.6-luna"
        | "gpt-5.5"
        | "gpt-5.5-pro"
        | "gpt-5.4"
        | "gpt-5.4-pro"
        | "gpt-5.4-mini"
        | "gpt-5.4-nano"
        | "gpt-5.3-codex"
        | "gpt-5.3-codex-spark"
        | "gpt-5.2"
        | "gpt-5.2-codex"
        | "gpt-5.1"
        | "gpt-5.1-codex"
        | "gpt-5.1-codex-max"
        | "gpt-5.1-codex-mini"
        | "gpt-5"
        | "gpt-5-codex"
        | "gpt-5-nano"
        | "grok-4.6"
        | "grok-4.5"
        | "grok-build-0.1"
        | "muse-spark-1.3"
        | "muse-spark-1.2"
        | "muse-spark-1.3-contributor-free" => Responses,
        "claude-fable-5-1" | "claude-fable-5" | "claude-opus-5" | "claude-opus-4-8"
        | "claude-opus-4-7" | "claude-opus-4-6" | "claude-opus-4-5" | "claude-sonnet-5"
        | "claude-sonnet-4-6" | "claude-sonnet-4-5" | "claude-haiku-4-5" | "qwen3.7-max"
        | "qwen3.7-plus" | "qwen3.6-plus" | "qwen3.5-plus" => Messages,
        "gemini-3.8-flash"
        | "gemini-3.7-flash"
        | "gemini-3.6-flash"
        | "gemini-3.5-flash"
        | "gemini-3.5-flash-lite"
        | "gemini-3.1-pro"
        | "gemini-3-flash" => GoogleGenerateContent,
        "deepseek-v4-pro"
        | "deepseek-v4-flash"
        | "deepseek-v4-flash-vision-exp"
        | "minimax-m3"
        | "minimax-m2.7"
        | "minimax-m2.5"
        | "glm-5.3-flash"
        | "glm-5.3"
        | "glm-5.2"
        | "glm-5.1"
        | "glm-5"
        | "kimi-k2.5"
        | "kimi-k2.6"
        | "kimi-k2.7-code"
        | "kimi-k3"
        | "big-pickle"
        | "mimo-v2.5-free"
        | "ling-3.0-flash-fin-free"
        | "nemotron-3-ultra-free"
        | "nemotron-3.5-lightning-free" => ChatCompletions,
        _ => return None,
    };
    Some(protocol)
}
