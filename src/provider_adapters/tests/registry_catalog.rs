use super::support::*;
use super::*;

#[test]
fn registry_has_unique_ids_and_valid_metadata() {
    let mut adapter_ids = HashSet::new();
    for adapter in registry::adapters() {
        let adapter_id = adapter.adapter_id();
        assert!(adapter_ids.insert(adapter_id), "duplicate adapter ID");
        assert!(preset_for_adapter(adapter_id).is_some());
    }
    let mut preset_ids = HashSet::new();
    for preset in presets() {
        assert!(preset_ids.insert(preset.id), "duplicate preset ID");
        let registered_adapter = adapter(preset.adapter_id).expect("registered preset adapter");
        assert!(!preset.name.trim().is_empty());
        assert!(!preset.default_supported_protocols.is_empty());
        assert!(
            preset
                .default_supported_protocols
                .contains(&preset.default_preferred_protocol)
        );
        assert!(
            preset
                .supported_auth_types
                .contains(&preset.default_auth_type)
        );
        assert!(preset.default_supported_protocols.iter().all(|protocol| {
            let parsed: Result<UpstreamProtocol, _> = protocol.parse();
            crate::protocol::UpstreamProtocol::as_str_list().contains(protocol) && parsed.is_ok()
        }));
        assert!(preset.capabilities.api_keys || preset.capabilities.oauth_accounts);
        assert_eq!(
            preset.capabilities.oauth_accounts || preset.capabilities.api_key_auth_assist,
            preset.capabilities.auth_panel.is_some()
        );
        if let Some(panel) = preset.capabilities.auth_panel {
            assert!(
                crate::provider_adapters::auth::panels::is_known(panel),
                "unknown auth panel"
            );
            assert!(
                auth_panel_is_known(panel),
                "auth panel contract is inconsistent"
            );
            assert!(
                known_auth_panels().contains(&panel),
                "auth panel contract is inconsistent"
            );
        }
        if preset.capabilities.api_key_auth_assist {
            assert!(!registered_adapter.api_key_auth_assist_origins().is_empty());
        }
        for sibling in all_presets().filter(|sibling| sibling.adapter_id == preset.adapter_id) {
            assert_eq!(sibling.capabilities, preset.capabilities);
            assert_eq!(sibling.supported_auth_types, preset.supported_auth_types);
        }
    }
    assert!(adapter_ids.contains(GENERIC_ADAPTER_ID));
    assert!(adapter_ids.contains(KILO_GATEWAY_ADAPTER_ID));
    assert!(adapter_ids.contains(CODEX_ADAPTER_ID));
    assert!(adapter_ids.contains(COMMAND_CODE_ADAPTER_ID));
    assert!(adapter_ids.contains(OPENCODE_GO_ADAPTER_ID));
    assert!(adapter_ids.contains(OPENCODE_ZEN_ADAPTER_ID));
    assert!(adapter_ids.contains(OPENROUTER_ADAPTER_ID));
    assert!(adapter_ids.contains(NVIDIA_NIM_ADAPTER_ID));
    assert!(adapter_ids.contains(FREEBUFF_ADAPTER_ID));
    assert!(adapter_ids.contains(ANTIGRAVITY_ADAPTER_ID));
    assert!(adapter_ids.contains(CLINE_ADAPTER_ID));
    assert!(adapter_ids.contains(CLINEPASS_ADAPTER_ID));
    assert_eq!(
        preset_for_adapter(CLINE_ADAPTER_ID)
            .expect("Cline preset")
            .default_model_prefix,
        Some("cl")
    );
    assert_eq!(
        preset_for_adapter(CLINEPASS_ADAPTER_ID)
            .expect("ClinePass preset")
            .default_model_prefix,
        Some("cp")
    );
    assert!(
        capabilities(CLINE_ADAPTER_ID)
            .expect("Cline capability")
            .local_usage_meter
    );
    assert!(
        capabilities(CLINEPASS_ADAPTER_ID)
            .expect("ClinePass capability")
            .local_usage_meter
    );
    assert!(capabilities(CODEX_ADAPTER_ID).unwrap().usage_limits);
    assert!(!capabilities(GENERIC_ADAPTER_ID).unwrap().usage_limits);
    assert!(preset_ids.contains("custom"));
    assert!(preset_ids.contains("kilo_gateway"));
    assert!(preset_ids.contains("openai_codex"));
    assert!(preset_ids.contains("command_code"));
    assert!(preset_ids.contains("opencode_go"));
    assert!(preset_ids.contains("opencode_zen"));
    assert!(preset_ids.contains("openrouter"));
    assert!(preset_ids.contains("nvidia_nim"));
    assert!(preset_ids.contains("freebuff"));
    assert_eq!(
        capabilities(OPENCODE_GO_ADAPTER_ID)
            .expect("OpenCode Go capability")
            .api_key_usage_status,
        "unverified"
    );
    assert!(
        capabilities(OPENCODE_GO_ADAPTER_ID)
            .expect("OpenCode Go capability")
            .api_key_usage
    );
    assert_eq!(
        capabilities(OPENCODE_ZEN_ADAPTER_ID)
            .expect("OpenCode Zen capability")
            .api_key_usage_status,
        "unverified"
    );
    assert!(
        !capabilities(OPENCODE_ZEN_ADAPTER_ID)
            .expect("OpenCode Zen capability")
            .api_key_usage
    );
    for adapter_id in [OPENCODE_GO_ADAPTER_ID, OPENCODE_ZEN_ADAPTER_ID] {
        assert!(
            capabilities(adapter_id)
                .expect("OpenCode capability")
                .model_protocol_routing,
            "OpenCode adapters route by documented per-model upstream protocol"
        );
    }
    let openrouter_capabilities =
        capabilities(OPENROUTER_ADAPTER_ID).expect("OpenRouter capability");
    assert!(openrouter_capabilities.api_keys);
    assert!(openrouter_capabilities.model_discovery);
    assert!(openrouter_capabilities.api_key_usage);
    assert_eq!(openrouter_capabilities.api_key_usage_status, "supported");
    assert!(openrouter_capabilities.model_catalog_authoritative);
    assert!(!openrouter_capabilities.model_protocol_routing);
    assert!(!openrouter_capabilities.usage_limits);
    let nvidia_capabilities = capabilities(NVIDIA_NIM_ADAPTER_ID).expect("NVIDIA NIM capability");
    assert!(nvidia_capabilities.api_keys);
    assert!(nvidia_capabilities.model_discovery);
    assert!(nvidia_capabilities.local_quota_tracking);
    assert!(!nvidia_capabilities.model_catalog_authoritative);
    assert_eq!(
        nvidia_capabilities.supported_upstream_protocols,
        ["chat_completions"]
    );
    let freebuff_capabilities = capabilities(FREEBUFF_ADAPTER_ID).expect("Freebuff capability");
    assert!(freebuff_capabilities.api_keys);
    assert!(freebuff_capabilities.model_discovery);
    assert!(!freebuff_capabilities.local_usage_meter);
    assert!(!freebuff_capabilities.local_quota_tracking);
    assert!(!freebuff_capabilities.usage_limits);
    assert!(!freebuff_capabilities.api_key_usage);
    assert_eq!(freebuff_capabilities.api_key_usage_status, "unsupported");
    assert!(!freebuff_capabilities.model_catalog_authoritative);
    assert_eq!(
        freebuff_capabilities.supported_upstream_protocols,
        ["chat_completions"]
    );
    assert!(!retry_upstream_response_as_key_rejection(
        FREEBUFF_ADAPTER_ID
    ));
    assert!(retry_upstream_response_as_key_rejection(GENERIC_ADAPTER_ID));
    assert_eq!(
        preset_for_adapter(FREEBUFF_ADAPTER_ID)
            .expect("Freebuff preset")
            .default_model_prefix,
        Some("fb")
    );
    let antigravity_capabilities =
        capabilities(ANTIGRAVITY_ADAPTER_ID).expect("Antigravity capability");
    assert!(!antigravity_capabilities.api_keys);
    assert!(antigravity_capabilities.oauth_accounts);
    assert!(antigravity_capabilities.model_discovery);
    assert!(antigravity_capabilities.usage_limits);
    assert!(!antigravity_capabilities.local_quota_tracking);
    assert!(!antigravity_capabilities.model_catalog_authoritative);
    assert_eq!(
        antigravity_capabilities.supported_upstream_protocols,
        ["google_generate_content"]
    );
    assert_eq!(
        preset_for_adapter(ANTIGRAVITY_ADAPTER_ID)
            .expect("Antigravity preset")
            .default_model_prefix,
        Some("ag")
    );
    assert_eq!(
        preset_for_adapter(ANTIGRAVITY_ADAPTER_ID)
            .expect("Antigravity preset")
            .default_auth_type,
        "antigravity_oauth"
    );
    assert!(supports_custom_oauth_redirect_uri(ANTIGRAVITY_ADAPTER_ID));
    assert!(!supports_custom_oauth_redirect_uri(CODEX_ADAPTER_ID));
}

