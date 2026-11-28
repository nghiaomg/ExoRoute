use crate::{
    gateway::request_preparation::{ProviderModelAliasResolution, resolve_provider_model_alias},
    provider_adapters::OPENCODE_GO_ADAPTER_ID,
    support::test_support::{ProviderSeed, TestDatabase, seed_provider},
};

#[tokio::test]
async fn unknown_opencode_model_routes_without_a_saved_model() {
    let database = TestDatabase::open().await;
    seed_provider(
        &database.db,
        ProviderSeed {
            id: "opencode",
            name: "OpenCode Go",
            base_url: "https://opencode.ai/zen/go/v1",
            adapter_id: OPENCODE_GO_ADAPTER_ID,
            auth_type: "bearer",
            model_prefix: "ocg",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed provider");
    seed_provider(
        &database.db,
        ProviderSeed {
            id: "generic",
            name: "Generic",
            base_url: "https://example.test/v1",
            adapter_id: "generic",
            auth_type: "bearer",
            model_prefix: "gen",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed generic provider");

    match resolve_provider_model_alias(&database.db, "ocg/brand-new-model")
        .await
        .expect("resolve alias")
    {
        ProviderModelAliasResolution::Found(target) => {
            assert_eq!(target.provider_id, "opencode");
            assert_eq!(target.model, "brand-new-model");
            assert!(target.protocol.is_none());
            assert!(target.model_protocol.is_none());
            assert!(target.enabled);
        }
        ProviderModelAliasResolution::NotFound => panic!("new OpenCode model should passthrough"),
    }

    assert!(matches!(
        resolve_provider_model_alias(&database.db, "gen/brand-new-model")
            .await
            .expect("resolve alias"),
        ProviderModelAliasResolution::NotFound
    ));
}
