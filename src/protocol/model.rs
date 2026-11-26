use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    #[default]
    ChatCompletions,
    Responses,
    Messages,
}

impl Protocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Responses => "responses",
            Self::Messages => "messages",
        }
    }
}

impl std::str::FromStr for Protocol {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "chat" | "chat_completions" => Ok(Self::ChatCompletions),
            "responses" => Ok(Self::Responses),
            "messages" | "anthropic_messages" => Ok(Self::Messages),
            _ => Err(format!("unsupported protocol: {value}")),
        }
    }
}

/// Wire protocol selected for a provider model. This stays separate from the
/// three public client APIs so provider-only formats never become public routes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamProtocol {
    #[default]
    ChatCompletions,
    Responses,
    Messages,
    GoogleGenerateContent,
}

impl UpstreamProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Responses => "responses",
            Self::Messages => "messages",
            Self::GoogleGenerateContent => "google_generate_content",
        }
    }

    #[allow(dead_code)]
    pub fn all() -> &'static [Self] {
        &[
            Self::ChatCompletions,
            Self::Responses,
            Self::Messages,
            Self::GoogleGenerateContent,
        ]
    }

    #[allow(dead_code)]
    pub fn as_str_list() -> &'static [&'static str] {
        &[
            "chat_completions",
            "responses",
            "messages",
            "google_generate_content",
        ]
    }

    pub fn client_protocol(self) -> Option<Protocol> {
        match self {
            Self::ChatCompletions => Some(Protocol::ChatCompletions),
            Self::Responses => Some(Protocol::Responses),
            Self::Messages => Some(Protocol::Messages),
            Self::GoogleGenerateContent => None,
        }
    }
}

impl From<Protocol> for UpstreamProtocol {
    fn from(protocol: Protocol) -> Self {
        match protocol {
            Protocol::ChatCompletions => Self::ChatCompletions,
            Protocol::Responses => Self::Responses,
            Protocol::Messages => Self::Messages,
        }
    }
}

impl std::str::FromStr for UpstreamProtocol {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "chat" | "chat_completions" => Ok(Self::ChatCompletions),
            "responses" => Ok(Self::Responses),
            "messages" | "anthropic_messages" => Ok(Self::Messages),
            "google_generate_content" => Ok(Self::GoogleGenerateContent),
            _ => Err(format!("unsupported upstream protocol: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    Developer,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Developer => "developer",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
        #[serde(default, flatten)]
        extra: BTreeMap<String, Value>,
    },
    RedactedThinking {
        data: String,
        #[serde(default, flatten)]
        extra: BTreeMap<String, Value>,
    },
    Image {
        url: String,
        detail: Option<String>,
    },
    Document {
        source: DocumentSource,
        filename: Option<String>,
    },
    GeneratedImage {
        data: String,
        media_type: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },
    ToolResult {
        tool_call_id: String,
        content: Vec<ContentBlock>,
    },
    Reasoning {
        text: String,
    },
}

/// Provider-specific handling for Anthropic thinking blocks in request history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThinkingHandling<'a> {
    Preserve,
    Override(&'a str),
    Remove,
}

pub const DEFAULT_THINKING_MODE: &str = "preserve";
pub const MAX_THINKING_OVERRIDE_BYTES: usize = 4096;

pub fn parse_thinking_handling<'a>(
    mode: &str,
    override_text: Option<&'a str>,
) -> Result<ThinkingHandling<'a>, &'static str> {
    if override_text.is_some_and(|text| {
        text.len() > MAX_THINKING_OVERRIDE_BYTES || text.as_bytes().contains(&0)
    }) {
        return Err("thinking replacement exceeds the 4096-byte limit or contains invalid data");
    }
    match mode {
        "preserve" if override_text.is_none() => Ok(ThinkingHandling::Preserve),
        "remove" if override_text.is_none() => Ok(ThinkingHandling::Remove),
        "override" => match override_text {
            Some(text) if !text.trim().is_empty() => Ok(ThinkingHandling::Override(text)),
            _ => Err("thinking override text is required"),
        },
        "preserve" | "remove" => Err("thinking override text is only valid in override mode"),
        _ => Err("thinking mode must be preserve, override or remove"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DocumentSource {
    Url { url: String },
    Base64 { media_type: String, data: String },
    Text { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalRequest {
    #[serde(default)]
    pub source_protocol: Protocol,
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub tools: Vec<Tool>,
    #[serde(default)]
    pub tool_choice: Option<Value>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub stop: Vec<String>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
    /// Internal request-local guard preventing duplicate style instructions.
    #[serde(skip)]
    pub(crate) output_styles_applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cached_tokens: u64,
    pub cost_micro_usd: Option<i64>,
    #[serde(skip)]
    pub(crate) cache_input_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalResponse {
    pub id: String,
    pub model: String,
    pub message: Message,
    pub finish_reason: String,
    #[serde(default)]
    pub usage: Option<Usage>,
}
