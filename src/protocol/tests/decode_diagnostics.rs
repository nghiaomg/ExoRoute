//! Decode failures must name the payload shape callers can act on. An
//! upstream that answers with another protocol's body is a configuration
//! problem, so the error names the payload it saw instead of only reporting
//! that the expected field was missing.

use super::*;

#[test]
fn chat_completions_decode_names_the_payload_shape_when_it_mismatches() {
    let cases = [
        (
            json!({"object":"response","status":"completed","output":[]}),
            "responses",
        ),
        (
            json!({
                "id":"msg-1",
                "type":"message",
                "role":"assistant",
                "content":[{"type":"text","text":"hello"}],
                "stop_reason":"end_turn"
            }),
            "messages",
        ),
        (
            json!({"candidates":[{"content":{"parts":[{"text":"hello"}]}}]}),
            "google_generate_content",
        ),
    ];
    for (payload, expected) in cases {
        let error = decode_upstream_response(UpstreamProtocol::ChatCompletions, &payload, "m")
            .expect_err("a payload without choices must fail chat decoding");
        assert!(
            error.starts_with("chat response has no choices"),
            "unexpected summary: {error}"
        );
        assert!(
            error.contains(expected),
            "the error must name the payload shape it saw: {error}"
        );
        assert!(
            error.contains("upstream protocol"),
            "the error must point at the protocol setting: {error}"
        );
    }
}

#[test]
fn an_empty_choices_list_is_reported_as_empty_without_a_shape_claim() {
    let error = decode_upstream_response(
        UpstreamProtocol::ChatCompletions,
        &json!({"choices":[]}),
        "m",
    )
    .expect_err("an empty choices list must fail chat decoding");
    assert_eq!(
        error,
        "chat response has no choices (the provider returned an empty choices list)"
    );
}

#[test]
fn undecodable_payloads_without_a_known_shape_keep_the_plain_summary() {
    let error = decode_upstream_response(
        UpstreamProtocol::ChatCompletions,
        &json!({"error":{"message":"model is unavailable"}}),
        "m",
    )
    .expect_err("an error payload must fail chat decoding");
    assert_eq!(error, "chat response has no choices");
}

#[test]
fn empty_completions_name_the_shape_that_arrived_instead() {
    // A payload of the expected protocol keeps the plain summary: this is an
    // empty completion, not a configuration mismatch.
    let error = decode_upstream_response(
        UpstreamProtocol::Messages,
        &json!({"content":[],"stop_reason":"end_turn"}),
        "m",
    )
    .expect_err("an empty Messages payload must fail validation");
    assert_eq!(error, "upstream completed the response without content");

    // A completion of the wrong protocol names what arrived.
    let error = decode_upstream_response(
        UpstreamProtocol::Messages,
        &json!({
            "choices":[{"message":{"role":"assistant","content":null},"finish_reason":"stop"}]
        }),
        "m",
    )
    .expect_err("a chat payload must not satisfy Messages decoding");
    assert!(
        error.contains("upstream completed the response without content"),
        "unexpected summary: {error}"
    );
    assert!(error.contains("chat_completions"), "{error}");
    assert!(error.contains("upstream protocol"), "{error}");
}
