use super::support::*;
use super::*;

#[tokio::test]
async fn restored_provider_runtime_state_clears_stale_routes_and_usage_results() {
    let state = new_test_state().await;
    state
        .gates
        .round_robin
        .lock()
        .await
        .insert("provider-id".to_owned(), 12);
    state.clear_provider_runtime_state("provider-id").await;
    assert_eq!(
        state.gates.round_robin.lock().await.get("provider-id"),
        Some(&12)
    );
    state
        .set_provider_key_cursor(
            "provider-id",
            "provider/hash/created/2026-01-01T00:00:00.000Z/key-1",
        )
        .await;
    assert_eq!(
        state.provider_key_cursor("provider-id").await.as_deref(),
        Some("provider/hash/created/2026-01-01T00:00:00.000Z/key-1")
    );
    state.clear_provider_runtime_state("provider-id").await;
    assert!(state.provider_key_cursor("provider-id").await.is_none());
    state
        .gates
        .round_robin
        .lock()
        .await
        .insert("route-1".to_owned(), 8);
    state.clear_route_runtime_state("route-1").await;
    assert!(!state.gates.round_robin.lock().await.contains_key("route-1"));

    let cache_epoch = state.provider_usage_cache_epoch();
    let cached = ProviderUsageCacheEntry {
        snapshot: Some(serde_json::json!({"plan":"old"})),
        fetched_at_ms: Some(1),
        fetched_at: Some(Instant::now()),
        last_attempt_at: Instant::now(),
        last_error: None,
    };
    state
        .cache_provider_usage("key-1".to_owned(), cached.clone(), cache_epoch)
        .await;
    assert!(state.cached_provider_usage("key-1").await.is_some());

    state
        .set_provider_key_cursor(
            "restore-provider",
            "provider/hash/created/2026-01-02T00:00:00.000Z/key-9",
        )
        .await;
    state.clear_restored_provider_runtime_state().await;
    assert!(state.cached_provider_usage("key-1").await.is_none());
    state
        .cache_provider_usage("key-1".to_owned(), cached, cache_epoch)
        .await;
    assert!(state.cached_provider_usage("key-1").await.is_none());
    assert!(state.gates.round_robin.lock().await.is_empty());
    assert!(
        state
            .provider_key_cursor("restore-provider")
            .await
            .is_none()
    );
}
