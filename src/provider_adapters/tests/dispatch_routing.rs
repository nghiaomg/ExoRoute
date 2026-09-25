use super::*;

#[test]
fn custom_headers_are_applied_to_provider_requests() {
    let headers = BTreeMap::from([
        ("x-test".to_owned(), "bla bla".to_owned()),
        ("x-test-b".to_owned(), "blue blue".to_owned()),
    ]);
    let request = apply_custom_headers(
        reqwest::Client::new().get("https://api.example.com/v1/models"),
        &headers,
    )
    .expect("custom headers are valid")
    .build()
    .expect("provider request builds");

    assert_eq!(request.headers().get("x-test").unwrap(), "bla bla");
    assert_eq!(request.headers().get("x-test-b").unwrap(), "blue blue");
}

#[test]
fn command_code_uses_documented_provider_paths_and_capabilities() {
    let command_code = adapter(COMMAND_CODE_ADAPTER_ID).expect("Command Code adapter");
    assert_eq!(
        command_code
            .endpoint(
                "https://api.commandcode.ai",
                Protocol::ChatCompletions,
                None,
            )
            .expect("chat endpoint")
            .as_str(),
        "https://api.commandcode.ai/provider/v1/chat/completions"
    );
    assert_eq!(
        command_code
            .endpoint("https://api.commandcode.ai", Protocol::Messages, None)
            .expect("messages endpoint")
            .as_str(),
        "https://api.commandcode.ai/provider/v1/messages"
    );
    assert_eq!(
        command_code
            .model_list_endpoint("https://api.commandcode.ai")
            .expect("models endpoint")
            .as_str(),
        "https://api.commandcode.ai/provider/v1/models"
    );
    assert!(
        command_code
            .endpoint("https://api.commandcode.ai", Protocol::Responses, None)
            .is_err()
    );
    let capabilities = capabilities(COMMAND_CODE_ADAPTER_ID).expect("capabilities");
    assert!(capabilities.api_key_auth_assist);
    assert!(capabilities.api_key_usage);
    assert!(!capabilities.usage_limits);
    assert!(!capabilities.model_catalog_authoritative);
    assert!(
        validate_adapter_config(
            COMMAND_CODE_ADAPTER_ID,
            "bearer",
            "https://api.commandcode.ai",
            "messages",
            &["messages".to_owned()],
            true,
        )
        .is_ok()
    );
    assert!(
        validate_adapter_config(
            COMMAND_CODE_ADAPTER_ID,
            "bearer",
            "https://api.commandcode.ai",
            "responses",
            &["responses".to_owned()],
            true,
        )
        .is_err()
    );
}

#[test]
fn openrouter_accepts_its_three_protocols_and_requires_bearer_auth() {
    let supported = vec![
        "chat_completions".to_owned(),
        "responses".to_owned(),
        "messages".to_owned(),
    ];
    assert!(
        validate_adapter_config(
            OPENROUTER_ADAPTER_ID,
            "bearer",
            "https://openrouter.ai/api/v1",
            "chat_completions",
            &supported,
            true,
        )
        .is_ok()
    );

    let unsupported = ["chat_completions", "google_generate_content"].map(str::to_owned);
    assert!(
        validate_adapter_config(
            OPENROUTER_ADAPTER_ID,
            "bearer",
            "https://openrouter.ai/api/v1",
            "chat_completions",
            &unsupported,
            true,
        )
        .is_err()
    );
    assert!(
        validate_adapter_config(
            OPENROUTER_ADAPTER_ID,
            "header",
            "https://openrouter.ai/api/v1",
            "chat_completions",
            &["chat_completions".to_owned()],
            true,
        )
        .is_err()
    );
    assert_eq!(
        model_upstream_protocol(OPENROUTER_ADAPTER_ID, "anthropic/claude-test"),
        None,
        "OpenRouter model IDs must not imply an upstream protocol"
    );
}

