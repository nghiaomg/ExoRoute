//! Router-side handling of the tool-call envelopes the Devin model sometimes
//! emits as plain assistant text (see docs/OmniRoute
//! `DEVIN_CLAUDE_BRIDGE.md`: the parser accepts one tool envelope per turn and
//! permits one bounded repair). The transport frames caller-side tools as
//! externally available and asks the model to answer in text only, but the
//! underlying model still occasionally wraps an action in its native envelope
//! (for example `<execute_bash> {...} </execute_bash>`), often with unescaped
//! quotes that make the JSON invalid. Left alone that markup leaks to the
//! client; this module either converts a valid envelope into a router-level
//! tool call or strips the markup so it can never reach the client.

use serde_json::Value;

/// One recovered client-owned tool call.
pub(super) struct TurnToolCall {
    pub name: String,
    /// JSON-encoded arguments string, as OpenAI `tool_calls` expects.
    pub arguments: String,
}

/// What the router should do with a completed CLI turn when the client
/// declared tools.
pub(super) enum ToolEnvelopeOutcome {
    /// The turn contained a parsable envelope naming a caller-side tool.
    ToolCall(TurnToolCall),
    /// Plain assistant text with every envelope markup stripped.
    Text(String),
}

/// Resolves the first tool envelope in `text` against the caller's tool list.
///
/// An envelope is a `<(tag)> {json} </(tag)>` block whose payload starts with
/// `{`. Recognized names: `<tool>` (the bridge format — the JSON carries a
/// `name` field) and any tag naming a caller-side tool (for example
/// `<execute_bash>`). The JSON gets one bounded repair pass because the model
/// frequently forgets to escape shell quotes inside string values.
pub(super) fn resolve_tool_envelope(text: &str, caller_tools: &[String]) -> ToolEnvelopeOutcome {
    let Some((tag, inner_range, _)) = find_first_envelope(text) else {
        return ToolEnvelopeOutcome::Text(text.to_owned());
    };
    let Some(value) = parse_lenient_json(text[inner_range.clone()].trim()) else {
        return ToolEnvelopeOutcome::Text(strip_envelopes(text));
    };

    let name = match tag.as_str() {
        "tool" => value.get("name").and_then(Value::as_str).map(str::to_owned),
        _ => Some(tag.clone()),
    };
    let Some(name) = name else {
        return ToolEnvelopeOutcome::Text(strip_envelopes(text));
    };
    let recognized = caller_tools
        .iter()
        .any(|tool| tool.eq_ignore_ascii_case(&name));
    if !recognized {
        return ToolEnvelopeOutcome::Text(strip_envelopes(text));
    }

    let arguments = match value.get("arguments") {
        Some(Value::String(raw)) => raw.clone(),
        Some(other) => other.to_string(),
        None => match value.as_object() {
            // The envelope body is the argument object itself; drop the
            // redundant name field so the schema stays clean.
            Some(object) => {
                let mut args = object.clone();
                args.remove("name");
                Value::Object(args).to_string()
            }
            None => Value::Object(Default::default()).to_string(),
        },
    };

    ToolEnvelopeOutcome::ToolCall(TurnToolCall { name, arguments })
}

/// Removes every `<(tag)> {json} </(tag)>` block from the text, then trims it.
fn strip_envelopes(text: &str) -> String {
    let mut cleaned = text.to_owned();
    while let Some((_, _, whole)) = find_first_envelope(&cleaned) {
        cleaned = format!("{}{}", &cleaned[..whole.start], &cleaned[whole.end..])
            .trim()
            .to_owned();
    }
    cleaned
}

/// Finds the earliest `<(tag)> {json} </(tag)>` block.
/// Returns `(tag, inner_range, whole_range)`.
fn find_first_envelope(
    text: &str,
) -> Option<(String, std::ops::Range<usize>, std::ops::Range<usize>)> {
    let mut search_from = 0usize;
    loop {
        let open_rel = text[search_from..].find('<')?;
        let open_at = search_from + open_rel;
        let after_open = &text[open_at + 1..];
        let tag_end = after_open.find('>')?;
        let tag = &after_open[..tag_end];
        if !is_tag_name(tag) {
            search_from = open_at + 1;
            continue;
        }
        let inner_start = open_at + 1 + tag_end + 1;
        let closer = format!("</{tag}>");
        let Some(close_rel) = text[inner_start..].find(&closer) else {
            search_from = open_at + 1;
            continue;
        };
        let inner_end = inner_start + close_rel;
        // A tool envelope carries a JSON payload; angle-bracket prose such as
        // `<b>bold</b>` must not match.
        if !text[inner_start..inner_end].trim_start().starts_with('{') {
            search_from = inner_end;
            continue;
        }
        let whole_end = inner_end + closer.len();
        return Some((tag.to_owned(), inner_start..inner_end, open_at..whole_end));
    }
}

fn is_tag_name(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

/// Parses `raw` as JSON; on failure applies one bounded repair: quotes that
/// do not sit at a structural boundary are treated as content and escaped,
/// so shell commands with unescaped `"` inside string values recover.
fn parse_lenient_json(raw: &str) -> Option<Value> {
    if let Ok(value) = serde_json::from_str::<Value>(raw) {
        return Some(value);
    }
    let mut out = String::with_capacity(raw.len() + 8);
    let mut in_string = false;
    let mut escaped = false;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            if escaped {
                out.push(c);
                escaped = false;
                continue;
            }
            match c {
                '\\' => {
                    out.push(c);
                    escaped = true;
                }
                '"' => {
                    // The quote closes the string only when the next
                    // non-whitespace character is structural (`:` included so
                    // a key's closing quote recovers); anything else (for
                    // example a shell-quoted `"` before another `"`) stays
                    // inside the value as an escaped quote.
                    let closes = chars
                        .clone()
                        .find(|ch| !ch.is_whitespace())
                        .is_some_and(|next| matches!(next, ',' | ':' | '}' | ']' | '{'));
                    if closes {
                        out.push('"');
                        in_string = false;
                    } else {
                        out.push('\\');
                        out.push('"');
                    }
                }
                _ => out.push(c),
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else {
            out.push(c);
        }
    }
    serde_json::from_str(&out).ok()
}