#[test]
fn yaml_catalog_preserves_builtin_provider_defaults() {
    let presets = presets();
    let expected = [
        (
            "kilo_gateway",
            "Kilo AI Gateway",
            Some("https://api.kilo.ai/api/gateway"),
            ProviderCategory::Gateway,
        ),
        (
            "openai_codex",
            "OpenAI Codex",
            Some(codex_oauth::BASE_URL),
            ProviderCategory::Oauth,
        ),
        (
            "command_code",
            "Command Code",
            Some("https://api.commandcode.ai"),
            ProviderCategory::Gateway,
        ),
        (
            "opencode_go",
            "OpenCode Go",
            Some("https://opencode.ai/zen/go/v1"),
            ProviderCategory::Gateway,
        ),
        (
            "opencode_zen",
            "OpenCode Zen",
            Some("https://opencode.ai/zen/v1"),
            ProviderCategory::Gateway,
        ),
        (
            "openrouter",
            "OpenRouter",
            Some("https://openrouter.ai/api/v1"),
            ProviderCategory::Gateway,
        ),
        (
            "nvidia_nim",
            "NVIDIA NIM",
            Some("https://integrate.api.nvidia.com/v1"),
            ProviderCategory::Gateway,
        ),
    ];

    for (id, name, base_url, category) in expected {
        let preset = presets
            .iter()
            .find(|preset| preset.id == id)
            .expect("provider preset is embedded from YAML");
        assert_eq!(preset.name, name);
        assert_eq!(preset.default_base_url, base_url);
        assert_eq!(preset.category, category);
        assert!(preset.labels.is_empty());
    }
    let freebuff = presets
        .iter()
        .find(|preset| preset.id == "freebuff")
        .expect("Freebuff provider preset is embedded from YAML");
    assert_eq!(freebuff.name, "Freebuff");
    assert_eq!(
        freebuff.default_base_url,
        Some("https://www.codebuff.com/api/v1")
    );
    assert_eq!(freebuff.default_model_prefix, Some("fb"));
    assert_eq!(freebuff.labels, [ProviderLabel::Free]);
    assert_eq!(freebuff.default_auth_type, "bearer");
    assert_eq!(freebuff.default_supported_protocols, ["chat_completions"]);
}

