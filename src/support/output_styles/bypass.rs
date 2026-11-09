//! Bypass classifier: decides when enabled styles must not be injected.
//!
//! Pure function over the bounded tail of canonical messages. Scans at most
//! three trailing messages and 24 KiB total so classification cost stays
//! flat regardless of request size.

use crate::protocol::{ContentBlock, Message};

const PER_MESSAGE_LIMIT: usize = 8 * 1024;
const TOTAL_LIMIT: usize = 24 * 1024;
const TAIL_MESSAGE_COUNT: usize = 3;
const SEQUENCE_LOOKAHEAD_BYTES: usize = 240;

const SECURITY_TERMS: &[&str] = &[
    "security",
    "vulnerability",
    "exploit",
    "credential leak",
    "secret leak",
    "malware",
    "phishing",
];

const IRREVERSIBLE_TERMS: &[&str] = &[
    "delete",
    "drop table",
    "truncate",
    "destroy",
    "wipe",
    "irreversible",
    "permanently remove",
];

const CLARIFICATION_TERMS: &[&str] = &[
    "clarify",
    "explain in detail",
    "more detail",
    "step by step",
    "why exactly",
    "what do you mean",
];

const SEQUENCE_ORDERS: &[&str] = &[
    "first",
    "then",
    "after that",
    "before",
    "rollback",
    "backup",
];

const SEQUENCE_TERMS: &[&str] = &["delete", "drop", "migrate", "deploy", "release"];

pub fn should_bypass(messages: &[Message]) -> Option<&'static str> {
    let text = collect_bypass_text(messages).to_ascii_lowercase();
    if text.trim().is_empty() {
        return None;
    }
    if SECURITY_TERMS.iter().any(|term| contains_word(&text, term)) {
        return Some("security_warning");
    }
    if IRREVERSIBLE_TERMS
        .iter()
        .any(|term| contains_word(&text, term))
    {
        return Some("irreversible_action");
    }
    if CLARIFICATION_TERMS
        .iter()
        .any(|term| contains_word(&text, term))
    {
        return Some("clarification_requested");
    }
    if has_order_sensitive_sequence(&text) {
        return Some("order_sensitive_sequence");
    }
    None
}

/// Collects text from the bounded tail of the conversation. Tool results are
/// walked recursively so their nested content participates in detection.
fn collect_bypass_text(messages: &[Message]) -> String {
    let mut text = String::new();
    for message in messages.iter().rev().take(TAIL_MESSAGE_COUNT).rev() {
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
    text
}

/// Detects an ordering word followed within a bounded window by an
/// action word, which indicates step-sensitive instructions.
fn has_order_sensitive_sequence(text: &str) -> bool {
    for order in SEQUENCE_ORDERS {
        let mut start = 0;
        while let Some(relative) = text[start..].find(order) {
            let position = start + relative;
            if !is_word_boundary(text, position, order.len()) {
                start = position.saturating_add(order.len());
                if start >= text.len() {
                    break;
                }
                continue;
            }
            let mut end = position
                .saturating_add(order.len())
                .saturating_add(SEQUENCE_LOOKAHEAD_BYTES)
                .min(text.len());
            while end > position && !text.is_char_boundary(end) {
                end -= 1;
            }
            if SEQUENCE_TERMS
                .iter()
                .any(|term| contains_word(&text[position..end], term))
            {
                return true;
            }
            start = position.saturating_add(order.len());
            if start >= text.len() {
                break;
            }
        }
    }
    false
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
