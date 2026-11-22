//! Round-trip coverage for native request options that are not represented in
//! the canonical request model. These options travel through `metadata` and
//! must survive a same-protocol decode/encode cycle without being duplicated
//! into modeled fields.

use super::*;

#[test]
fn responses_native_options_survive_same_protocol_round_trip() {
    let input = json!({
        "model": "gpt-test",
        "input": [{"role":"user","content":[{"type":"input_text","text":"hello"}]}],
        "store": false,
        "previous_response_id": "resp_123",
        "parallel_tool_calls": false,
        "service_tier": "flex",
        "truncation": "auto",
        "user": "u-1",
        "metadata": {"trace":"abc"},
        "text": {"verbosity":"low"},
        "prompt_cache_key": "conv-1"
    });
    let canonical = decode_request(Protocol::Responses, &input).expect("Responses request decodes");
    let encoded = encode_request(Protocol::Responses, &canonical, "gpt-test")
        .expect("native Responses options encode");

    assert_eq!(encoded["store"], json!(false));
    assert_eq!(encoded["previous_response_id"], json!("resp_123"));
    assert_eq!(encoded["parallel_tool_calls"], json!(false));
    assert_eq!(encoded["service_tier"], json!("flex"));
    assert_eq!(encoded["truncation"], json!("auto"));
    assert_eq!(encoded["user"], json!("u-1"));
    assert_eq!(encoded["metadata"], json!({"trace":"abc"}));
    assert_eq!(encoded["text"], json!({"verbosity":"low"}));
    assert_eq!(encoded["prompt_cache_key"], json!("conv-1"));

    // Modeled fields are encoded once, from the canonical model.
    assert_eq!(encoded["model"], json!("gpt-test"));
    assert_eq!(encoded["input"], input["input"]);
}

#[test]
fn messages_native_options_survive_same_protocol_round_trip() {
    let input = json!({
        "model": "claude-test",
        "system": "be brief",
        "messages": [{"role":"user","content":"hello"}],
        "max_tokens": 128,
        "top_k": 5,
        "thinking": {"type":"enabled","budget_tokens":1024},
        "metadata": {"user_id":"u-1"},
        "service_tier": "auto"
    });
    let canonical = decode_request(Protocol::Messages, &input).expect("Messages request decodes");
    let encoded = encode_request(Protocol::Messages, &canonical, "claude-test")
        .expect("native Messages options encode");

    assert_eq!(encoded["top_k"], json!(5));
    assert_eq!(encoded["thinking"], input["thinking"]);
    assert_eq!(encoded["metadata"], json!({"user_id":"u-1"}));
    assert_eq!(encoded["service_tier"], json!("auto"));

    // Modeled fields are not duplicated by metadata forwarding.
    assert_eq!(encoded["model"], json!("claude-test"));
    assert_eq!(encoded["system"], json!("be brief"));
    assert_eq!(encoded["max_tokens"], json!(128));
    let messages = encoded["messages"].as_array().expect("messages array");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], json!("user"));
    assert_eq!(messages[0]["content"][0]["text"], json!("hello"));
    assert!(encoded.get("input").is_none());
}

#[test]
fn native_null_empty_and_nested_values_are_forwarded_verbatim() {
    let responses = json!({
        "model": "gpt-test",
        "input": "hello",
        "previous_response_id": null,
        "background": true,
        "metadata": {"nested":{"items":[1,2,3]}},
        "include": []
    });
    let canonical =
        decode_request(Protocol::Responses, &responses).expect("null-valued options decode");
    let encoded = encode_request(Protocol::Responses, &canonical, "gpt-test")
        .expect("null-valued options encode");
    assert!(encoded.get("previous_response_id").is_some());
    assert_eq!(encoded["previous_response_id"], Value::Null);
    assert_eq!(encoded["background"], json!(true));
    assert_eq!(encoded["metadata"], responses["metadata"]);
    assert_eq!(encoded["include"], json!([]));

    let messages = json!({
        "model": "claude-test",
        "messages": [{"role":"user","content":"hello"}],
        "top_k": null,
        "metadata": {},
        "container": {"id":"c-1"}
    });
    let canonical =
        decode_request(Protocol::Messages, &messages).expect("empty native values decode");
    let encoded = encode_request(Protocol::Messages, &canonical, "claude-test")
        .expect("empty native values encode");
    assert_eq!(encoded["top_k"], Value::Null);
    assert_eq!(encoded["metadata"], json!({}));
    assert_eq!(encoded["container"], json!({"id":"c-1"}));
}