#[test]
fn opencode_models_route_to_documented_upstream_protocols() {
    use UpstreamProtocol::{ChatCompletions, Messages, Responses};
    // Go: Responses-only models previously fell back to the provider's
    // preferred chat_completions endpoint and upstream rejected them with
    // HTTP 400 "Invalid request parameters".
    for model in ["grok-4.7", "grok-4.6", "gpt-6-luna", "gpt-5.6-luna"] {
        assert_eq!(
            model_upstream_protocol(OPENCODE_GO_ADAPTER_ID, model),
            Some(Responses),
            "Go {model} must use the Responses endpoint"
        );
    }
    for model in ["minimax-m3", "qwen3.8-flash"] {
        assert_eq!(
            model_upstream_protocol(OPENCODE_GO_ADAPTER_ID, model),
            Some(Messages),
            "Go {model} must use the Messages endpoint"
        );
    }
    for model in [
        "glm-5.3",
        "kimi-k3",
        "deepseek-v4.1-flash",
        "mimo-v2.6-flash",
        "space-bunny-free",
    ] {
        assert_eq!(
            model_upstream_protocol(OPENCODE_GO_ADAPTER_ID, model),
            Some(ChatCompletions)
        );
    }
    // Zen additions verified against the published endpoint table.
    assert_eq!(
        model_upstream_protocol(OPENCODE_ZEN_ADAPTER_ID, "grok-4.7"),
        Some(Responses)
    );
    assert_eq!(
        model_upstream_protocol(OPENCODE_ZEN_ADAPTER_ID, "claude-opus-5-5"),
        Some(Messages)
    );
    assert_eq!(
        model_upstream_protocol(OPENCODE_ZEN_ADAPTER_ID, "gpt-6-sol"),
        Some(Responses)
    );
    assert_eq!(
        model_upstream_protocol(OPENCODE_ZEN_ADAPTER_ID, "gpt-6-luna"),
        Some(Responses)
    );
    assert_eq!(
        model_upstream_protocol(OPENCODE_ZEN_ADAPTER_ID, "glm-5"),
        Some(ChatCompletions)
    );
    // Models behind the dedicated /systemone endpoint are not routable
    // through the three OpenAI/Anthropic surfaces.
    assert_eq!(
        model_upstream_protocol(OPENCODE_ZEN_ADAPTER_ID, "jev-1.13"),
        None
    );
}

#[test]
fn command_code_auth_assist_is_dispatched_through_its_adapter() {
    let callback_url = "http://localhost:5959/callback";
    let state = "flow-state-value";
    let auth_url = api_key_auth_assist_start_url(COMMAND_CODE_ADAPTER_ID, callback_url, state)
        .expect("build Command Code Studio URL");
    let parsed = reqwest::Url::parse(&auth_url).expect("valid sign-in URL");
    assert_eq!(parsed.host_str(), Some("commandcode.ai"));
    let query = parsed.query_pairs().collect::<HashMap<_, _>>();
    assert_eq!(
        query.get("callback").map(|value| value.as_ref()),
        Some(callback_url)
    );
    assert_eq!(query.get("state").map(|value| value.as_ref()), Some(state));
    assert!(api_key_auth_assist_origin_allowed(
        COMMAND_CODE_ADAPTER_ID,
        "https://commandcode.ai"
    ));
    assert!(!api_key_auth_assist_origin_allowed(
        GENERIC_ADAPTER_ID,
        "https://commandcode.ai"
    ));
    assert!(api_key_auth_assist_origin_registered(
        "https://commandcode.ai"
    ));
    assert!(!api_key_auth_assist_origin_registered(
        "https://evil.commandcode.ai"
    ));

    let callback = parse_api_key_auth_callback(
        COMMAND_CODE_ADAPTER_ID,
        br#"{"apiKey":"secret","state":"state","userId":"user"}"#,
    )
    .expect("parse Command Code callback");
    assert_eq!(callback.state, "state");
    assert_eq!(callback.user_id.as_deref(), Some("user"));
    assert!(parse_api_key_auth_callback(GENERIC_ADAPTER_ID, b"{}").is_err());
}

#[test]
fn kilo_gateway_uses_its_chat_and_model_paths_and_forwards_only_safe_mode_hints() {
    let kilo = adapter(KILO_GATEWAY_ADAPTER_ID).expect("Kilo adapter");
    assert_eq!(
        kilo.endpoint(
            "https://api.kilo.ai/api/gateway",
            Protocol::ChatCompletions,
            None
        )
        .expect("Kilo chat endpoint")
        .as_str(),
        "https://api.kilo.ai/api/gateway/chat/completions"
    );
    assert_eq!(
        kilo.model_list_endpoint("https://api.kilo.ai/api/gateway")
            .expect("Kilo models endpoint")
            .as_str(),
        "https://api.kilo.ai/api/gateway/models"
    );
    let mut headers = HeaderMap::new();
    headers.insert("x-kilocode-mode", HeaderValue::from_static("plan"));
    let request = kilo
        .apply_client_headers(
            reqwest::Client::new().post("https://api.kilo.ai/api/gateway"),
            &headers,
        )
        .build()
        .expect("Kilo request");
    assert_eq!(
        request.headers().get("x-kilocode-mode"),
        Some(&HeaderValue::from_static("plan"))
    );

    headers.insert("x-kilocode-mode", HeaderValue::from_static("plan:override"));
    let request = kilo
        .apply_client_headers(
            reqwest::Client::new().post("https://api.kilo.ai/api/gateway"),
            &headers,
        )
        .build()
        .expect("Kilo request with invalid mode");
    assert!(!request.headers().contains_key("x-kilocode-mode"));

    let page = parse_provider_model_page(&json!({
        "data": [{"id":"anthropic/claude-sonnet-4.6","context_length":200000}]
    }))
    .expect("Kilo model catalog response");
    assert_eq!(page.models, ["anthropic/claude-sonnet-4.6"]);
}
