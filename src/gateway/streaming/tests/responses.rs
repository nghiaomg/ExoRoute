use super::support::*;
use super::*;

#[test]
fn recognizes_responses_terminal_aliases_and_statuses() {
    assert_eq!(
        responses_terminal_state(
            "",
            &json!({
                "type":"response.completed",
                "response":{"status":"completed"}
            })
        ),
        Some(ResponsesTerminal::Completed)
    );
    assert_eq!(
        responses_terminal_state(
            "",
            &json!({
                "type":"response.done",
                "response":{"status":"completed"}
            })
        ),
        Some(ResponsesTerminal::Completed)
    );
    assert_eq!(
        responses_terminal_state(
            "response.incomplete",
            &json!({
                "response":{"status":"incomplete"}
            })
        ),
        Some(ResponsesTerminal::Incomplete)
    );
    assert_eq!(
        extract_stream_finish(
            UpstreamProtocol::Responses,
            "response.incomplete",
            &json!({
                "type":"response.incomplete",
                "response":{
                    "status":"incomplete",
                    "output":[{"type":"function_call"}]
                }
            })
        ),
        Some("length".to_owned())
    );
    assert_eq!(
        responses_terminal_state(
            "",
            &json!({
                "type":"response.done",
                "response":{"status":"incomplete"}
            })
        ),
        Some(ResponsesTerminal::Incomplete)
    );
    assert_eq!(
        responses_terminal_state(
            "",
            &json!({
                "type":"response.done",
                "response":{"status":"failed"}
            })
        ),
        Some(ResponsesTerminal::Failed)
    );
    assert_eq!(
        responses_terminal_state(
            "",
            &json!({"type":"response.failed","error":{"code":"upstream_error"}})
        ),
        Some(ResponsesTerminal::Failed)
    );
    assert_eq!(
        responses_terminal_state("error", &json!({"error":{"message":"provider stopped"}})),
        Some(ResponsesTerminal::Failed)
    );
    assert_eq!(
        responses_terminal_state(
            "response.output_text.delta",
            &json!({"type":"response.output_text.delta"})
        ),
        None
    );
}

#[tokio::test]
async fn responses_done_and_incomplete_without_final_delimiter_are_terminal() {
    let done = concat!(
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"done\"}\n\n",
        "event: response.done\n",
        "data: {\"type\":\"response.done\",\"response\":{\"id\":\"resp-1\",\"object\":\"response\",\"status\":\"completed\",\"model\":\"cmc/model\",\"output\":[{\"id\":\"msg-1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"done\",\"annotations\":[]}]}]}}"
    );
    let (done_body, done_outcome) =
        run_stream_translation_test(done, false, Duration::from_secs(1), None, None).await;
    assert!(
        done_body.contains("\"status\":\"completed\""),
        "{done_body}"
    );
    assert!(!done_body.contains("upstream_error"), "{done_body}");
    assert!(done_outcome.completed());

    let incomplete = concat!(
        "event: response.incomplete\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp-2\",\"object\":\"response\",\"status\":\"incomplete\",\"model\":\"cmc/model\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"},\"output\":[{\"id\":\"msg-2\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"partial\",\"annotations\":[]}]}]}}"
    );
    let (incomplete_body, incomplete_outcome) =
        run_stream_translation_test(incomplete, false, Duration::from_secs(1), None, None).await;
    assert!(
        incomplete_body.contains("\"status\":\"incomplete\""),
        "{incomplete_body}"
    );
    assert!(
        !incomplete_body.contains("upstream_error"),
        "{incomplete_body}"
    );
    assert!(incomplete_outcome.completed());
}

#[tokio::test]
async fn responses_function_call_stream_is_reconstructed_when_terminal_output_is_empty() {
    let body = concat!(
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"status\":\"in_progress\",\"call_id\":\"call_1\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"output_index\":0,\"delta\":\"{\\\"query\\\":\\\"status\\\"}\"}\n\n",
        "event: response.function_call_arguments.done\n",
        "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_1\",\"output_index\":0,\"arguments\":\"{\\\"query\\\":\\\"status\\\"}\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"status\":\"completed\",\"call_id\":\"call_1\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"object\":\"response\",\"status\":\"completed\",\"model\":\"cmc/model\",\"output\":[]}}\n\n",
    );
    let (translated, outcome) =
        run_stream_translation_test(body, false, Duration::from_secs(1), None, None).await;
    assert!(
        translated.contains("response.function_call_arguments.delta"),
        "{translated}"
    );
    assert!(
        translated.contains("\"arguments\":\"{\\\"query\\\":\\\"status\\\"}\""),
        "{translated}"
    );
    assert!(
        translated.contains("\"finish_reason\":\"tool_calls\""),
        "{translated}"
    );
    assert!(!translated.contains("upstream_error"), "{translated}");
    assert!(outcome.completed());
}
