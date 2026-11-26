use super::*;

#[test]
fn effort_suffix_splits_only_recognized_levels() {
    assert_eq!(
        split_model_effort_suffix("gpt-6-luna-max"),
        Some(("gpt-6-luna", "max"))
    );
    assert_eq!(
        split_model_effort_suffix("gpt-6-luna-MAX"),
        Some(("gpt-6-luna", "MAX"))
    );
    assert_eq!(split_model_effort_suffix("gpt-6-luna"), None);
    assert_eq!(split_model_effort_suffix("gpt-6-luna-turbo"), None);
    assert_eq!(split_model_effort_suffix("-max"), None);
    assert_eq!(split_model_effort_suffix("max"), None);
}

#[test]
fn chat_upstream_receives_reasoning_effort() {
    let mut body = json!({"model":"gpt-6-luna","messages":[]});
    apply_model_effort(
        UpstreamProtocol::ChatCompletions,
        &mut body,
        "high",
        "gpt-6-luna",
    )
    .expect("chat effort applies");
    assert_eq!(body["reasoning_effort"], json!("high"));
}

#[test]
fn client_reasoning_options_keep_precedence_over_the_suffix() {
    let mut chat = json!({"model":"m","reasoning_effort":"low"});
    apply_model_effort(
        UpstreamProtocol::ChatCompletions,
        &mut chat,
        "max",
        "gpt-6-luna",
    )
    .expect("chat effort applies");
    assert_eq!(chat["reasoning_effort"], json!("low"));

    let mut responses = json!({"model":"m","reasoning":{"effort":"low","summary":"auto"}});
    apply_model_effort(
        UpstreamProtocol::Responses,
        &mut responses,
        "max",
        "gpt-6-luna",
    )
    .expect("responses effort applies");
    assert_eq!(
        responses["reasoning"],
        json!({"effort":"low","summary":"auto"})
    );
}

#[test]
fn responses_upstream_receives_reasoning_effort() {
    let mut body = json!({"model":"gpt-6-luna","input":[]});
    apply_model_effort(UpstreamProtocol::Responses, &mut body, "max", "gpt-6-luna")
        .expect("responses effort applies");
    assert_eq!(body["reasoning"], json!({"effort":"max"}));

    // A reasoning object without an effort still gains the suffix level.
    let mut body = json!({"model":"m","reasoning":{"summary":"auto"}});
    apply_model_effort(UpstreamProtocol::Responses, &mut body, "low", "gpt-test")
        .expect("responses effort merges");
    assert_eq!(body["reasoning"], json!({"summary":"auto","effort":"low"}));
}

#[test]
fn messages_upstream_receives_output_config_effort() {
    let mut body = json!({"model":"claude-opus-5-5","messages":[]});
    apply_model_effort(
        UpstreamProtocol::Messages,
        &mut body,
        "xhigh",
        "claude-opus-5-5",
    )
    .expect("messages effort applies");
    assert_eq!(body["output_config"], json!({"effort":"xhigh"}));

    // "none" has no Anthropic level and degrades to the lowest tier.
    let mut body = json!({"model":"m","messages":[]});
    apply_model_effort(
        UpstreamProtocol::Messages,
        &mut body,
        "none",
        "claude-opus-5-5",
    )
    .expect("messages effort degrades");
    assert_eq!(body["output_config"], json!({"effort":"low"}));
}

#[test]
fn google_upstream_maps_effort_to_thinking_config() {
    let mut body = json!({"contents":[]});
    apply_model_effort(
        UpstreamProtocol::GoogleGenerateContent,
        &mut body,
        "max",
        "gemini-3-pro",
    )
    .expect("google effort applies");
    assert_eq!(
        body["generationConfig"]["thinkingConfig"],
        json!({"thinkingLevel":"high"})
    );

    let mut body = json!({"contents":[]});
    apply_model_effort(
        UpstreamProtocol::GoogleGenerateContent,
        &mut body,
        "none",
        "gemini-3-pro",
    )
    .expect("google effort applies");
    assert_eq!(
        body["generationConfig"]["thinkingConfig"],
        json!({"thinkingBudget":0})
    );

    // A client-supplied thinking config is not overwritten.
    let mut body =
        json!({"contents":[],"generationConfig":{"thinkingConfig":{"thinkingBudget":1024}}});
    apply_model_effort(
        UpstreamProtocol::GoogleGenerateContent,
        &mut body,
        "max",
        "gemini-3-pro",
    )
    .expect("google effort applies");
    assert_eq!(
        body["generationConfig"]["thinkingConfig"],
        json!({"thinkingBudget":1024})
    );
}

#[test]
fn max_effort_degrades_for_models_outside_the_codex_tier() {
    let mut body = json!({"model":"gpt-4o","messages":[]});
    apply_model_effort(
        UpstreamProtocol::ChatCompletions,
        &mut body,
        "max",
        "gpt-4o",
    )
    .expect("chat effort applies");
    assert_eq!(body["reasoning_effort"], json!("xhigh"));
}
