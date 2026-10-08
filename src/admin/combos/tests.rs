use super::references::{
    ComboReferenceError, ComboReferenceIssue, PendingCombo, validate_pending_combo,
};
use super::storage::TargetRecordWrite;
use super::*;
use crate::config::MAX_ROUTE_TARGETS;
use crate::infra::storage::Field;
use crate::support::test_support::{
    ProviderSeed, RouteTargetSeed, TestDatabase, seed_provider, seed_provider_model,
    seed_route_with_targets,
};
use std::collections::HashMap;

fn combo_with_target_count(count: usize) -> ComboInput {
    ComboInput {
        id: "combo".to_owned(),
        name: "test combo".to_owned(),
        strategy: "priority".to_owned(),
        accepted_protocols: all_protocols(),
        enabled: true,
        targets: (0..count)
            .map(|index| ComboTargetInput {
                provider_id: format!("provider-{index}"),
                model: "test-model".to_owned(),
                combo_id: None,
                protocol: None,
                priority: index as u32,
                enabled: true,
                weight: None,
            })
            .collect(),
    }
}

#[test]
fn combo_api_caps_targets_but_legacy_routes_remain_compatible() {
    assert!(
        validate_combo(
            &combo_with_target_count(MAX_ROUTE_TARGETS),
            Some(MAX_ROUTE_TARGETS)
        )
        .is_ok()
    );
    assert_eq!(
        validate_combo(
            &combo_with_target_count(MAX_ROUTE_TARGETS + 1),
            Some(MAX_ROUTE_TARGETS)
        )
        .unwrap_err()
        .0,
        StatusCode::BAD_REQUEST
    );
    assert!(validate_combo(&combo_with_target_count(MAX_ROUTE_TARGETS + 1), None).is_ok());
}

#[test]
fn route_name_index_preserves_binary_name_order() {
    assert!(
        route_name_index_key("alpha", "a").unwrap() < route_name_index_key("beta", "b").unwrap()
    );
    assert!(route_name_index_key("a", "a").unwrap() < route_name_index_key("aa", "aa").unwrap());
}

#[test]
fn update_uses_path_id_when_body_omits_id() {
    let mut input = combo_with_target_count(1);
    input.id.clear();

    apply_path_id_for_update("vilaoxvision", &mut input).expect("path ID is authoritative");

    assert_eq!(input.id, "vilaoxvision");
}

#[test]
fn update_rejects_a_conflicting_body_id() {
    let mut input = combo_with_target_count(1);

    let error = apply_path_id_for_update("vilaoxvision", &mut input)
        .expect_err("conflicting body ID must be rejected");

    assert_eq!(error, "path id must match combo id");
}

