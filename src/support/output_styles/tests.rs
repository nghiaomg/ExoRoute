//! Output-style regression tests: catalog bounds, normalization, single
//! injection, bypass classification, and provider-encoder compatibility.

use super::catalog::{
    MAX_INSTRUCTION_BYTES, OUTPUT_STYLE_MARKER, OutputStyleId, OutputStyleLevel,
    OutputStyleSelection, level_text, normalize_styles,
};
use super::{ApplyResult, apply, should_bypass};
use crate::protocol::{CanonicalRequest, ContentBlock, Message, Protocol, Role, UpstreamProtocol};
use serde_json::Value;

fn request(messages: Vec<Message>) -> CanonicalRequest {
    CanonicalRequest {
        source_protocol: Protocol::ChatCompletions,
        model: "model".to_owned(),
        messages,
        tools: Vec::new(),
        tool_choice: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        stop: Vec::new(),
        stream: false,
        metadata: Default::default(),
        output_styles_applied: false,
    }
}

fn text(role: Role, value: &str) -> Message {
    Message {
        role,
        content: vec![ContentBlock::Text {
            text: value.to_owned(),
        }],
        name: None,
    }
}

#[test]
fn catalog_contains_three_styles_and_each_level_is_bounded() {
    assert_eq!(OutputStyleId::ALL.len(), 3);
    for id in OutputStyleId::ALL {
        for level in [
            OutputStyleLevel::Lite,
            OutputStyleLevel::Full,
            OutputStyleLevel::Ultra,
        ] {
            assert!(level_text(id, level).len() < MAX_INSTRUCTION_BYTES);
        }
    }
}

#[test]
fn normalization_orders_styles_and_rejects_duplicates() {
    let input = [
        OutputStyleSelection {
            id: OutputStyleId::Ponytail,
            level: OutputStyleLevel::Ultra,
        },
        OutputStyleSelection {
            id: OutputStyleId::TerseProse,
            level: OutputStyleLevel::Lite,
        },
    ];
    let normalized = normalize_styles(&input).expect("valid styles");
    assert_eq!(normalized[0].id, OutputStyleId::TerseProse);
    assert_eq!(normalized[1].id, OutputStyleId::Ponytail);
    assert!(
        normalize_styles(&[
            OutputStyleSelection {
                id: OutputStyleId::TerseProse,
                level: OutputStyleLevel::Lite,
            },
            OutputStyleSelection {
                id: OutputStyleId::TerseProse,
                level: OutputStyleLevel::Full,
            },
        ])
        .is_err()
    );
}

#[test]
fn applies_once_after_system_and_developer_without_cloning_messages() {
    let mut request = request(vec![
        text(Role::System, "system"),
        text(Role::Developer, "developer"),
        text(Role::User, "hello"),
    ]);
    let styles = [OutputStyleSelection {
        id: OutputStyleId::LessCode,
        level: OutputStyleLevel::Full,
    }];
    let result = apply(&mut request, &styles).expect("apply style");
    assert!(matches!(result, ApplyResult::Applied(_)));
    assert_eq!(request.messages.len(), 4);
    assert!(matches!(request.messages[2].role, Role::System));
    let ContentBlock::Text { text } = &request.messages[2].content[0] else {
        panic!("style message is text");
    };
    assert!(text.contains(OUTPUT_STYLE_MARKER));
    assert_eq!(
        apply(&mut request, &styles).expect("idempotent"),
        ApplyResult::AlreadyApplied
    );
}

#[test]
fn user_supplied_marker_does_not_disable_injection() {
    let mut request = request(vec![
        text(Role::System, "client text [ExoRoute Output Styles]"),
        text(Role::User, "hello"),
    ]);
    let result = apply(
        &mut request,
        &[OutputStyleSelection {
            id: OutputStyleId::TerseProse,
            level: OutputStyleLevel::Lite,
        }],
    )
    .expect("apply style");
    assert!(matches!(result, ApplyResult::Applied(_)));
    assert_eq!(request.messages.len(), 3);
}

