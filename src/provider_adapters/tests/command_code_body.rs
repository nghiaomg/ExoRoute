use super::*;
use crate::provider_adapters::normalize_command_code_reasoning_effort;

/// The command-code proxy validates `reasoning_effort` against exactly
/// low|medium|high|xhigh|max and answers HTTP 400 with
/// `Invalid option: expected one of "low"|"medium"|"high"|"xhigh"|"max"` for
/// anything else, including OpenAI-standard values.
#[test]
fn unsupported_reasoning_efforts_are_normalized_to_proxy_tiers() {
    let mut minimal = json!({"model":"m","reasoning_effort":"minimal"});
    normalize_command_code_reasoning_effort(&mut minimal);
    assert_eq!(minimal["reasoning_effort"], json!("low"));

    let mut none = json!({"model":"m","reasoning_effort":"none"});
    normalize_command_code_reasoning_effort(&mut none);
    assert_eq!(none["reasoning_effort"], json!("low"));

    let mut ultra = json!({"model":"m","reasoning_effort":"ultra"});
    normalize_command_code_reasoning_effort(&mut ultra);
    assert_eq!(ultra["reasoning_effort"], json!("max"));

    let mut upper = json!({"model":"m","reasoning_effort":" Minimal "});
    normalize_command_code_reasoning_effort(&mut upper);
    assert_eq!(upper["reasoning_effort"], json!("low"));
}

#[test]
fn supported_reasoning_efforts_and_shapes_are_left_alone() {
    for effort in ["low", "medium", "high", "xhigh", "max"] {
        let mut body = json!({"model":"m","reasoning_effort":effort});
        normalize_command_code_reasoning_effort(&mut body);
        assert_eq!(body["reasoning_effort"], json!(effort));
    }

    // Unknown values stay untouched: the provider is the authority and its own
    // 400 diagnostic reaches the client instead of a silent local rewrite.
    let mut unknown = json!({"model":"m","reasoning_effort":"diagnostic"});
    normalize_command_code_reasoning_effort(&mut unknown);
    assert_eq!(unknown["reasoning_effort"], json!("diagnostic"));

    // Non-string, nested, and absent fields are ignored.
    let mut null_effort = json!({"model":"m","reasoning_effort":null});
    normalize_command_code_reasoning_effort(&mut null_effort);
    assert!(null_effort["reasoning_effort"].is_null());

    let mut nested = json!({"messages":[{"reasoning_effort":"none"}]});
    normalize_command_code_reasoning_effort(&mut nested);
    assert_eq!(nested["messages"][0]["reasoning_effort"], json!("none"));

    let mut absent = json!({"model":"m"});
    normalize_command_code_reasoning_effort(&mut absent);
    assert!(absent.get("reasoning_effort").is_none());
}

/// The normalization runs through the adapter prepare hook, the same hook the
/// gateway and the workspace chat relay call before dispatching to the
/// provider, so both paths normalize the body.
#[test]
fn adapter_prepare_body_normalizes_reasoning_effort() {
    let adapter = adapter(crate::provider_adapters::COMMAND_CODE_ADAPTER_ID)
        .expect("Command Code adapter is registered");
    let mut body = json!({"model":"m","messages":[],"reasoning_effort":"minimal"});
    adapter.prepare_body(&mut body, "probe-request");
    assert_eq!(body["reasoning_effort"], json!("low"));
}
