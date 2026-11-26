use super::*;

#[test]
fn cloud_api_endpoint_handles_trailing_slashes_and_paths() {
    assert_eq!(
        cloud_api_endpoint("https://api.openai.com/v1", "chat/completions")
            .unwrap()
            .as_str(),
        "https://api.openai.com/v1/chat/completions"
    );
    assert_eq!(
        cloud_api_endpoint("https://api.openai.com/v1/", "/chat/completions")
            .unwrap()
            .as_str(),
        "https://api.openai.com/v1/chat/completions"
    );
    assert_eq!(
        cloud_api_endpoint("https://api.perplexity.ai", "chat/completions")
            .unwrap()
            .as_str(),
        "https://api.perplexity.ai/chat/completions"
    );
    assert_eq!(
        cloud_api_endpoint(
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "chat/completions"
        )
        .unwrap()
        .as_str(),
        "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
    );
    assert_eq!(
        cloud_api_endpoint("https://api.fireworks.ai/inference/v1", "models")
            .unwrap()
            .as_str(),
        "https://api.fireworks.ai/inference/v1/models"
    );
}

#[test]
fn openai_endpoints_support_chat_and_responses() {
    let adapter = &OPENAI_ADAPTER;
    assert_eq!(adapter.adapter_id(), "openai");
    assert_eq!(
        adapter
            .endpoint("https://api.openai.com/v1", Protocol::ChatCompletions, None)
            .unwrap()
            .as_str(),
        "https://api.openai.com/v1/chat/completions"
    );
    assert_eq!(
        adapter
            .endpoint("https://api.openai.com/v1", Protocol::Responses, None)
            .unwrap()
            .as_str(),
        "https://api.openai.com/v1/responses"
    );
    assert!(
        adapter
            .endpoint("https://api.openai.com/v1", Protocol::Messages, None)
            .is_err()
    );
    assert_eq!(
        adapter
            .model_list_endpoint("https://api.openai.com/v1")
            .unwrap()
            .as_str(),
        "https://api.openai.com/v1/models"
    );
}

#[test]
fn anthropic_endpoints_support_messages_only() {
    let adapter = &ANTHROPIC_ADAPTER;
    assert_eq!(adapter.adapter_id(), "anthropic");
    assert_eq!(
        adapter
            .endpoint("https://api.anthropic.com/v1", Protocol::Messages, None)
            .unwrap()
            .as_str(),
        "https://api.anthropic.com/v1/messages"
    );
    assert!(
        adapter
            .endpoint(
                "https://api.anthropic.com/v1",
                Protocol::ChatCompletions,
                None
            )
            .is_err()
    );
    assert_eq!(
        adapter
            .model_list_endpoint("https://api.anthropic.com/v1")
            .unwrap()
            .as_str(),
        "https://api.anthropic.com/v1/models"
    );
}

#[test]
fn anthropic_auth_header_injection() {
    let adapter = &ANTHROPIC_ADAPTER;
    let client = reqwest::Client::new();
    let req = client.get("https://api.anthropic.com/v1/models");
    let req = adapter
        .apply_request_auth(
            req,
            "header",
            Some("x-api-key"),
            Some("sk-ant-test-key-123"),
            None,
            "test-sess",
        )
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(
        req.headers().get("x-api-key").unwrap().to_str().unwrap(),
        "sk-ant-test-key-123"
    );

    // Bearer fallback also attaches x-api-key for Anthropic
    let req2 = client.get("https://api.anthropic.com/v1/models");
    let req2 = adapter
        .apply_request_auth(
            req2,
            "bearer",
            None,
            Some("sk-ant-test-key-123"),
            None,
            "test-sess",
        )
        .unwrap()
        .build()
        .unwrap();

    assert_eq!(
        req2.headers().get("x-api-key").unwrap().to_str().unwrap(),
        "sk-ant-test-key-123"
    );
}

#[test]
fn all_cloud_adapters_have_distinct_ids_and_valid_models_urls() {
    let adapters: [&dyn ProviderAdapter; 12] = [
        &OPENAI_ADAPTER,
        &ANTHROPIC_ADAPTER,
        &GEMINI_ADAPTER,
        &DEEPSEEK_ADAPTER,
        &XAI_ADAPTER,
        &MISTRAL_ADAPTER,
        &PERPLEXITY_ADAPTER,
        &GROQ_ADAPTER,
        &CEREBRAS_ADAPTER,
        &DASHSCOPE_ADAPTER,
        &TOGETHER_ADAPTER,
        &FIREWORKS_ADAPTER,
    ];

    let mut ids = std::collections::HashSet::new();
    for a in adapters {
        assert!(ids.insert(a.adapter_id()));
        assert!(a.model_list_endpoint("https://example.com/v1").is_ok());
    }
}