#[test]
fn bypass_is_all_or_nothing_and_bounded() {
    let mut request = request(vec![text(
        Role::User,
        "Please explain in detail the security issue",
    )]);
    let result = apply(
        &mut request,
        &[OutputStyleSelection {
            id: OutputStyleId::TerseProse,
            level: OutputStyleLevel::Ultra,
        }],
    )
    .expect("bypass");
    assert_eq!(result, ApplyResult::Bypassed("security_warning"));
    assert_eq!(request.messages.len(), 1);
}

#[test]
fn all_bypass_categories_are_detected_without_substring_false_positives() {
    for (text_value, reason) in [
        ("security review", "security_warning"),
        ("delete this record", "irreversible_action"),
        ("please explain in detail", "clarification_requested"),
        ("first backup, then deploy", "order_sensitive_sequence"),
    ] {
        assert_eq!(should_bypass(&[text(Role::User, text_value)]), Some(reason));
    }
    assert_eq!(
        should_bypass(&[text(Role::User, "insecurity is a word")]),
        None
    );
}

#[test]
fn bypass_scans_nested_tool_and_document_text_within_the_same_byte_limits() {
    let nested_tool_result = Message {
        role: Role::Tool,
        content: vec![ContentBlock::ToolResult {
            tool_call_id: "tool-call".to_owned(),
            content: vec![ContentBlock::Reasoning {
                text: "security review".to_owned(),
            }],
        }],
        name: None,
    };
    assert_eq!(
        should_bypass(&[nested_tool_result]),
        Some("security_warning")
    );

    let document = Message {
        role: Role::User,
        content: vec![ContentBlock::Document {
            source: crate::protocol::DocumentSource::Text {
                text: "delete this record".to_owned(),
            },
            filename: Some("notes.txt".to_owned()),
        }],
        name: None,
    };
    assert_eq!(should_bypass(&[document]), Some("irreversible_action"));
}

#[test]
fn ordered_sequence_scan_handles_unicode_at_lookahead_boundary() {
    let mut text_value = String::from("first ");
    text_value.push_str(&"é".repeat(115));
    text_value.push_str(" deploy!é");
    assert_eq!(
        should_bypass(&[text(Role::User, &text_value)]),
        Some("order_sensitive_sequence")
    );
}

#[test]
fn protocol_encoders_receive_one_synthetic_instruction() {
    let styles = [OutputStyleSelection {
        id: OutputStyleId::TerseProse,
        level: OutputStyleLevel::Lite,
    }];
    for protocol in crate::protocol::UpstreamProtocol::all() {
        let mut request = request(vec![text(Role::User, "hello")]);
        assert!(matches!(
            apply(&mut request, &styles),
            Ok(ApplyResult::Applied(_))
        ));
        let encoded = crate::protocol::encode_upstream_request(*protocol, &request, "model")
            .expect("canonical request encodes");
        let wire = encoded.to_string();
        assert_eq!(wire.matches(OUTPUT_STYLE_MARKER).count(), 1);
        match protocol {
            UpstreamProtocol::ChatCompletions => {
                assert!(encoded["messages"].is_array());
            }
            UpstreamProtocol::Responses => assert!(encoded["instructions"].is_string()),
            UpstreamProtocol::Messages => assert!(encoded["system"].is_string()),
            UpstreamProtocol::GoogleGenerateContent => {
                assert!(encoded["systemInstruction"]["parts"].is_array());
            }
        }
    }
}

#[test]
fn tools_and_media_are_preserved_when_instruction_is_added() {
    let mut request = request(vec![Message {
        role: Role::User,
        content: vec![ContentBlock::Image {
            url: "https://example.com/image.png".to_owned(),
            detail: None,
        }],
        name: None,
    }]);
    request.tools.push(crate::protocol::Tool {
        name: "lookup".to_owned(),
        description: Some("lookup".to_owned()),
        parameters: Value::Object(Default::default()),
    });
    let original_media = request.messages[0].content.len();
    let original_tool_count = request.tools.len();
    apply(
        &mut request,
        &[OutputStyleSelection {
            id: OutputStyleId::LessCode,
            level: OutputStyleLevel::Full,
        }],
    )
    .expect("style applies to media request");
    assert_eq!(request.tools.len(), original_tool_count);
    assert_eq!(
        request.messages.last().unwrap().content.len(),
        original_media
    );
}
