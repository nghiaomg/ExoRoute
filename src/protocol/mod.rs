//! Canonical request/response model and protocol codecs.
//!
//! Client request decoding and provider request encoding are separate from
//! provider response decoding/encoding. Provider-only codecs such as Google
//! Generate Content stay under their protocol module instead of leaking into
//! the gateway orchestration layer.

use serde_json::{Value, json};
use std::collections::BTreeMap;

mod google;
mod model;
mod parse;
mod request_decoding;
mod request_encode;
mod request_metadata;
mod response_decoding;
mod response_encode;
mod thinking;
#[cfg(test)]
use model::MAX_THINKING_OVERRIDE_BYTES;
pub use model::{
    CanonicalRequest, CanonicalResponse, ContentBlock, DEFAULT_THINKING_MODE, DocumentSource,
    Message, Protocol, Role, StreamEvent, ThinkingHandling, Tool, UpstreamProtocol, Usage,
    parse_thinking_handling,
};

// The canonical types are the protocol's public surface. The remaining helpers
// live in the submodules above and stay crate-visible here so the gateway, the
// decoders, and the tests all resolve them through one path.
pub use parse::parse_cost_micro_usd;
pub(crate) use parse::{
    cache_input_token_total, optional_bool, optional_f32, optional_u32, parse_cached_input_tokens,
    parse_chat_tools, parse_content, parse_messages_tools, parse_response_tools, parse_role,
    parse_string_list, parse_usage, string_field,
};
pub(crate) use request_encode::encode_openai_family_request;
pub use response_encode::encode_response;
#[cfg(test)]
pub(crate) use thinking::INTERNAL_THINKING_BLOCK_TYPE;
pub(crate) use thinking::{
    contains_thinking_block, finalize_thinking_payload, internal_thinking_block,
    parse_reasoning_content, parse_responses_reasoning_item,
};
pub fn decode_request(protocol: Protocol, body: &Value) -> Result<CanonicalRequest, String> {
    request_decoding::decode_request(protocol, body)
}

#[cfg(test)]
pub fn encode_request(
    protocol: Protocol,
    request: &CanonicalRequest,
    model: &str,
) -> Result<Value, String> {
    encode_upstream_request(protocol.into(), request, model)
}

#[cfg(test)]
pub fn encode_upstream_request(
    protocol: UpstreamProtocol,
    request: &CanonicalRequest,
    model: &str,
) -> Result<Value, String> {
    encode_upstream_request_with_thinking(protocol, request, model, ThinkingHandling::Preserve)
}

pub fn encode_upstream_request_with_thinking(
    protocol: UpstreamProtocol,
    request: &CanonicalRequest,
    model: &str,
    thinking_handling: ThinkingHandling<'_>,
) -> Result<Value, String> {
    if protocol == UpstreamProtocol::Messages
        && request.messages.iter().any(|message| {
            !matches!(message.role, Role::Assistant)
                && message.content.iter().any(|block| {
                    matches!(
                        block,
                        ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. }
                    )
                })
        })
    {
        return Err("Anthropic thinking blocks are only valid in assistant messages".to_owned());
    }
    if protocol != UpstreamProtocol::Messages
        && matches!(thinking_handling, ThinkingHandling::Preserve)
        && request.messages.iter().any(|message| {
            message
                .content
                .iter()
                .any(|block| matches!(block, ContentBlock::RedactedThinking { .. }))
        })
    {
        return Err(
            "redacted thinking cannot be represented by this provider protocol; choose override or remove"
                .to_owned(),
        );
    }
    let mut encoded = match protocol {
        UpstreamProtocol::GoogleGenerateContent => google::encode_request(request, model)?,
        protocol => encode_openai_family_request(
            protocol
                .client_protocol()
                .ok_or("upstream protocol cannot encode this request")?,
            request,
            model,
        )?,
    };
    if request
        .messages
        .iter()
        .any(|message| message.content.iter().any(contains_thinking_block))
    {
        finalize_thinking_payload(&mut encoded, protocol, thinking_handling)?;
    }
    Ok(encoded)
}

#[cfg(test)]
pub fn decode_response(
    protocol: Protocol,
    value: &Value,
    requested_model: &str,
) -> Result<CanonicalResponse, String> {
    response_decoding::decode_response(protocol, value, requested_model)
}

pub fn decode_upstream_response(
    protocol: UpstreamProtocol,
    value: &Value,
    requested_model: &str,
) -> Result<CanonicalResponse, String> {
    response_decoding::decode_upstream_response(protocol, value, requested_model)
}

/// Returns whether a decoded assistant response contains output that can be
/// represented to a client. A terminal status alone is not enough: providers
/// can send an empty completed response, and a tool-call finish reason can be
/// present even when the tool-call payload was lost.
#[cfg(test)]
pub fn response_has_output(response: &CanonicalResponse) -> bool {
    response_decoding::response_has_output(response)
}

#[cfg(test)]
mod tests;
