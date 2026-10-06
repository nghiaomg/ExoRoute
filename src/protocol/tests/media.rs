use super::*;

#[test]
fn image_urls_and_data_uris_translate_between_all_request_protocols() {
    let chat = json!({
        "model":"m",
        "messages":[{"role":"user","content":[
            {"type":"image_url","image_url":{"url":"https://images.test/photo.png","detail":"high"}},
            {"type":"image_url","image_url":{"url":"data:image/png;base64,aGVsbG8="}}
        ]}]
    });
    let canonical = decode_request(Protocol::ChatCompletions, &chat).expect("valid images");

    let responses = encode_request(Protocol::Responses, &canonical, "m").expect("responses encode");
    assert_eq!(
        responses["input"][0]["content"][0]["image_url"],
        "https://images.test/photo.png"
    );
    assert_eq!(responses["input"][0]["content"][0]["detail"], "high");
    assert_eq!(
        responses["input"][0]["content"][1]["image_url"],
        "data:image/png;base64,aGVsbG8="
    );

    let messages = encode_request(Protocol::Messages, &canonical, "m").expect("messages encode");
    assert_eq!(
        messages["messages"][0]["content"][0]["source"]["type"],
        "url"
    );
    assert_eq!(
        messages["messages"][0]["content"][0]["source"]["url"],
        "https://images.test/photo.png"
    );
    assert_eq!(
        messages["messages"][0]["content"][1]["source"]["type"],
        "base64"
    );
    assert_eq!(
        messages["messages"][0]["content"][1]["source"]["media_type"],
        "image/png"
    );

    let no_detail = decode_request(
            Protocol::Responses,
            &json!({"model":"m","input":[{"role":"user","content":[{"type":"input_image","image_url":"https://images.test/no-detail.png"}]}]}),
        ).expect("responses image without detail");
    let chat_no_detail =
        encode_request(Protocol::ChatCompletions, &no_detail, "m").expect("chat encode");
    assert!(
        chat_no_detail["messages"][0]["content"][0]["image_url"]
            .get("detail")
            .is_none()
    );

    let response_detail = decode_request(
            Protocol::Responses,
            &json!({"model":"m","input":[{"role":"user","content":[{"type":"input_image","image_url":"https://images.test/detail.png","detail":"low"}]}]}),
        ).expect("responses image detail");
    assert_eq!(
        encode_request(Protocol::Responses, &response_detail, "m").unwrap()["input"][0]["content"]
            [0]["detail"],
        "low"
    );
    let anthropic = decode_request(
            Protocol::Messages,
            &json!({"model":"m","messages":[{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"aGVsbG8="}}]}]}),
        ).expect("anthropic base64 image");
    assert_eq!(
        encode_request(Protocol::ChatCompletions, &anthropic, "m").unwrap()["messages"][0]["content"]
            [0]["image_url"]["url"],
        "data:image/jpeg;base64,aGVsbG8="
    );
}

