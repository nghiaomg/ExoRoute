use super::*;

#[tokio::test]
async fn route_and_provider_model_aliases_read_lmdb_indexes() {
    let database = TestDatabase::open().await;
    crate::support::test_support::seed_provider(
        &database.db,
        crate::support::test_support::ProviderSeed {
            id: "provider",
            name: "Mock provider",
            base_url: "https://provider.example/v1",
            adapter_id: "generic",
            auth_type: "none",
            model_prefix: "openai",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed provider");
    crate::support::test_support::seed_provider_model(&database.db, "provider", "gpt-test")
        .await
        .expect("seed model");
    crate::support::test_support::seed_route(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        &[("provider", "gpt-test", "chat_completions", 0)],
    )
    .await
    .expect("seed route");

    let route = request_preparation::load_stored_route(&database.db, "coding")
        .await
        .expect("load route")
        .expect("route exists");
    assert!(route.enabled);
    assert_eq!(route.targets.len(), 1);
    assert_eq!(route.targets[0].provider_id, "provider");
    let request_preparation::ProviderModelAliasResolution::Found(target) =
        request_preparation::resolve_provider_model_alias(&database.db, "openai/gpt-test")
            .await
            .expect("resolve alias")
    else {
        panic!("saved model alias should resolve");
    };
    assert_eq!(target.provider_id, "provider");
    assert_eq!(target.model, "gpt-test");
}
