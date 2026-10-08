//! OpenCode client-identity fingerprint.
//!
//! opencode.ai refuses a free-tier request that does not carry the official
//! client's identity, with HTTP 403 and
//! `{"error":{"type":"FreeTierError","message":"OpenCode's free tier can only be
//! used from within OpenCode"}}`. The refusal is scoped to the *request*, not the
//! account: the same key answers 200 once the request matches the client
//! contract, which is why the fix belongs on the outbound request rather than in
//! credential handling (rotating keys cannot clear it).
//!
//! The contract, as established by the 9router and OmniRoute projects against the
//! live upstream:
//!
//! * `User-Agent: opencode/<major>.<minor>` with a version of at least 1.17;
//! * `x-opencode-client` and `x-opencode-project` client metadata;
//! * `x-opencode-session` in the canonical `ses_<12 hex><14 base62>` format;
//! * the file-search tool quartet `bash`, `glob`, `grep`, `read` declared on the
//!   request body.
//!
//! Every helper here is a pure function of its inputs, so a retry or a later turn
//! of the same conversation derives the same identity without keeping per-request
//! state.

use crate::protocol::UpstreamProtocol;
use http::{HeaderMap, HeaderName, HeaderValue, header};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Identity claimed when the caller does not supply a usable one. The version
/// must satisfy [`has_minimum_client_version`], which is the gate the upstream
/// applies.
pub(super) const CLIENT_USER_AGENT: &str = "opencode/1.18.31";

pub(super) const SESSION_HEADER: &str = "x-opencode-session";
pub(super) const REQUEST_HEADER: &str = "x-opencode-request";
const CLIENT_HEADER: &str = "x-opencode-client";
const PROJECT_HEADER: &str = "x-opencode-project";

const CLIENT_NAME: &str = "desktop";
const PROJECT: &str = "global";

/// Longest accepted value for a forwarded client-metadata header.
const MAX_CLIENT_HEADER_CHARS: usize = 64;

/// The file-search tool quartet the upstream free tier requires on the request.
pub(super) const FINGERPRINT_TOOLS: [&str; 4] = ["bash", "glob", "grep", "read"];

/// Refuses a decoy call so the model does not spend a turn on a tool the caller
/// never declared.
const FINGERPRINT_TOOL_DESCRIPTION: &str =
    "This tool is currently unavailable and must not be used.";

const HEX: &[u8; 16] = b"0123456789abcdef";
const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Whether a `User-Agent` already claims an OpenCode build the upstream accepts.
/// The real CLI's own version is kept when it qualifies, so a genuine client
/// stays visible upstream.
pub(super) fn has_minimum_client_version(user_agent: &str) -> bool {
    let lowered = user_agent.trim().to_ascii_lowercase();
    let Some(version) = lowered.strip_prefix("opencode/") else {
        return false;
    };
    // Only the leading `major.minor` is compared; a trailing patch or suffix is
    // accepted and ignored, matching the upstream's own version check.
    let mut parts = version.split(['.', '-', '+', ' ']);
    let (Some(major), Some(minor)) = (parts.next(), parts.next()) else {
        return false;
    };
    let (Ok(major), Ok(minor)) = (major.parse::<u32>(), minor.parse::<u32>()) else {
        return false;
    };
    major > 1 || (major == 1 && minor >= 17)
}

/// Whether `value` already carries the canonical session id shape.
pub(super) fn is_canonical_session_id(value: &str) -> bool {
    is_canonical_id(value, "ses_")
}

/// Canonical `ses_<12 hex><14 base62>` session id derived from `seed`.
///
/// Deterministic on purpose: the real client keeps one session per conversation
/// and the upstream accounts free-tier quota against it, so deriving the id from
/// the caller-derived session seed keeps retries and later turns on one session
/// without a process-wide map.
pub(super) fn canonical_session_id(seed: &str) -> String {
    canonical_id("ses_", "opencode\u{0}session\u{0}", seed)
}

/// Canonical `msg_<12 hex><14 base62>` request id derived from `seed`.
pub(super) fn canonical_request_id(seed: &str) -> String {
    canonical_id("msg_", "opencode\u{0}request\u{0}", seed)
}