#[test]
fn documents_translate_between_responses_and_anthropic_and_chat_rejects_them() {
    let responses_input = json!({
        "model":"m",
        "input":[{"role":"user","content":[
            {"type":"input_file","file_url":"https://files.test/report.pdf","filename":"report.pdf"},
            {"type":"input_file","file_data":"data:application/pdf;base64,aGVsbG8=","filename":"inline.pdf"}
        ]}]
    });
    let canonical =
        decode_request(Protocol::Responses, &responses_input).expect("responses documents");
    let anthropic = encode_request(Protocol::Messages, &canonical, "m").expect("anthropic docs");
    assert_eq!(
        anthropic["messages"][0]["content"][0]["source"]["type"],
        "url"
    );
    assert_eq!(
        anthropic["messages"][0]["content"][0]["source"]["url"],
        "https://files.test/report.pdf"
    );
    assert_eq!(
        anthropic["messages"][0]["content"][0]["title"],
        "report.pdf"
    );
    assert_eq!(
        anthropic["messages"][0]["content"][1]["source"]["type"],
        "base64"
    );
    assert_eq!(
        anthropic["messages"][0]["content"][1]["source"]["data"],
        "aGVsbG8="
    );
    assert!(
        encode_request(Protocol::ChatCompletions, &canonical, "m")
            .unwrap_err()
            .contains("cannot represent document")
    );

    let from_anthropic = decode_request(
            Protocol::Messages,
            &json!({"model":"m","messages":[{"role":"user","content":[
                {"type":"document","source":{"type":"url","url":"https://files.test/from-anthropic.pdf"}},
                {"type":"document","source":{"type":"base64","media_type":"application/pdf","data":"aGVsbG8="}}
            ]}]}),
        ).expect("Anthropic URL and base64 documents");
    let back_to_responses = encode_request(Protocol::Responses, &from_anthropic, "m")
        .expect("Responses URL and base64 documents");
    assert_eq!(
        back_to_responses["input"][0]["content"][0]["file_url"],
        "https://files.test/from-anthropic.pdf"
    );
    assert_eq!(
        back_to_responses["input"][0]["content"][1]["file_data"],
        "data:application/pdf;base64,aGVsbG8="
    );

    let inline_text = decode_request(
            Protocol::Messages,
            &json!({"model":"m","messages":[{"role":"user","content":[{"type":"document","source":{"type":"text","media_type":"text/plain","data":"inline notes"},"title":"notes.txt"}]}]}),
        ).expect("inline text document");
    let openai = encode_request(Protocol::Responses, &inline_text, "m")
        .expect("responses inline text document");
    assert_eq!(openai["input"][0]["content"][0]["type"], "input_file");
    assert_eq!(openai["input"][0]["content"][0]["filename"], "notes.txt");
    assert_eq!(
        openai["input"][0]["content"][0]["file_data"],
        "data:text/plain;base64,aW5saW5lIG5vdGVz"
    );
    let decoded_again =
        decode_request(Protocol::Responses, &openai).expect("decoded inline text document");
    let anthropic_again = encode_request(Protocol::Messages, &decoded_again, "m")
        .expect("re-encoded inline text document");
    assert_eq!(
        anthropic_again["messages"][0]["content"][0]["source"]["type"],
        "text"
    );
    assert_eq!(
        anthropic_again["messages"][0]["content"][0]["source"]["data"],
        "inline notes"
    );
}

#[test]
fn provider_scoped_files_and_malformed_documents_are_rejected_explicitly() {
    for (protocol, body) in [
        (
            Protocol::Responses,
            json!({"model":"m","input":[{"role":"user","content":[{"type":"input_image","file_id":"file_img"}]}]}),
        ),
        (
            Protocol::Responses,
            json!({"model":"m","input":[{"role":"user","content":[{"type":"input_file","file_id":"file_doc"}]}]}),
        ),
        (
            Protocol::Messages,
            json!({"model":"m","messages":[{"role":"user","content":[{"type":"image","source":{"type":"file","file_id":"file_img"}}]}]}),
        ),
        (
            Protocol::Messages,
            json!({"model":"m","messages":[{"role":"user","content":[{"type":"document","source":{"type":"file","file_id":"file_doc"}}]}]}),
        ),
    ] {
        assert!(
            decode_request(protocol, &body)
                .unwrap_err()
                .contains("file")
        );
    }
    for body in [
        json!({"model":"m","input":[{"role":"user","content":[{"type":"input_file"}]}]}),
        json!({"model":"m","input":[{"role":"user","content":[{"type":"input_file","file_data":"data:application/pdf;base64,%%%"}]}]}),
        json!({"model":"m","input":[{"role":"user","content":[{"type":"input_file","file_url":"https://files.test/a.pdf","file_data":"aGVsbG8="}]}]}),
    ] {
        assert!(decode_request(Protocol::Responses, &body).is_err());
    }
}