#[tokio::test]
async fn route_target_write_rejects_provider_marked_for_deletion_in_same_transaction() {
    let database = crate::support::test_support::TestDatabase::open().await;
    crate::support::test_support::seed_provider(
        &database.db,
        crate::support::test_support::ProviderSeed {
            id: "provider",
            name: "Test provider",
            base_url: "https://api.example.com",
            adapter_id: "generic",
            auth_type: "bearer",
            model_prefix: "provider",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("provider fixture");

    database
        .db
        .write(|transaction| {
            let mut provider = transaction
                .get::<Record>(Table::Providers, "provider")?
                .ok_or(StorageError::NotFound)?;
            provider.insert("enabled", Field::Bool(false));
            provider.insert("deleting", Field::Bool(true));
            provider.insert("deletion_phase", Field::Text("route_targets".to_owned()));
            transaction.put(Table::Providers, "provider", &provider)
        })
        .await
        .expect("persist deletion tombstone");

    let route_id = "late-route";
    let target_key = route_target_key(route_id, 0, 0).expect("target key");
    let provider_index =
        route_target_provider_index_key("provider", &target_key).expect("provider target index");
    let route = Record::new()
        .with("id", Field::Text(route_id.to_owned()))
        .with("name", Field::Text("Late route".to_owned()))
        .with("strategy", Field::Text("priority".to_owned()))
        .with(
            "accepted_protocols",
            Field::Text("[\"chat_completions\"]".to_owned()),
        )
        .with("enabled", Field::Bool(true));
    let target = Record::new()
        .with("provider_id", Field::Text("provider".to_owned()))
        .with("model", Field::Text("model".to_owned()))
        .with("priority", Field::I64(0))
        .with("enabled", Field::Bool(true));
    let targets = [TargetRecordWrite {
        target_key,
        provider_index: Some(provider_index),
        combo_index: None,
        record: target,
    }];
    let save = database
        .db
        .write(move |transaction| {
            save_combo(
                transaction,
                route_id,
                &route,
                &route_name_index_key("Late route", route_id)?,
                &targets,
                false,
            )
        })
        .await;
    assert!(matches!(save, Err(StorageError::Conflict)));
    assert!(
        database
            .db
            .read(|transaction| transaction.get::<Record>(Table::Routes, route_id))
            .await
            .expect("read rejected route")
            .is_none()
    );
}

fn reference_target(combo_id: &str) -> ComboTargetInput {
    ComboTargetInput {
        provider_id: String::new(),
        model: String::new(),
        combo_id: Some(combo_id.to_owned()),
        protocol: None,
        priority: 0,
        enabled: true,
        weight: None,
    }
}

fn combo_with_references(id: &str, name: &str, references: &[String]) -> ComboInput {
    ComboInput {
        id: id.to_owned(),
        name: name.to_owned(),
        strategy: "priority".to_owned(),
        accepted_protocols: all_protocols(),
        enabled: true,
        targets: references
            .iter()
            .enumerate()
            .map(|(ordinal, combo_id)| ComboTargetInput {
                priority: ordinal as u32,
                ..reference_target(combo_id)
            })
            .collect(),
    }
}

fn provider_target(provider_id: &str, model: &str, priority: u32) -> ComboTargetInput {
    ComboTargetInput {
        provider_id: provider_id.to_owned(),
        model: model.to_owned(),
        combo_id: None,
        protocol: None,
        priority,
        enabled: true,
        weight: None,
    }
}

fn error_message(error: &(StatusCode, Json<Value>)) -> String {
    error.1.0["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// Asserts a handler succeeded and returned the shared `{"ok": true}` envelope.
fn assert_ok(result: Result<Json<Value>, (StatusCode, Json<Value>)>) {
    match result {
        Ok(body) => assert_eq!(body.0["ok"], Value::Bool(true)),
        Err(error) => panic!("expected success, got {}", error_message(&error)),
    }
}

async fn stored_node(database: &TestDatabase, combo_id: &str) -> Option<ComboNode> {
    let combo_id = combo_id.to_owned();
    database
        .db
        .read(move |transaction| transaction.combo_node(&combo_id))
        .await
        .expect("read stored combo node")
}

async fn route_exists(database: &TestDatabase, route_id: &str) -> bool {
    let route_id = route_id.to_owned();
    database
        .db
        .read(move |transaction| {
            Ok(transaction
                .get::<Record>(Table::Routes, &route_id)?
                .is_some())
        })
        .await
        .expect("read route record")
}

async fn test_state(database: &TestDatabase) -> AppState {
    AppState::new(database.config(), database.db.clone())
}

/// Seeds a raw combo of provider targets, which bypasses the provider and model
/// checks the save path performs.
async fn seed_provider_targets(database: &TestDatabase, combo_id: &str, count: usize) {
    let models: Vec<String> = (0..count).map(|index| format!("model-{index}")).collect();
    let targets: Vec<RouteTargetSeed<'_>> = models
        .iter()
        .map(|model| RouteTargetSeed::Provider {
            provider_id: "provider",
            model: model.as_str(),
            protocol: "chat_completions",
        })
        .collect();
    seed_route_with_targets(
        &database.db,
        combo_id,
        combo_id,
        &["chat_completions"],
        true,
        &targets,
    )
    .await
    .expect("seed raw combo");
}

#[tokio::test]
async fn create_rejects_self_reference() {
    let database = TestDatabase::open().await;
    let state = test_state(&database).await;

    let error = create_item(
        state,
        combo_with_references("a", "Combo A", &["a".to_owned()]),
        Some(MAX_ROUTE_TARGETS),
    )
    .await
    .expect_err("a combo cannot reference itself");

    assert_eq!(error.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        error_message(&error),
        "a combo target cannot reference its own combo"
    );
    assert!(!route_exists(&database, "a").await);
}

#[tokio::test]
async fn create_rejects_unknown_combo_reference() {
    let database = TestDatabase::open().await;
    let state = test_state(&database).await;

    let error = create_item(
        state,
        combo_with_references("a", "Combo A", &["ghost".to_owned()]),
        Some(MAX_ROUTE_TARGETS),
    )
    .await
    .expect_err("a reference to a missing combo is rejected");

    assert_eq!(error.0, StatusCode::BAD_REQUEST);
    assert_eq!(error_message(&error), "target combo 'ghost' was not found");
    assert!(!route_exists(&database, "a").await);
}

#[tokio::test]
async fn update_rejects_indirect_cycle_and_keeps_the_old_targets() {
    let database = TestDatabase::open().await;
    let state = test_state(&database).await;

    // a -> b already stored, so pointing b back at a closes a cycle.
    seed_route_with_targets(
        &database.db,
        "a",
        "Combo A",
        &["chat_completions"],
        true,
        &[RouteTargetSeed::Combo { combo_id: "b" }],
    )
    .await
    .expect("seed combo a");
    seed_provider_targets(&database, "b", 1).await;

    let error = update_item(
        state,
        "b".to_owned(),
        combo_with_references("b", "Combo B", &["a".to_owned()]),
        Some(MAX_ROUTE_TARGETS),
    )
    .await
    .expect_err("the cycle must be rejected");

    assert_eq!(error.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        error_message(&error),
        "combo references would create a cycle: b -> a -> b"
    );
    let node = stored_node(&database, "b")
        .await
        .expect("combo b survived the rejected update");
    assert_eq!(node.provider_targets, 1);
    assert!(node.references.is_empty());
}

#[tokio::test]
async fn create_rejects_expanded_target_overflow() {
    let database = TestDatabase::open().await;
    let state = test_state(&database).await;

    seed_provider_targets(&database, "big", MAX_ROUTE_TARGETS).await;
    seed_provider_targets(&database, "one", 1).await;

    let error = create_item(
        state,
        combo_with_references("parent", "Parent", &["big".to_owned(), "one".to_owned()]),
        Some(MAX_ROUTE_TARGETS),
    )
    .await
    .expect_err("an expansion above the target cap is rejected");

    assert_eq!(error.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        error_message(&error),
        format!(
            "combo expands to {} targets; the supported maximum is {MAX_ROUTE_TARGETS}",
            MAX_ROUTE_TARGETS + 1
        )
    );
    assert!(!route_exists(&database, "parent").await);
}

#[tokio::test]
async fn delete_is_blocked_while_referenced_by_another_combo() {
    let database = TestDatabase::open().await;
    let state = test_state(&database).await;

    seed_route_with_targets(
        &database.db,
        "child",
        "Child",
        &["chat_completions"],
        true,
        &[RouteTargetSeed::Provider {
            provider_id: "provider",
            model: "model-child",
            protocol: "chat_completions",
        }],
    )
    .await
    .expect("seed child combo");
    assert_ok(
        create_item(
            state.clone(),
            combo_with_references("parent", "Parent A", &["child".to_owned()]),
            Some(MAX_ROUTE_TARGETS),
        )
        .await,
    );

    let error = delete_item(state, "child".to_owned())
        .await
        .expect_err("a referenced combo must not be deleted");

    assert_eq!(error.0, StatusCode::CONFLICT);
    assert_eq!(
        error_message(&error),
        "This combo is referenced by Parent A. Remove the nested references before deleting it."
    );
    assert!(route_exists(&database, "child").await);
}

#[tokio::test]
async fn delete_succeeds_once_the_nested_reference_is_gone() {
    let database = TestDatabase::open().await;
    let state = test_state(&database).await;

    seed_provider(
        &database.db,
        ProviderSeed {
            id: "provider",
            name: "Test provider",
            base_url: "https://api.example.com",
            adapter_id: "generic",
            auth_type: "bearer",
            model_prefix: "provider",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("provider fixture");
    seed_provider_model(&database.db, "provider", "model")
        .await
        .expect("provider model fixture");
    seed_provider_targets(&database, "child", 1).await;

    // The parent starts with the nested reference and one provider target, so
    // dropping the reference leaves a valid combo behind.
    let mut input = combo_with_references("parent", "Parent A", &["child".to_owned()]);
    input.targets.push(provider_target("provider", "model", 1));
    assert_ok(create_item(state.clone(), input, Some(MAX_ROUTE_TARGETS)).await);
    assert_ok(
        update_item(
            state.clone(),
            "parent".to_owned(),
            ComboInput {
                id: "parent".to_owned(),
                name: "Parent A".to_owned(),
                strategy: "priority".to_owned(),
                accepted_protocols: all_protocols(),
                enabled: true,
                targets: vec![provider_target("provider", "model", 0)],
            },
            Some(MAX_ROUTE_TARGETS),
        )
        .await,
    );

    assert_ok(delete_item(state, "child".to_owned()).await);
    assert!(!route_exists(&database, "child").await);
}

#[tokio::test]
async fn reference_targets_are_recorded_in_the_combo_index_only() {
    let database = TestDatabase::open().await;
    let state = test_state(&database).await;

    seed_route_with_targets(
        &database.db,
        "child",
        "Child",
        &["chat_completions"],
        true,
        &[RouteTargetSeed::Provider {
            provider_id: "provider",
            model: "model-child",
            protocol: "chat_completions",
        }],
    )
    .await
    .expect("seed child combo");
    assert_ok(
        create_item(
            state,
            combo_with_references("parent", "Parent A", &["child".to_owned()]),
            Some(MAX_ROUTE_TARGETS),
        )
        .await,
    );

    let node = stored_node(&database, "parent")
        .await
        .expect("stored parent combo");
    assert_eq!(node.provider_targets, 0);
    assert_eq!(node.references, vec!["child".to_owned()]);

    // A reference row has no provider, so it must not appear in the provider
    // index: that index is what provider deletion walks to find its routes.
    let target_key = route_target_key("parent", 0, 0).expect("reference target key");
    let combo_index_key =
        route_target_combo_index_key("child", &target_key).expect("combo target index");
    let (combo_value, provider_rows) = database
        .db
        .read(move |transaction| {
            Ok((
                transaction.get::<String>(Table::RouteTargetComboIndex, &combo_index_key)?,
                transaction.scan_prefix::<String>(
                    Table::RouteTargetProviderIndex,
                    "",
                    MAX_ROUTE_TARGETS_PAGE,
                )?,
            ))
        })
        .await
        .expect("read target indexes");
    assert_eq!(combo_value.as_deref(), Some("parent"));
    assert!(
        !provider_rows.iter().any(|(_, value)| value == &target_key),
        "the reference row must not be reachable through the provider index"
    );
}

/// An in-memory reference graph, so the walk's decisions can be tested without
/// a database fixture.
#[derive(Default)]
struct MemoryStore {
    nodes: HashMap<String, ComboNode>,
}

impl MemoryStore {
    fn with(
        &mut self,
        combo_id: &str,
        provider_targets: usize,
        references: &[String],
    ) -> &mut Self {
        self.nodes.insert(
            combo_id.to_owned(),
            ComboNode {
                enabled: true,
                provider_targets,
                references: references.to_vec(),
            },
        );
        self
    }

    fn disabled(&mut self, combo_id: &str, references: &[String]) -> &mut Self {
        self.nodes.insert(
            combo_id.to_owned(),
            ComboNode {
                enabled: false,
                provider_targets: 0,
                references: references.to_vec(),
            },
        );
        self
    }

    /// Nodes `c1` .. `c{length}`, each referencing the next, so the chain below
    /// the root has exactly `length` references.
    fn chain(&mut self, length: usize) -> &mut Self {
        for index in 1..=length {
            let next = if index == length {
                Vec::new()
            } else {
                vec![format!("c{}", index + 1)]
            };
            self.with(&format!("c{index}"), 1, &next);
        }
        self
    }
}

impl ComboStore for MemoryStore {
    fn combo_node(&self, combo_id: &str) -> Result<Option<ComboNode>, StorageError> {
        Ok(self.nodes.get(combo_id).cloned())
    }
}

fn check_references(
    store: &MemoryStore,
    root_id: &str,
    references: &[String],
) -> Result<(), ComboReferenceIssue> {
    let input = combo_with_references(root_id, "Root", references);
    validate_pending_combo(store, root_id, &PendingCombo::from_input(&input)).map_err(|error| {
        match error {
            ComboReferenceError::Issue(issue) => issue,
            ComboReferenceError::Storage(error) => panic!("unexpected storage error: {error}"),
        }
    })
}

#[test]
fn pending_self_reference_is_rejected() {
    let store = MemoryStore::default();

    assert_eq!(
        check_references(&store, "root", &["root".to_owned()]),
        Err(ComboReferenceIssue::SelfReference)
    );
}

#[test]
fn reference_cycle_reports_the_path() {
    let mut store = MemoryStore::default();
    store.with("a", 0, &["root".to_owned()]);

    assert_eq!(
        check_references(&store, "root", &["a".to_owned()]),
        Err(ComboReferenceIssue::Cycle {
            path: vec!["root".to_owned(), "a".to_owned(), "root".to_owned()],
        })
    );
}

#[test]
fn reference_depth_at_the_limit_is_accepted_and_beyond_it_is_rejected() {
    // Eight references below the root is the supported maximum.
    let mut allowed = MemoryStore::default();
    allowed.chain(8);
    assert_eq!(
        check_references(&allowed, "root", &["c1".to_owned()]),
        Ok(())
    );

    let mut too_deep = MemoryStore::default();
    too_deep.chain(9);
    assert!(matches!(
        check_references(&too_deep, "root", &["c1".to_owned()]),
        Err(ComboReferenceIssue::Depth { .. })
    ));
}

#[test]
fn reference_to_a_missing_combo_is_rejected() {
    let store = MemoryStore::default();

    assert_eq!(
        check_references(&store, "root", &["ghost".to_owned()]),
        Err(ComboReferenceIssue::Missing {
            combo_id: "ghost".to_owned(),
        })
    );
}

#[test]
fn expansion_above_the_target_limit_is_rejected() {
    // Exactly the cap is still routable.
    let mut store = MemoryStore::default();
    store.with("wide", MAX_ROUTE_TARGETS, &[]);
    assert_eq!(
        check_references(&store, "root", &["wide".to_owned()]),
        Ok(())
    );

    // One more leaf than the cap, contributed by the root's own target.
    let mut input = combo_with_references("root", "Root", &["wide".to_owned()]);
    input.targets.push(provider_target("provider", "model", 1));
    assert_eq!(
        validate_pending_combo(&store, "root", &PendingCombo::from_input(&input)).map_err(
            |error| match error {
                ComboReferenceError::Issue(issue) => issue,
                ComboReferenceError::Storage(error) => panic!("unexpected storage error: {error}"),
            }
        ),
        Err(ComboReferenceIssue::TooManyTargets {
            expanded: MAX_ROUTE_TARGETS + 1,
        })
    );
}

#[test]
fn shared_subgraphs_are_counted_once_per_reference() {
    let mut store = MemoryStore::default();
    store.with("shared", MAX_ROUTE_TARGETS / 2 + 1, &[]);
    store.with("left", 0, &["shared".to_owned()]);
    store.with("right", 0, &["shared".to_owned()]);

    // The memo makes the shared combo cheap to walk, but routing expands it
    // once per reference, so both copies of its leaves count against the cap.
    assert_eq!(
        check_references(&store, "root", &["left".to_owned(), "right".to_owned()]),
        Err(ComboReferenceIssue::TooManyTargets {
            expanded: MAX_ROUTE_TARGETS + 2,
        })
    );
}

#[test]
fn a_disabled_combo_contributes_nothing_to_the_graph() {
    let mut store = MemoryStore::default();
    // The disabled combo points back at the root; because it is disabled its
    // references are never walked, so this is not a cycle.
    store.with("root", 0, &[]);
    store.disabled("off", &["root".to_owned()]);

    assert_eq!(
        check_references(&store, "root", &["off".to_owned()]),
        Ok(())
    );
}

#[test]
fn an_oversized_reference_graph_fails_closed() {
    // One more reachable combo than the walk's own scan cap (4096).
    let mut store = MemoryStore::default();
    let mut references = Vec::new();
    for index in 0..4_097 {
        let combo_id = format!("leaf-{index}");
        store.with(&combo_id, 0, &[]);
        references.push(combo_id);
    }

    assert_eq!(
        check_references(&store, "root", &references),
        Err(ComboReferenceIssue::GraphTooLarge)
    );
}