fn is_canonical_id(value: &str, prefix: &str) -> bool {
    let Some(rest) = value.strip_prefix(prefix) else {
        return false;
    };
    let bytes = rest.as_bytes();
    if bytes.len() != 26 {
        return false;
    }
    bytes[..12]
        .iter()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        && bytes[12..].iter().all(u8::is_ascii_alphanumeric)
}

fn canonical_id(prefix: &str, namespace: &str, seed: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(namespace.as_bytes());
    hasher.update(seed.as_bytes());
    let digest = hasher.finalize();
    let mut id = String::with_capacity(prefix.len() + 26);
    id.push_str(prefix);
    for byte in &digest[..6] {
        id.push(HEX[usize::from(byte >> 4)] as char);
        id.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    for byte in &digest[6..20] {
        id.push(BASE62[usize::from(*byte % 62)] as char);
    }
    id
}

/// Session and request identity for one outbound request. A session the caller
/// already sends in canonical form is preserved; anything else (a UUID, an
/// OpenAI-style conversation id, our own request id) is translated so the
/// upstream sees the shape the real client produces.
pub(super) fn fingerprint_headers(session_seed: &str, request_seed: &str) -> HeaderMap {
    let session = if is_canonical_session_id(session_seed) {
        session_seed.to_owned()
    } else {
        canonical_session_id(session_seed)
    };
    let mut headers = HeaderMap::new();
    insert_header(&mut headers, SESSION_HEADER, &session);
    insert_header(
        &mut headers,
        REQUEST_HEADER,
        &canonical_request_id(request_seed),
    );
    headers
}

/// Applies the client metadata the upstream free tier expects. A caller that
/// already supplies an acceptable value keeps it; everything else gets the
/// canonical identity.
pub(super) fn apply_client_identity(
    request: reqwest::RequestBuilder,
    client_headers: &HeaderMap,
) -> reqwest::RequestBuilder {
    let user_agent = client_header(client_headers, header::USER_AGENT.as_str())
        .filter(|value| has_minimum_client_version(value))
        .unwrap_or(CLIENT_USER_AGENT);
    request
        .header(header::USER_AGENT, user_agent)
        .header(
            CLIENT_HEADER,
            client_header(client_headers, CLIENT_HEADER)
                .and_then(safe_client_token)
                .unwrap_or(CLIENT_NAME),
        )
        .header(
            PROJECT_HEADER,
            client_header(client_headers, PROJECT_HEADER)
                .and_then(safe_client_token)
                .unwrap_or(PROJECT),
        )
}

fn client_header<'a>(client_headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    // `HeaderMap` lookups are already case-insensitive.
    client_headers
        .get(name)
        .and_then(|value| value.to_str().ok())
}

/// Accepts a forwarded value only when it is a short, printable token, so a
/// caller cannot inject header content through these fields.
fn safe_client_token(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()
        && value.chars().count() <= MAX_CLIENT_HEADER_CHARS
        && value.chars().all(|character| !character.is_control())
        && !value.contains(char::is_whitespace))
    .then_some(value)
}

fn insert_header(headers: &mut HeaderMap, name: &'static str, value: &str) {
    if let (Ok(name), Ok(value)) = (
        HeaderName::from_bytes(name.as_bytes()),
        HeaderValue::from_str(value),
    ) {
        headers.insert(name, value);
    }
}

