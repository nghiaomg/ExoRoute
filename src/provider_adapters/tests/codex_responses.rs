use super::*;

#[test]
fn codex_empty_completed_response_is_rejected_before_non_stream_failover() {
    let error = parse_adapter_event_stream(
        br#"event: response.completed
data: {"type":"response.completed","response":{"status":"completed","output":[]}}

"#,
    )
    .expect_err("empty Codex completion must not be accepted");
    assert!(
        error.message.contains("without content"),
        "{}",
        error.message
    );
    assert!(error.safe_to_fail_over);

    let response = parse_adapter_event_stream(
        br#"event: response.completed
data: {"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}}

"#,
    )
    .expect("non-empty Codex completion remains valid");
    assert_eq!(response["output"][0]["content"][0]["text"], "ok");
}

#[test]
fn codex_completed_response_reconstructs_output_from_incremental_events() {
    let response = parse_adapter_event_stream(
        br#"event: response.output_item.added
data: {"type":"response.output_item.added","output_index":0,"item":{"id":"msg_1","type":"message","role":"assistant","content":[]}}

event: response.output_text.delta
data: {"type":"response.output_text.delta","item_id":"msg_1","output_index":0,"content_index":0,"delta":"OK"}

event: response.output_text.done
data: {"type":"response.output_text.done","item_id":"msg_1","output_index":0,"content_index":0,"text":"OK."}

event: response.output_item.done
data: {"type":"response.output_item.done","output_index":0,"item":{"id":"msg_1","type":"message","role":"assistant","content":[]}}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_1","model":"gpt-5.6-luna","status":"completed","output":[]}}

"#,
    )
    .expect("incremental Codex output should complete successfully");

    assert_eq!(response["output"][0]["id"], "msg_1");
    assert_eq!(response["output"][0]["content"][0]["text"], "OK.");
}

#[test]
fn codex_completed_response_reconstructs_custom_tool_call_input() {
    // Codex freeform tools such as apply_patch stream their patch through
    // custom_tool_call_input events and complete with an empty-input item.
    let response = parse_adapter_event_stream(
        br#"event: response.output_item.added
data: {"type":"response.output_item.added","output_index":0,"item":{"id":"ctc_1","type":"custom_tool_call","status":"in_progress","call_id":"call_1","name":"apply_patch","input":""}}

event: response.custom_tool_call_input.delta
data: {"type":"response.custom_tool_call_input.delta","item_id":"ctc_1","output_index":0,"delta":"*** Begin Patch\n"}

event: response.custom_tool_call_input.delta
data: {"type":"response.custom_tool_call_input.delta","item_id":"ctc_1","output_index":0,"delta":"*** End Patch"}

event: response.custom_tool_call_input.done
data: {"type":"response.custom_tool_call_input.done","item_id":"ctc_1","output_index":0,"input":"*** Begin Patch\n*** End Patch"}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_1","model":"gpt-5.6-luna","status":"completed","output":[]}}

"#,
    )
    .expect("incremental Codex custom tool call should complete successfully");

    assert_eq!(response["output"][0]["type"], "custom_tool_call");
    assert_eq!(response["output"][0]["call_id"], "call_1");
    assert_eq!(response["output"][0]["name"], "apply_patch");
    assert_eq!(
        response["output"][0]["input"],
        "*** Begin Patch\n*** End Patch"
    );
}

#[test]
fn codex_completed_response_reconstructs_function_call_arguments() {
    let response = parse_adapter_event_stream(
        br#"event: response.output_item.added
data: {"type":"response.output_item.added","output_index":0,"item":{"id":"fc_1","type":"function_call","status":"in_progress","call_id":"call_1","name":"lookup","arguments":""}}

event: response.function_call_arguments.delta
 data: {"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":0,"delta":"{\"query\":\"sta"}

event: response.function_call_arguments.delta
data: {"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":0,"delta":"tus\"}"}

event: response.function_call_arguments.done
data: {"type":"response.function_call_arguments.done","item_id":"fc_1","output_index":0,"arguments":"{\"query\":\"status\"}"}

event: response.output_item.done
data: {"type":"response.output_item.done","output_index":0,"item":{"id":"fc_1","type":"function_call","status":"completed","call_id":"call_1","name":"lookup","arguments":""}}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_1","model":"gpt-5.6-luna","status":"completed","output":[{"id":"fc_1","type":"function_call","status":"completed","call_id":"call_1","name":"lookup","arguments":""}]}}

"#,
    )
    .expect("incremental Codex function call should complete successfully");

    assert_eq!(response["output"][0]["type"], "function_call");
    assert_eq!(response["output"][0]["call_id"], "call_1");
    assert_eq!(response["output"][0]["name"], "lookup");
    assert_eq!(response["output"][0]["arguments"], "{\"query\":\"status\"}");
}

#[test]
fn only_codex_can_accept_a_missing_event_stream_content_type() {
    assert!(
        adapter(CODEX_ADAPTER_ID)
            .expect("Codex adapter")
            .allows_missing_event_stream_content_type()
    );
    assert!(
        !adapter(GENERIC_ADAPTER_ID)
            .expect("generic adapter")
            .allows_missing_event_stream_content_type()
    );
}
