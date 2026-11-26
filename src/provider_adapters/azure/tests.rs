use super::*;

#[test]
fn azure_endpoint_addresses_the_deployment_and_keeps_the_api_version() {
    let adapter = &AZURE_ADAPTER as &dyn ProviderAdapter;
    let url = adapter
        .upstream_endpoint(
            "https://example.openai.azure.com?api-version=2024-10-21",
            UpstreamProtocol::ChatCompletions,
            "gpt-4o-prod",
            false,
            None,
        )
        .unwrap();
    assert_eq!(
        url.path(),
        "/openai/deployments/gpt-4o-prod/chat/completions"
    );
    assert_eq!(
        url.query_pairs()
            .find(|(name, _)| name == "api-version")
            .map(|(_, value)| value.into_owned()),
        Some("2024-10-21".to_owned())
    );
}

#[test]
fn azure_responses_endpoint_uses_the_same_deployment_path() {
    let adapter = &AZURE_ADAPTER as &dyn ProviderAdapter;
    assert_eq!(
        adapter
            .upstream_endpoint(
                "https://example.openai.azure.com?api-version=2024-10-21",
                UpstreamProtocol::Responses,
                "o3-prod",
                true,
                None,
            )
            .unwrap()
            .path(),
        "/openai/deployments/o3-prod/responses"
    );
}

#[test]
fn azure_requires_an_api_version_on_the_provider_url() {
    let adapter = &AZURE_ADAPTER as &dyn ProviderAdapter;
    let error = adapter
        .upstream_endpoint(
            "https://example.openai.azure.com",
            UpstreamProtocol::ChatCompletions,
            "gpt-4o-prod",
            false,
            None,
        )
        .unwrap_err();
    assert!(error.contains("api-version"), "{error}");
}

#[test]
fn azure_rejects_a_missing_or_path_traversing_deployment() {
    let adapter = &AZURE_ADAPTER as &dyn ProviderAdapter;
    let base = "https://example.openai.azure.com?api-version=2024-10-21";
    assert!(
        adapter
            .upstream_endpoint(base, UpstreamProtocol::ChatCompletions, "", false, None)
            .is_err()
    );
    assert!(
        adapter
            .upstream_endpoint(
                base,
                UpstreamProtocol::ChatCompletions,
                "../other/deployments",
                false,
                None
            )
            .is_err()
    );
}

#[test]
fn azure_rejects_protocols_it_cannot_serve() {
    let adapter = &AZURE_ADAPTER as &dyn ProviderAdapter;
    let base = "https://example.openai.azure.com?api-version=2024-10-21";
    assert!(
        adapter
            .upstream_endpoint(base, UpstreamProtocol::Messages, "gpt-4o", false, None)
            .is_err()
    );
    assert!(
        adapter
            .upstream_endpoint(
                base,
                UpstreamProtocol::GoogleGenerateContent,
                "gpt-4o",
                false,
                None
            )
            .is_err()
    );
}

#[test]
fn azure_bare_endpoint_reports_the_missing_deployment() {
    let adapter = &AZURE_ADAPTER as &dyn ProviderAdapter;
    assert!(
        adapter
            .endpoint(
                "https://example.openai.azure.com?api-version=2024-10-21",
                Protocol::ChatCompletions,
                None
            )
            .is_err()
    );
}

#[test]
fn azure_reuses_a_pasted_deployment_url() {
    let adapter = &AZURE_ADAPTER as &dyn ProviderAdapter;
    assert_eq!(
        adapter
            .upstream_endpoint(
                "https://example.openai.azure.com/openai/deployments/gpt-4o-prod?api-version=2024-10-21",
                UpstreamProtocol::ChatCompletions,
                "gpt-4o-prod",
                false,
                None,
            )
            .unwrap()
            .path(),
        "/openai/deployments/gpt-4o-prod/chat/completions"
    );
}

#[test]
fn azure_model_list_endpoint_lists_deployments() {
    let adapter = &AZURE_ADAPTER as &dyn ProviderAdapter;
    assert_eq!(
        adapter
            .model_list_endpoint("https://example.openai.azure.com?api-version=2024-10-21")
            .unwrap()
            .path(),
        "/openai/deployments"
    );
}

#[test]
fn azure_honors_the_adapter_base_url_override() {
    let adapter = &AZURE_ADAPTER as &dyn ProviderAdapter;
    assert_eq!(
        adapter
            .upstream_endpoint(
                "https://stale.openai.azure.com?api-version=2024-10-21",
                UpstreamProtocol::ChatCompletions,
                "gpt-4o",
                false,
                Some("https://live.openai.azure.com?api-version=2024-10-21"),
            )
            .unwrap()
            .host_str(),
        Some("live.openai.azure.com")
    );
}

#[test]
fn azure_validate_config_requires_the_api_version_and_bearer_preset() {
    let adapter = &AZURE_ADAPTER as &dyn ProviderAdapter;
    let preset = preset_for_adapter(AZURE_ADAPTER_ID).expect("azure preset");
    assert!(
        adapter
            .validate_config(
                preset,
                "azure_api_key",
                "https://example.openai.azure.com",
                "chat_completions",
                &["chat_completions".to_owned()],
                true,
            )
            .is_err()
    );
    assert!(
        adapter
            .validate_config(
                preset,
                "azure_api_key",
                "https://example.openai.azure.com?api-version=2024-10-21",
                "chat_completions",
                &["chat_completions".to_owned()],
                true,
            )
            .is_ok()
    );
    // The preset declares only azure_api_key, so a plain bearer header is refused.
    assert!(
        adapter
            .validate_config(
                preset,
                "bearer",
                "https://example.openai.azure.com?api-version=2024-10-21",
                "chat_completions",
                &["chat_completions".to_owned()],
                true,
            )
            .is_err()
    );
}