#[test]
fn malformed_native_options_are_reported_without_panicking() {
    let bad_store = json!({
        "model": "gpt-test",
        "input": "hello",
        "store": "yes"
    });
    let canonical = decode_request(Protocol::Responses, &bad_store).expect("decodes");
    let error = encode_request(Protocol::Responses, &canonical, "gpt-test")
        .expect_err("non-boolean store must be reported");
    assert!(error.contains("store"), "{error}");

    let bad_id = json!({
        "model": "gpt-test",
        "input": "hello",
        "previous_response_id": {"unexpected": true}
    });
    let canonical = decode_request(Protocol::Responses, &bad_id).expect("decodes");
    let error = encode_request(Protocol::Responses, &canonical, "gpt-test")
        .expect_err("non-string previous_response_id must be reported");
    assert!(error.contains("previous_response_id"), "{error}");
}

#[test]
fn modeled_fields_take_precedence_over_metadata() {
    let input = json!({"model":"gpt-test","input":"hello"});
    let baseline = decode_request(Protocol::Responses, &input).expect("decodes");
    let baseline_encoded = encode_request(Protocol::Responses, &baseline, "authoritative-model")
        .expect("baseline encodes");
    let mut canonical = decode_request(Protocol::Responses, &input).expect("decodes");
    canonical
        .metadata
        .insert("model".to_owned(), json!("from-metadata"));
    canonical
        .metadata
        .insert("input".to_owned(), json!("from-metadata"));
    let encoded = encode_request(Protocol::Responses, &canonical, "authoritative-model")
        .expect("modeled fields win over metadata");
    assert_eq!(encoded["model"], json!("authoritative-model"));
    assert_eq!(encoded["input"], baseline_encoded["input"]);
    assert_ne!(encoded["input"], json!("from-metadata"));

    let messages = json!({"model":"claude-test","messages":[{"role":"user","content":"hello"}]});
    let baseline = decode_request(Protocol::Messages, &messages).expect("decodes");
    let baseline_encoded = encode_request(Protocol::Messages, &baseline, "authoritative-model")
        .expect("baseline encodes");
    let mut canonical = decode_request(Protocol::Messages, &messages).expect("decodes");
    canonical
        .metadata
        .insert("model".to_owned(), json!("from-metadata"));
    canonical.metadata.insert("messages".to_owned(), json!([]));
    let encoded = encode_request(Protocol::Messages, &canonical, "authoritative-model")
        .expect("modeled fields win over metadata");
    assert_eq!(encoded["model"], json!("authoritative-model"));
    assert_eq!(encoded["messages"], baseline_encoded["messages"]);
}

#[test]
fn cross_protocol_translation_keeps_its_existing_contract() {
    let responses = json!({
        "model": "gpt-test",
        "input": "hello",
        "store": false,
        "previous_response_id": "resp_123",
        "top_k": 5
    });
    let canonical =
        decode_request(Protocol::Responses, &responses).expect("Responses request decodes");
    let chat = encode_request(Protocol::ChatCompletions, &canonical, "chat-model")
        .expect("native Responses options do not break Chat translation");
    assert!(chat.get("previous_response_id").is_none());
    assert!(chat.get("store").is_none());
    assert!(chat.get("top_k").is_none());

    let messages = json!({
        "model": "claude-test",
        "messages": [{"role":"user","content":"hello"}],
        "top_k": 5,
        "metadata": {"user_id":"u-1"}
    });
    let canonical =
        decode_request(Protocol::Messages, &messages).expect("Messages request decodes");
    let chat = encode_request(Protocol::ChatCompletions, &canonical, "chat-model")
        .expect("native Messages options do not break Chat translation");
    assert!(chat.get("top_k").is_none());
    assert!(chat.get("metadata").is_none());

    // Chat-sourced unsupported options still fail instead of being dropped.
    let chat_input = json!({
        "model": "alias",
        "messages": [{"role":"user","content":"hello"}],
        "service_tier": "flex"
    });
    let canonical = decode_request(Protocol::ChatCompletions, &chat_input).expect("decodes");
    let error = encode_request(Protocol::Responses, &canonical, "gpt-test")
        .expect_err("unknown Chat option stays unsupported on Responses");
    assert!(error.contains("service_tier"), "{error}");
}

#[test]
fn sensitive_native_option_names_are_not_forwarded() {
    let input = json!({
        "model": "gpt-test",
        "input": "hello",
        "api_key": "sk-should-not-reach-provider"
    });
    let canonical = decode_request(Protocol::Responses, &input).expect("decodes");
    let error = encode_request(Protocol::Responses, &canonical, "gpt-test")
        .expect_err("credential-like native options are rejected");
    assert!(error.contains("cannot be forwarded safely"), "{error}");

    let input = json!({
        "model": "claude-test",
        "messages": [{"role":"user","content":"hello"}],
        "secret_token": "shh"
    });
    let canonical = decode_request(Protocol::Messages, &input).expect("decodes");
    let error = encode_request(Protocol::Messages, &canonical, "claude-test")
        .expect_err("credential-like native options are rejected");
    assert!(error.contains("cannot be forwarded safely"), "{error}");
}