/// Declares the fingerprint tool quartet on a request body that is missing it.
///
/// The upstream rejects a request that does not declare `bash`, `glob`, `grep`
/// and `read`, so an agent client that sends its own tool set still needs them,
/// and a request with no tools at all (a model probe, a plain chat call) needs
/// them added outright. A quartet member the caller already declares in any
/// casing is left alone rather than duplicated: the upstream rejects a body that
/// declares the same tool twice, and renaming the caller's tool would hand its
/// tool calls back under a name it does not recognise.
///
/// Only the Chat Completions and Responses tool shapes are handled — the two the
/// upstream gates. The Anthropic Messages and Google tool shapes are left
/// untouched; no refusal has been observed for them.
pub(super) fn apply_fingerprint_tools(body: &mut Value, protocol: UpstreamProtocol) {
    let flat = match protocol {
        UpstreamProtocol::Responses => true,
        UpstreamProtocol::ChatCompletions => false,
        UpstreamProtocol::Messages | UpstreamProtocol::GoogleGenerateContent => return,
    };
    let Some(object) = body.as_object_mut() else {
        return;
    };
    let caller_tools = object.get("tools").and_then(Value::as_array);
    let caller_declared_tools = caller_tools.is_some_and(|tools| !tools.is_empty());
    let declared = caller_tools
        .map(|tools| {
            tools
                .iter()
                .filter_map(tool_name)
                .map(str::to_ascii_lowercase)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !object.get("tools").is_some_and(Value::is_array) {
        object.insert("tools".to_owned(), Value::Array(Vec::new()));
    }
    if let Some(tools) = object.get_mut("tools").and_then(Value::as_array_mut) {
        for name in FINGERPRINT_TOOLS {
            if declared.iter().any(|declared| declared == name) {
                continue;
            }
            tools.push(fingerprint_tool(name, flat));
        }
    }
    // A caller that chose a tool-choice policy keeps it. Otherwise the Responses
    // endpoint takes the upstream-observed `auto` default, and Chat Completions
    // takes `none` when the caller declared no tools — the injected decoys are
    // then the only declarations, and `none` stops the model from answering with
    // a call the caller never declared.
    if object.get("tool_choice").is_none_or(Value::is_null) {
        if flat {
            object.insert("tool_choice".to_owned(), json!("auto"));
        } else if !caller_declared_tools {
            object.insert("tool_choice".to_owned(), json!("none"));
        }
    }
}

fn fingerprint_tool(name: &str, flat: bool) -> Value {
    let parameters = json!({"type": "object", "properties": {}});
    if flat {
        json!({
            "type": "function",
            "name": name,
            "description": FINGERPRINT_TOOL_DESCRIPTION,
            "parameters": parameters,
        })
    } else {
        json!({
            "type": "function",
            "function": {
                "name": name,
                "description": FINGERPRINT_TOOL_DESCRIPTION,
                "parameters": parameters,
            },
        })
    }
}

/// Reads a tool name from either the flat Responses shape or the nested Chat
/// Completions shape.
fn tool_name(tool: &Value) -> Option<&str> {
    tool.get("name")
        .and_then(Value::as_str)
        .or_else(|| {
            tool.get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_supported_client_versions() {
        for accepted in [
            "opencode/1.18.31",
            "opencode/1.17.0",
            "OpenCode/2.0.0",
            "opencode/1.17",
        ] {
            assert!(has_minimum_client_version(accepted), "{accepted}");
        }
        for rejected in [
            "",
            "opencode",
            "opencode/1.16.9",
            "ExoRoute/0.1.1",
            "curl/8.5.0",
            "opencode/",
        ] {
            assert!(!has_minimum_client_version(rejected), "{rejected}");
        }
    }

    #[test]
    fn canonical_ids_match_the_client_shapes() {
        let session = canonical_session_id("conversation-a");
        assert_eq!(session.len(), 30);
        assert!(session.starts_with("ses_"));
        assert!(is_canonical_session_id(&session));

        let request = canonical_request_id("request-a");
        assert_eq!(request.len(), 30);
        assert!(request.starts_with("msg_"));
    }

    #[test]
    fn canonical_ids_are_stable_and_seed_specific() {
        assert_eq!(canonical_session_id("a"), canonical_session_id("a"));
        assert_ne!(canonical_session_id("a"), canonical_session_id("b"));
        assert_ne!(canonical_session_id("a"), canonical_request_id("a"));
    }

    #[test]
    fn rejects_sessions_that_are_not_canonical() {
        for value in [
            "",
            "ses_short",
            "ses_ZZZZZZZZZZZZ00000000000000",
            // Every digit of the time half must be lowercase hex.
            "ses_GGGGGGGGGGGG00000000000000",
            "5b1a1e57-0000-4000-8000-000000000000",
            &format!("ses_{}", "a".repeat(27)),
        ] {
            assert!(!is_canonical_session_id(value), "{value}");
        }
    }

    #[test]
    fn keeps_a_canonical_caller_session_and_translates_anything_else() {
        let canonical = canonical_session_id("conversation-a");
        let headers = fingerprint_headers(&canonical, "request-a");
        assert_eq!(
            headers
                .get(SESSION_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some(canonical.as_str())
        );

        let translated = fingerprint_headers("5b1a1e57-0000-4000-8000-000000000000", "request-a");
        assert_ne!(
            translated.get(SESSION_HEADER).unwrap(),
            "5b1a1e57-0000-4000-8000-000000000000"
        );
        assert!(is_canonical_session_id(
            translated.get(SESSION_HEADER).unwrap().to_str().unwrap()
        ));
    }

    #[test]
    fn client_identity_keeps_a_usable_caller_agent_and_replaces_the_rest() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::USER_AGENT,
            HeaderValue::from_static("opencode/1.20.0"),
        );
        headers.insert(CLIENT_HEADER, HeaderValue::from_static("terminal"));
        assert_eq!(
            client_header(&headers, header::USER_AGENT.as_str()),
            Some("opencode/1.20.0")
        );
        assert_eq!(safe_client_token("terminal"), Some("terminal"));
        assert_eq!(safe_client_token("desktop\r\nx-injected: 1"), None);
        assert_eq!(safe_client_token("   "), None);
        assert_eq!(safe_client_token(&"a".repeat(65)), None);
    }

    #[test]
    fn adds_the_missing_quartet_in_the_chat_shape() {
        let mut body = json!({"model": "m", "messages": []});
        apply_fingerprint_tools(&mut body, UpstreamProtocol::ChatCompletions);
        let names: Vec<&str> = body["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .filter_map(tool_name)
            .collect();
        assert_eq!(names, ["bash", "glob", "grep", "read"]);
        assert!(body["tools"][0]["function"]["parameters"]["properties"].is_object());
        assert_eq!(body["tool_choice"], json!("none"));
    }

    #[test]
    fn adds_the_missing_quartet_in_the_flat_responses_shape() {
        let mut body = json!({"model": "m", "input": []});
        apply_fingerprint_tools(&mut body, UpstreamProtocol::Responses);
        let names: Vec<&str> = body["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .filter_map(tool_name)
            .collect();
        assert_eq!(names, ["bash", "glob", "grep", "read"]);
        assert!(body["tools"][0]["name"].is_string());
        assert!(body["tools"][0].get("function").is_none());
        assert_eq!(body["tool_choice"], json!("auto"));
    }

    #[test]
    fn keeps_caller_tools_and_adds_only_what_is_missing() {
        let mut body = json!({
            "tools": [{"type": "function", "function": {"name": "search", "parameters": {}}}],
            "tool_choice": "required",
        });
        apply_fingerprint_tools(&mut body, UpstreamProtocol::ChatCompletions);
        let names: Vec<&str> = body["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .filter_map(tool_name)
            .collect();
        assert_eq!(names, ["search", "bash", "glob", "grep", "read"]);
        // A caller that declared its own tools keeps its own tool-choice policy.
        assert_eq!(body["tool_choice"], json!("required"));
    }

    #[test]
    fn never_declares_a_quartet_member_twice() {
        let mut body = json!({
            "tools": [
                {"type": "function", "name": "bash", "parameters": {}},
                {"type": "function", "function": {"name": "Read", "parameters": {}}},
            ],
        });
        apply_fingerprint_tools(&mut body, UpstreamProtocol::Responses);
        let names: Vec<&str> = body["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .filter_map(tool_name)
            .collect();
        assert_eq!(names, ["bash", "Read", "glob", "grep"]);
        // The Responses endpoint takes its own default whenever the caller did
        // not choose a policy, whether or not it declared tools.
        assert_eq!(body["tool_choice"], json!("auto"));
    }

    #[test]
    fn keeps_a_caller_tool_choice_policy_on_the_chat_shape() {
        let mut body = json!({"tools": [{"type": "function", "function": {"name": "search"}}]});
        apply_fingerprint_tools(&mut body, UpstreamProtocol::ChatCompletions);
        // A caller that declared tools and no policy keeps the endpoint default
        // rather than being pinned to `none`.
        assert!(body.get("tool_choice").is_none());

        let mut explicit = json!({"tools": [], "tool_choice": null});
        apply_fingerprint_tools(&mut explicit, UpstreamProtocol::ChatCompletions);
        assert_eq!(explicit["tool_choice"], json!("none"));
    }

    #[test]
    fn leaves_unsupported_tool_shapes_untouched() {
        for protocol in [
            UpstreamProtocol::Messages,
            UpstreamProtocol::GoogleGenerateContent,
        ] {
            let mut body = json!({"tools": [{"name": "Bash", "input_schema": {}}]});
            let before = body.clone();
            apply_fingerprint_tools(&mut body, protocol);
            assert_eq!(body, before);
        }
    }
}
