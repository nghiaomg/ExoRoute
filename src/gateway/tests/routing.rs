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

    let route = request_preparation::load_stored_route(
        &database.db,
        "coding",
        crate::protocol::Protocol::ChatCompletions,
    )
    .await
    .expect("load route")
    .expect("route exists");
    assert!(route.enabled);
    assert!(route.flatten_issue.is_none());
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

#[tokio::test]
async fn provider_model_alias_supports_effort_suffix() {
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
        .expect("seed base model");
    crate::support::test_support::seed_provider_model(&database.db, "provider", "gpt-test-high")
        .await
        .expect("seed literal suffixed model");

    // A recognized effort suffix resolves the base model and carries the level.
    let request_preparation::ProviderModelAliasResolution::Found(target) =
        request_preparation::resolve_provider_model_alias(&database.db, "openai/gpt-test-max")
            .await
            .expect("resolve effort alias")
    else {
        panic!("effort-suffixed alias should resolve");
    };
    assert_eq!(target.model, "gpt-test");
    assert_eq!(target.effort.as_deref(), Some("max"));

    // The exact alias keeps working with no effort attached.
    let request_preparation::ProviderModelAliasResolution::Found(target) =
        request_preparation::resolve_provider_model_alias(&database.db, "openai/gpt-test")
            .await
            .expect("resolve plain alias")
    else {
        panic!("plain alias should resolve");
    };
    assert_eq!(target.model, "gpt-test");
    assert_eq!(target.effort, None);

    // A saved model whose name ends in an effort word wins over the suffix
    // reading: `gpt-test-high` is a real model here, not `gpt-test` + high.
    let request_preparation::ProviderModelAliasResolution::Found(target) =
        request_preparation::resolve_provider_model_alias(&database.db, "openai/gpt-test-high")
            .await
            .expect("resolve literal alias")
    else {
        panic!("literal suffixed model should resolve exactly");
    };
    assert_eq!(target.model, "gpt-test-high");
    assert_eq!(target.effort, None);

    // An unrecognized suffix is not an effort hint and must not resolve.
    assert!(matches!(
        request_preparation::resolve_provider_model_alias(&database.db, "openai/gpt-test-turbo")
            .await
            .expect("resolve unknown suffix"),
        request_preparation::ProviderModelAliasResolution::NotFound
    ));
}

/// A combo renamed after creation keeps its original route id, so the gateway
/// must resolve the documented alias — the combo NAME — through the name
/// index rather than assuming alias == route id.
#[tokio::test]
async fn a_renamed_combo_resolves_through_its_name_index() {
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
        "ink",
        "extra",
        &["chat_completions"],
        &[("provider", "gpt-test", "chat_completions", 0)],
    )
    .await
    .expect("seed route");

    let by_name = request_preparation::load_route_for_alias(
        &database.db,
        "extra",
        crate::protocol::Protocol::ChatCompletions,
    )
    .await
    .expect("resolve by name")
    .expect("the renamed combo resolves by its name");
    assert!(by_name.enabled);
    assert_eq!(by_name.targets.len(), 1);
    assert_eq!(by_name.targets[0].provider_id, "provider");

    // Clients that kept addressing the combo by its stored id still resolve.
    let by_id = request_preparation::load_route_for_alias(
        &database.db,
        "ink",
        crate::protocol::Protocol::ChatCompletions,
    )
    .await
    .expect("resolve by id")
    .expect("the stored route id still resolves");
    assert_eq!(by_id.targets.len(), 1);
}

/// Two combos sharing one name (an operator error the create path does not
/// reject) resolve deterministically to the enabled route, whatever the index
/// key order is.
#[tokio::test]
async fn duplicate_combo_names_resolve_to_the_enabled_route() {
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
        "dupe-a",
        "dupe",
        &["chat_completions"],
        &[("provider", "gpt-test", "chat_completions", 0)],
    )
    .await
    .expect("seed first route");
    crate::support::test_support::seed_route(
        &database.db,
        "dupe-b",
        "dupe",
        &["chat_completions"],
        &[("provider", "gpt-test", "chat_completions", 1)],
    )
    .await
    .expect("seed second route");
    // The first route in index order is disabled: the enabled one must win.
    database
        .db
        .write(|transaction| {
            let mut route = transaction
                .get::<Record>(crate::infra::storage::Table::Routes, "dupe-a")?
                .expect("first route");
            route.insert("enabled", crate::infra::storage::Field::Bool(false));
            transaction.put(crate::infra::storage::Table::Routes, "dupe-a", &route)?;
            Ok(())
        })
        .await
        .expect("disable first route");

    let route = request_preparation::load_route_for_alias(
        &database.db,
        "dupe",
        crate::protocol::Protocol::ChatCompletions,
    )
    .await
    .expect("resolve duplicate name")
    .expect("an enabled duplicate exists");
    assert!(route.enabled, "the enabled duplicate is served");
}
