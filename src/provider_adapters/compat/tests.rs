use super::*;

#[test]
fn compat_adapters_keep_distinct_adapter_ids() {
    let adapters = [
        &MOONSHOT_ADAPTER as &dyn ProviderAdapter,
        &ZHIPU_ADAPTER,
        &MINIMAX_ADAPTER,
        &SAMBANOVA_ADAPTER,
        &HYPERBOLIC_ADAPTER,
        &DEEPINFRA_ADAPTER,
        &NOVITA_ADAPTER,
        &GITHUB_MODELS_ADAPTER,
    ];
    let mut ids = std::collections::HashSet::new();
    for adapter in adapters {
        assert!(ids.insert(adapter.adapter_id()), "duplicate compat id");
    }
    assert_eq!(ids.len(), 8);
}

#[test]
fn compat_endpoints_append_to_the_preset_base_path() {
    // Each case is the documented base path of a provider: the suffix is added
    // to the path the preset already carries, never under an implied `/v1`.
    let cases = [
        (
            &MOONSHOT_ADAPTER as &dyn ProviderAdapter,
            "https://api.moonshot.ai/v1",
            "https://api.moonshot.ai/v1/chat/completions",
        ),
        (
            &ZHIPU_ADAPTER,
            "https://open.bigmodel.cn/api/paas/v4",
            "https://open.bigmodel.cn/api/paas/v4/chat/completions",
        ),
        (
            &MINIMAX_ADAPTER,
            "https://api.minimax.chat/v1",
            "https://api.minimax.chat/v1/chat/completions",
        ),
        (
            &SAMBANOVA_ADAPTER,
            "https://api.sambanova.ai/v1",
            "https://api.sambanova.ai/v1/chat/completions",
        ),
        (
            &HYPERBOLIC_ADAPTER,
            "https://api.hyperbolic.xyz/v1",
            "https://api.hyperbolic.xyz/v1/chat/completions",
        ),
        (
            &DEEPINFRA_ADAPTER,
            "https://api.deepinfra.com/v1/openai",
            "https://api.deepinfra.com/v1/openai/chat/completions",
        ),
        (
            &NOVITA_ADAPTER,
            "https://api.novita.ai/v3/openai",
            "https://api.novita.ai/v3/openai/chat/completions",
        ),
        (
            &GITHUB_MODELS_ADAPTER,
            "https://models.inference.ai.azure.com",
            "https://models.inference.ai.azure.com/chat/completions",
        ),
    ];
    for (adapter, base_url, expected) in cases {
        assert_eq!(
            adapter
                .endpoint(base_url, Protocol::ChatCompletions, None)
                .unwrap()
                .as_str(),
            expected,
            "{} chat endpoint",
            adapter.adapter_id()
        );
    }
}

#[test]
fn compat_endpoints_support_responses_and_never_duplicate_the_suffix() {
    let adapter = &MOONSHOT_ADAPTER as &dyn ProviderAdapter;
    assert_eq!(
        adapter
            .endpoint("https://api.moonshot.ai/v1", Protocol::Responses, None)
            .unwrap()
            .as_str(),
        "https://api.moonshot.ai/v1/responses"
    );
    // A user who pastes the full completion URL keeps it, matching the
    // generic adapter's idempotent behavior.
    assert_eq!(
        adapter
            .endpoint(
                "https://api.moonshot.ai/v1/chat/completions",
                Protocol::ChatCompletions,
                None
            )
            .unwrap()
            .as_str(),
        "https://api.moonshot.ai/v1/chat/completions"
    );
}

#[test]
fn compat_endpoint_honors_the_adapter_base_url_override() {
    let adapter = &MINIMAX_ADAPTER as &dyn ProviderAdapter;
    assert_eq!(
        adapter
            .endpoint(
                "https://api.minimax.chat/v1",
                Protocol::ChatCompletions,
                Some("https://api.minimaxi.com/v1")
            )
            .unwrap()
            .as_str(),
        "https://api.minimaxi.com/v1/chat/completions"
    );
}

#[test]
fn compat_model_list_endpoint_appends_models() {
    let adapter = &ZHIPU_ADAPTER as &dyn ProviderAdapter;
    assert_eq!(
        adapter
            .model_list_endpoint("https://open.bigmodel.cn/api/paas/v4")
            .unwrap()
            .as_str(),
        "https://open.bigmodel.cn/api/paas/v4/models"
    );
}

#[test]
fn compat_rejects_invalid_base_urls() {
    let adapter = &DEEPINFRA_ADAPTER as &dyn ProviderAdapter;
    assert!(
        adapter
            .endpoint("not a url", Protocol::ChatCompletions, None)
            .is_err()
    );
}

#[test]
fn compat_validate_config_rejects_unknown_auth_and_protocols() {
    let adapter = &NOVITA_ADAPTER as &dyn ProviderAdapter;
    let preset = preset_for_adapter(NOVITA_ADAPTER_ID).expect("novita preset");
    // The preset only accepts bearer, so an OAuth-only auth type is refused.
    assert!(
        adapter
            .validate_config(
                preset,
                "antigravity_oauth",
                "https://api.novita.ai/v3/openai",
                "chat_completions",
                &["chat_completions".to_owned()],
                true,
            )
            .is_err()
    );
    // An upstream-only protocol has no client-facing translation here.
    assert!(
        adapter
            .validate_config(
                preset,
                "bearer",
                "https://api.novita.ai/v3/openai",
                "chat_completions",
                &["google_generate_content".to_owned()],
                true,
            )
            .is_err()
    );
    // The preferred protocol must be one the provider actually supports.
    assert!(
        adapter
            .validate_config(
                preset,
                "bearer",
                "https://api.novita.ai/v3/openai",
                "responses",
                &["chat_completions".to_owned()],
                true,
            )
            .is_err()
    );
    assert!(
        adapter
            .validate_config(
                preset,
                "bearer",
                "https://api.novita.ai/v3/openai",
                "chat_completions",
                &["chat_completions".to_owned()],
                true,
            )
            .is_ok()
    );
}
