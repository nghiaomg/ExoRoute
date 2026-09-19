//! Bypass classifier: decides when enabled styles must not be injected.
//!
//! Pure function over the bounded tail of canonical messages. Scans at most
//! three trailing messages and 24 KiB total so classification cost stays
//! flat regardless of request size.

use crate::protocol::{ContentBlock, Message};

pub fn should_bypass(messages: &[Message]) -> Option<&'static str> {
    const PER_MESSAGE_LIMIT: usize = 8 * 1024;
    const TOTAL_LIMIT: usize = 24 * 1024;
    let mut text = String::new();
    for message in messages.iter().rev().take(3).rev() {
        let mut message_bytes = 0usize;
        if text.len() >= TOTAL_LIMIT {
            break;
        }
        let mut blocks = vec![message.content.iter()];
        while message_bytes < PER_MESSAGE_LIMIT && text.len() < TOTAL_LIMIT {
            let Some(block) = blocks.last_mut().and_then(|remaining| remaining.next()) else {
                if blocks.pop().is_none() {
                    break;
                }
                continue;
            };
            let block_text = match block {
                ContentBlock::Text { text } | ContentBlock::Reasoning { text } => Some(text),
                ContentBlock::Thinking { thinking, .. } => Some(thinking),
                ContentBlock::Document {
                    source: crate::protocol::DocumentSource::Text { text },
                    ..
                } => Some(text),
                ContentBlock::ToolResult { content, .. } => {
                    blocks.push(content.iter());
                    None
                }
                _ => None,
            };
            let Some(block_text) = block_text else {
                continue;
            };
            for character in block_text.chars() {
                if character.len_utf8() > PER_MESSAGE_LIMIT.saturating_sub(message_bytes)
                    || character.len_utf8() > TOTAL_LIMIT.saturating_sub(text.len())
                {
                    break;
                }
                text.push(character);
                message_bytes += character.len_utf8();
            }
            if text.len() < TOTAL_LIMIT {
                text.push('\n');
            }
            if message_bytes >= PER_MESSAGE_LIMIT {
                break;
            }
        }
    }
    // Detection terms are ASCII. Keep the bounded scan bounded in bytes even
    // for Unicode input instead of allowing case folding to expand text.
    let text = text.to_ascii_lowercase();
    if text.trim().is_empty() {
        return None;
    }
    if [
        "security",
        "vulnerability",
        "exploit",
        "credential leak",
        "secret leak",
        "malware",
        "phishing",
    ]
    .iter()
    .any(|term| contains_word(&text, term))
    {
        return Some("security_warning");
    }
    if [
        "delete",
        "drop table",
        "truncate",
        "destroy",
        "wipe",
        "irreversible",
        "permanently remove",
    ]
    .iter()
    .any(|term| contains_word(&text, term))
    {
        return Some("irreversible_action");
    }
    if [
        "clarify",
        "explain in detail",
        "more detail",
        "step by step",
        "why exactly",
        "what do you mean",
    ]
    .iter()
    .any(|term| contains_word(&text, term))
    {
        return Some("clarification_requested");
    }
    for order in [
        "first",
        "then",
        "after that",
        "before",
        "rollback",
        "backup",
    ] {
        let mut start = 0;
        while let Some(relative) = text[start..].find(order) {
            let position = start + relative;
            if !is_word_boundary(&text, position, order.len()) {
                start = position.saturating_add(order.len());
                if start >= text.len() {
                    break;
                }
                continue;
            }
            let mut end = position
                .saturating_add(order.len())
                .saturating_add(240)
                .min(text.len());
            while end > position && !text.is_char_boundary(end) {
                end -= 1;
            }
            if ["delete", "drop", "migrate", "deploy", "release"]
                .iter()
                .any(|term| contains_word(&text[position..end], term))
            {
                return Some("order_sensitive_sequence");
            }
            start = position.saturating_add(order.len());
            if start >= text.len() {
                break;
            }
        }
    }
    None
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn is_word_boundary(text: &str, start: usize, length: usize) -> bool {
    let end = start.saturating_add(length);
    let bytes = text.as_bytes();
    (start == 0 || !is_word_byte(bytes[start - 1]))
        && (end >= bytes.len() || !is_word_byte(bytes[end]))
}

fn contains_word(text: &str, term: &str) -> bool {
    let mut offset = 0;
    while let Some(relative) = text[offset..].find(term) {
        let start = offset + relative;
        if is_word_boundary(text, start, term.len()) {
            return true;
        }
        offset = start.saturating_add(term.len());
        if offset >= text.len() {
            break;
        }
    }
    false
}
