use super::*;
use crate::infra::storage::Field;

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
            &combo_with_target_count(MAX_COMBO_TARGETS),
            Some(MAX_COMBO_TARGETS)
        )
        .is_ok()
    );
    assert_eq!(
        validate_combo(
            &combo_with_target_count(MAX_COMBO_TARGETS + 1),
            Some(MAX_COMBO_TARGETS)
        )
        .unwrap_err()
        .0,
        StatusCode::BAD_REQUEST
    );
    assert!(validate_combo(&combo_with_target_count(MAX_COMBO_TARGETS + 1), None).is_ok());
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
    let targets = [(target_key, provider_index, target)];
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
