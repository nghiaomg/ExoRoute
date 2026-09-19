//! Injection applier plus bounded Prometheus counters.
//!
//! `apply` mutates the canonical request in place at most once; `Metrics`
//! only counts fixed style/reason labels and never retains request text.

use super::catalog::{MAX_STYLE_SELECTIONS, OutputStyleId, OutputStyleSelection};
use crate::protocol::{CanonicalRequest, ContentBlock, Message, Role};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyResult {
    Disabled,
    AlreadyApplied,
    NoMessages,
    Bypassed(&'static str),
    Applied([Option<OutputStyleId>; MAX_STYLE_SELECTIONS]),
}

/// Bounded counters used by the admin Prometheus endpoint. Labels are fixed to
/// the catalog and bypass reasons; no request text is retained.
pub struct Metrics {
    pub injected: [AtomicU64; MAX_STYLE_SELECTIONS],
    pub bypassed: [AtomicU64; 4],
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            injected: [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)],
            bypassed: [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ],
        }
    }

    pub fn record(&self, result: ApplyResult) {
        match result {
            ApplyResult::Applied(ids) => {
                for id in ids.into_iter().flatten() {
                    saturating_increment(&self.injected[id.index()]);
                }
            }
            ApplyResult::Bypassed(reason) => {
                let index = match reason {
                    "security_warning" => 0,
                    "irreversible_action" => 1,
                    "clarification_requested" => 2,
                    "order_sensitive_sequence" => 3,
                    _ => return,
                };
                saturating_increment(&self.bypassed[index]);
            }
            _ => {}
        }
    }
}

fn saturating_increment(value: &AtomicU64) {
    let _ = value.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.saturating_add(1))
    });
}

pub fn apply(
    request: &mut CanonicalRequest,
    styles: &[OutputStyleSelection],
) -> Result<ApplyResult, String> {
    if request.output_styles_applied {
        return Ok(ApplyResult::AlreadyApplied);
    }
    let styles = super::catalog::validate_styles(styles)?;
    if styles.is_empty() {
        return Ok(ApplyResult::Disabled);
    }
    if request.messages.is_empty() {
        return Ok(ApplyResult::NoMessages);
    }
    if let Some(reason) = super::bypass::should_bypass(&request.messages) {
        return Ok(ApplyResult::Bypassed(reason));
    }
    let instruction = super::instruction::build_instruction(&styles)?;
    let mut ids = [None; MAX_STYLE_SELECTIONS];
    for style in &styles {
        ids[style.id.index()] = Some(style.id);
    }
    let last_system_or_developer = request
        .messages
        .iter()
        .rposition(|message| matches!(message.role, Role::System | Role::Developer));
    let insertion = last_system_or_developer.map_or_else(
        || {
            request
                .messages
                .iter()
                .position(|message| matches!(message.role, Role::User))
                .unwrap_or(0)
        },
        |index| index.saturating_add(1),
    );
    let messages = std::mem::take(&mut request.messages);
    let mut next = Vec::with_capacity(messages.len().saturating_add(1));
    for (index, message) in messages.into_iter().enumerate() {
        if index == insertion {
            next.push(Message {
                role: Role::System,
                content: vec![ContentBlock::Text {
                    text: instruction.clone(),
                }],
                name: None,
            });
        }
        next.push(message);
    }
    if insertion >= next.len() {
        next.push(Message {
            role: Role::System,
            content: vec![ContentBlock::Text { text: instruction }],
            name: None,
        });
    }
    request.messages = next;
    request.output_styles_applied = true;
    Ok(ApplyResult::Applied(ids))
}
