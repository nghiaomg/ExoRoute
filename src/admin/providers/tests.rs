//! Provider model catalog tests: import versus manual model sources.

use super::models_list::{ProviderModelPageQuery, list_provider_models};
use super::models_manual::{ManualProviderModelInput, add_manual_provider_model};
use super::models_store::{ProviderModelSource, prune_stale_models, store_provider_models};
use super::*;
use crate::infra::storage::{Field, Record, Table};
use crate::support::test_support::{ProviderSeed, TestDatabase, seed_provider, seed_route};

async fn seed_test_provider(database: &TestDatabase, adapter_id: &'static str) -> AppState {
    seed_provider(
        &database.db,
        ProviderSeed {
            id: "provider",
            name: "Mock provider",
            base_url: "https://provider.example/v1",
            adapter_id,
            auth_type: "bearer",
            model_prefix: "mock",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed provider");
    AppState::new(database.config(), database.db.clone())
}

fn model_names(models: &[&str]) -> Vec<String> {
    models.iter().map(|model| (*model).to_owned()).collect()
}

async fn stored_catalog(state: &AppState) -> (Vec<String>, Vec<String>) {
    let Json(value) = list_provider_models(
        State(state.clone()),
        Path("provider".to_owned()),
        Query(ProviderModelPageQuery::default()),
    )
    .await
    .expect("list provider models");
    let models = value["models"]
        .as_array()
        .expect("models array")
        .iter()
        .map(|model| model.as_str().expect("model name").to_owned())
        .collect::<Vec<_>>();
    let manual = value["manual_models"]
        .as_array()
        .expect("manual models array")
        .iter()
        .map(|model| model.as_str().expect("manual model name").to_owned())
        .collect::<Vec<_>>();
    (models, manual)
}

async fn save_manual_model(state: &AppState, model: &str) {
    let Json(value) = add_manual_provider_model(
        State(state.clone()),
        Path("provider".to_owned()),
        Json(ManualProviderModelInput {
            model: model.to_owned(),
        }),
    )
    .await
    .expect("save a manual model");
    assert_eq!(value["source"], "manual");
    assert_eq!(value["saved"], true);
}

async fn import_models(state: &AppState, models: &[&str], authoritative: bool) -> usize {
    store_provider_models(
        &state.db,
        "provider",
        &model_names(models),
        ProviderModelSource::Import,
        authoritative,
    )
    .await
    .expect("store imported models")
    .manual_kept
}

#[tokio::test]
async fn authoritative_import_replaces_imported_models_and_keeps_manual_models() {
    let database = TestDatabase::open().await;
    let state = seed_test_provider(&database, crate::provider_adapters::GENERIC_ADAPTER_ID).await;

    assert_eq!(
        import_models(&state, &["model-a", "model-b"], true).await,
        0
    );
    assert_eq!(
        import_models(&state, &["model-a", "model-b"], true).await,
        0
    );

    save_manual_model(&state, "model-b").await;
    save_manual_model(&state, "model-c").await;

    let (models, manual) = stored_catalog(&state).await;
    assert_eq!(models, model_names(&["model-a", "model-b", "model-c"]));
    assert_eq!(manual, model_names(&["model-b", "model-c"]));

    // The provider now reports model-a and model-d only: the stale imported
    // row goes away, while both manual rows are reported as kept.
    let manual_kept = import_models(&state, &["model-a", "model-d"], true).await;
    assert_eq!(manual_kept, 2);

    let (models, manual) = stored_catalog(&state).await;
    assert_eq!(
        models,
        model_names(&["model-a", "model-b", "model-c", "model-d"])
    );
    assert_eq!(manual, model_names(&["model-b", "model-c"]));
}

#[tokio::test]
async fn authoritative_import_drops_stale_imported_models() {
    let database = TestDatabase::open().await;
    let state = seed_test_provider(&database, crate::provider_adapters::GENERIC_ADAPTER_ID).await;

    import_models(&state, &["model-a", "model-retired"], true).await;
    assert_eq!(import_models(&state, &["model-a"], true).await, 0);

    let (models, manual) = stored_catalog(&state).await;
    assert_eq!(models, model_names(&["model-a"]));
    assert!(manual.is_empty());
}

#[tokio::test]
async fn non_authoritative_store_only_adds_models() {
    let database = TestDatabase::open().await;
    let state = seed_test_provider(&database, crate::provider_adapters::GENERIC_ADAPTER_ID).await;

    import_models(&state, &["model-a", "model-b"], true).await;
    assert_eq!(
        import_models(&state, &["model-c"], false).await,
        0,
        "a merged import never prunes, so it reports no kept manual rows"
    );

    let (models, manual) = stored_catalog(&state).await;
    assert_eq!(models, model_names(&["model-a", "model-b", "model-c"]));
    assert!(manual.is_empty());
}

#[tokio::test]
async fn catalog_rows_written_without_a_source_are_still_imported() {
    let database = TestDatabase::open().await;
    let state = seed_test_provider(&database, crate::provider_adapters::GENERIC_ADAPTER_ID).await;

    // A row written by an earlier release carries no source field.
    let provider_id = "provider".to_owned();
    let primary_key =
        crate::infra::db::provider_model_key(&provider_id, "legacy-model").expect("model key");
    let order_key = crate::infra::db::provider_model_index_key(&provider_id, "legacy-model")
        .expect("model index key");
    database
        .db
        .write(move |transaction| {
            transaction.put(
                Table::ProviderModels,
                &primary_key,
                &Record::new()
                    .with("provider_id", Field::Text("provider".to_owned()))
                    .with("model", Field::Text("legacy-model".to_owned()))
                    .with("upstream_protocol", Field::Null),
            )?;
            transaction.put(
                Table::ProviderModelIndex,
                &order_key,
                &"legacy-model".to_owned(),
            )
        })
        .await
        .expect("write legacy model row");

    import_models(&state, &["model-a"], true).await;

    let (models, manual) = stored_catalog(&state).await;
    assert_eq!(models, model_names(&["model-a"]));
    assert!(
        manual.is_empty(),
        "rows without a source must be treated as imported"
    );
}

#[tokio::test]
async fn stale_pruning_keeps_manual_and_combo_referenced_models() {
    let database = TestDatabase::open().await;
    let state =
        seed_test_provider(&database, crate::provider_adapters::ANTIGRAVITY_ADAPTER_ID).await;

    import_models(
        &state,
        &["MODEL_PLACEHOLDER_M26", "model-a", "model-referenced"],
        false,
    )
    .await;
    save_manual_model(&state, "model-manual").await;
    seed_route(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        &[("provider", "model-referenced", "chat_completions", 0)],
    )
    .await
    .expect("seed route target");

    let pruned = prune_stale_models(&state.db, "provider", &model_names(&["model-a"]))
        .await
        .expect("prune stale models");
    assert_eq!(pruned, model_names(&["MODEL_PLACEHOLDER_M26"]));

    let (models, manual) = stored_catalog(&state).await;
    assert_eq!(
        models,
        model_names(&["model-a", "model-manual", "model-referenced"])
    );
    assert_eq!(manual, model_names(&["model-manual"]));
}