#[test]
fn provider_categories_and_labels_serialize_as_stable_identifiers() {
    assert_eq!(
        serde_json::to_value(ProviderCategory::CloudApi).expect("serialize category"),
        "cloud_api"
    );
    assert_eq!(
        serde_json::to_value(ProviderLabel::Free).expect("serialize label"),
        "free"
    );
    assert_eq!(
        serde_json::to_value(ProviderLabel::FreeTier).expect("serialize label"),
        "free_tier"
    );
}

#[tokio::test]
async fn freebuff_static_catalog_does_not_need_a_key_or_upstream_call() {
    let (_database, state) = test_state().await;
    let discovery = discover_api_key_models(
        FREEBUFF_ADAPTER_ID,
        AdapterApiKeyRequest {
            state: &state,
            base_url: "https://www.codebuff.com/api/v1",
            auth_type: "bearer",
            auth_header: None,
            custom_headers: &BTreeMap::new(),
            preferred_protocol: "chat_completions",
            credential: "",
        },
    )
    .await
    .expect("Freebuff static model catalog");
    assert!(matches!(
        discovery,
        ModelDiscoveryResult::Available { ref models, truncated: false }
            if models == &[
                "deepseek/deepseek-v4-flash",
                "deepseek/deepseek-v4-pro",
                "openai/gpt-5.6-luna",
                "minimax/minimax-m3",
                "mimo/mimo-v2.5",
                "z-ai/glm-5.2",
                "z-ai/glm-5.3-flash",
                "crof/kimi-k3-eco",
                "anthropic/claude-fable-5",
                "meta/muse-spark-1.2-contributor",
            ]
    ));
}

#[test]
fn unknown_adapter_ids_are_not_accepted() {
    assert!(adapter("unregistered").is_none());
    assert!(
        validate_adapter_config(
            "unregistered",
            "none",
            "https://example.com/v1",
            "chat_completions",
            &["chat_completions".to_owned()],
            false
        )
        .is_err()
    );
}
