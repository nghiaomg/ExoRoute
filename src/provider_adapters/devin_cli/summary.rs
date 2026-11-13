//! The summarizer agent wraps its turn report in `<summary>…</summary>`
//! envelope markup. Left alone the tags stream into the chat as literal text —
//! the closing `</summary>` visibly leaks into the dashboard chat frame and
//! every other Chat Completions client.
//!
//! The envelope wraps the *whole turn*: the opener is the first non-whitespace
//! output and the closer ends the envelope. The strippers here remove exactly
//! that pair and deliberately never touch a `<summary>` sequence inside the
//! answer body, where it can be legitimate content (an HTML example, a code
//! block). A turn that does not open with the envelope passes through
//! untouched, and one bridge turn carries one envelope, matching the
//! tool-envelope contract in [`super::tool_envelope`].

const OPEN_TAG: &str = "<summary>";
const CLOSE_TAG: &str = "</summary>";

/// Removes a whole-turn `<summary>…</summary>` envelope from a completed turn
/// while keeping the report body. A turn without the opener is returned
/// byte-identical; a turn truncated before the closer keeps the content that
/// arrived, minus the opener.
pub(super) fn strip_summary_envelope(text: &str) -> String {
    let Some(after_open) = text.trim_start().strip_prefix(OPEN_TAG) else {
        return text.to_owned();
    };
    match after_open.find(CLOSE_TAG) {
        Some(at) => format!(
            "{}{}",
            &after_open[..at],
            &after_open[at + CLOSE_TAG.len()..]
        )
        .trim()
        .to_owned(),
        None => after_open.trim().to_owned(),
    }
}

enum Phase {
    /// Collecting the turn opening until the opener matches or the output
    /// diverges from it. The held text is emitted unchanged on divergence.
    Head(String),
    /// Opener consumed; the body streams through while a possible partial
    /// closer (at most [`CLOSE_TAG`] minus one character) is held back.
    Body(String),
    /// The envelope is absent or fully consumed: pure passthrough.
    Done,
}

/// Streaming counterpart of [`strip_summary_envelope`]: strips the envelope
/// from per-chunk deltas, tolerating tags split across chunk boundaries.
/// Holds back at most one partial tag so the streaming feel is preserved.
pub(super) struct SummaryStripper {
    phase: Phase,
}

impl SummaryStripper {
    pub(super) fn new() -> Self {
        Self {
            phase: Phase::Head(String::new()),
        }
    }

    /// Consumes one delta and returns the text to emit now (possibly empty).
    pub(super) fn push(&mut self, delta: &str) -> String {
        match std::mem::replace(&mut self.phase, Phase::Done) {
            Phase::Head(mut held) => {
                held.push_str(delta);
                let trimmed = held.trim_start();
                if trimmed.is_empty() || OPEN_TAG.starts_with(trimmed) {
                    // Still deciding: pure whitespace, or a prefix of the opener.
                    self.phase = Phase::Head(held);
                    String::new()
                } else if let Some(rest) = trimmed.strip_prefix(OPEN_TAG) {
                    self.phase = Phase::Body(String::new());
                    self.push_body(rest)
                } else {
                    // No envelope: the whole turn passes through untouched.
                    self.phase = Phase::Done;
                    held
                }
            }
            Phase::Body(held) => {
                self.phase = Phase::Body(held);
                self.push_body(delta)
            }
            Phase::Done => {
                self.phase = Phase::Done;
                delta.to_owned()
            }
        }
    }

    fn push_body(&mut self, delta: &str) -> String {
        let Phase::Body(held) = std::mem::replace(&mut self.phase, Phase::Done) else {
            return delta.to_owned();
        };
        let mut buf = held;
        buf.push_str(delta);
        let Some(at) = buf.find(CLOSE_TAG) else {
            // Hold back a trailing fragment that may be a partial closer; it
            // either completes on a later delta or passes through at finish.
            let hold = (1..CLOSE_TAG.len())
                .rev()
                .find(|len| buf.ends_with(&CLOSE_TAG[..*len]))
                .unwrap_or(0);
            self.phase = Phase::Body(buf[buf.len() - hold..].to_owned());
            let emit_len = buf.len() - hold;
            return buf[..emit_len].to_owned();
        };
        // The closer ends the envelope; the body before it and any output the
        // model emitted after it both stream through.
        let body = buf[..at].to_owned();
        let after = buf[at + CLOSE_TAG.len()..].to_owned();
        self.phase = Phase::Done;
        format!("{body}{after}")
    }

    /// Flushes the held text at turn end. A partial tag the turn never
    /// completed passes through as the literal content it is.
    pub(super) fn finish(&mut self) -> String {
        match std::mem::replace(&mut self.phase, Phase::Done) {
            Phase::Head(held) | Phase::Body(held) => held,
            Phase::Done => String::new(),
        }
    }
}

impl Default for SummaryStripper {
    fn default() -> Self {
        Self::new()
    }
}
