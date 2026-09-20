//! Tests for shared application state runtime and gates.
use super::*;

use crate::{
    infra::db::OperationalSettingsRecord,
    infra::storage::{Field, Record, Table},
    support::test_support::TestDatabase,
};
use std::{ops::Deref, sync::atomic::Ordering, time::Duration};

struct TestState {
    state: AppState,
    _database: TestDatabase,
}

impl Deref for TestState {
    type Target = AppState;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

async fn new_test_state() -> TestState {
    let database = TestDatabase::open().await;
    let state = AppState::new(database.config(), database.db.clone());
    TestState {
        state,
        _database: database,
    }
}

fn live_test_record(request_id: &str, provider_id: Option<&str>) -> RequestLogRecord {
    RequestLogRecord {
        id: uuid::Uuid::new_v4().to_string(),
        request_id: request_id.to_owned(),
        route_alias: "route".to_owned(),
        provider_id: provider_id.map(str::to_owned),
        provider_credential_id: None,
        api_key_id: Some("key".to_owned()),
        model: "model".to_owned(),
        client_protocol: "chat_completions".to_owned(),
        upstream_protocol: Some("chat_completions".to_owned()),
        status: 200,
        duration_ms: 12,
        input_tokens: Some(12),
        output_tokens: Some(7),
        cached_tokens: None,
        cache_input_tokens: None,
        cost_micro_usd: None,
        error: None,
    }
}

#[tokio::test]
async fn request_live_registry_is_bounded_and_keeps_duplicate_request_ids_separate() {
    let registry = Arc::new(RequestLiveRegistry::new());
    let mut updates = registry.subscribe();
    let mut guards = Vec::with_capacity(super::request_live::MAX_REQUEST_LIVE_ENTRIES);
    for _ in 0..super::request_live::MAX_REQUEST_LIVE_ENTRIES {
        guards.push(registry.start("same-request", "route", "model", "messages", None));
    }
    let first_id = match updates.recv().await.expect("first live event") {
        RequestLiveEvent::Started { row, .. } => row.live_id,
        event => panic!("unexpected event: {event:?}"),
    };
    for _ in 1..super::request_live::MAX_REQUEST_LIVE_ENTRIES {
        let _ = updates.recv().await.expect("live start event");
    }
    let second = registry.start("same-request", "route", "model", "messages", None);
    let snapshot = registry.snapshot();
    assert_eq!(
        snapshot.requests.len(),
        super::request_live::MAX_REQUEST_LIVE_ENTRIES
    );
    assert!(snapshot.truncated);
    assert!(
        snapshot
            .requests
            .iter()
            .any(|request| request.live_id == first_id)
    );
    assert_eq!(
        snapshot
            .requests
            .iter()
            .filter(|request| request.request_id == "same-request")
            .count(),
        super::request_live::MAX_REQUEST_LIVE_ENTRIES
    );
    assert_eq!(
        snapshot.active_count,
        super::request_live::MAX_REQUEST_LIVE_ENTRIES + 1
    );
    assert!(matches!(
        updates.recv().await.expect("capacity event"),
        RequestLiveEvent::Capacity {
            truncated: true,
            ..
        }
    ));

    drop(guards.pop());
    assert!(matches!(
        updates.recv().await.expect("finish event"),
        RequestLiveEvent::Finished { .. }
    ));
    assert!(registry.snapshot().truncated);
    assert_eq!(
        registry.snapshot().active_count,
        super::request_live::MAX_REQUEST_LIVE_ENTRIES
    );
    drop(second);
    assert!(matches!(
        updates.recv().await.expect("capacity recovery event"),
        RequestLiveEvent::Capacity {
            truncated: false,
            ..
        }
    ));
    assert_eq!(
        registry.snapshot().active_count,
        super::request_live::MAX_REQUEST_LIVE_ENTRIES - 1
    );
    drop(guards);
}

#[tokio::test]
async fn request_live_guard_updates_provider_and_finishes_once() {
    let registry = Arc::new(RequestLiveRegistry::new());
    let mut updates = registry.subscribe();
    let guard = registry.start("request", "route", "model", "messages", Some("key"));
    let live_id = match updates.recv().await.expect("start event") {
        RequestLiveEvent::Started { row, .. } => row.live_id,
        event => panic!("unexpected event: {event:?}"),
    };
    guard.update_provider(Some("provider-a"));
    assert!(
        matches!(updates.recv().await.expect("provider event"), RequestLiveEvent::Updated { live_id: id, provider_id: Some(provider), .. } if id == live_id && provider == "provider-a")
    );
    let record = live_test_record("request", Some("provider-a"));
    let record_id = record.id.clone();
    guard.update_log(&record);
    assert!(matches!(
        updates.recv().await.expect("latest log event"),
        RequestLiveEvent::Updated {
            latest_log_id: Some(id),
            input_tokens: Some(12),
            output_tokens: Some(7),
            ..
        } if id == record_id
    ));
    drop(guard);
    assert!(
        matches!(updates.recv().await.expect("finish event"), RequestLiveEvent::Finished { live_id: id, request: Some(record), .. } if id == live_id && record.status == 200)
    );
    assert!(matches!(
        updates.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}

#[test]
fn local_rpm_tracker_rolls_a_bounded_sixty_second_window_and_warms_up() {
    let started = Instant::now();
    let mut tracker = ProviderRpmTracker::new(started);
    assert_eq!(
        tracker.snapshot("provider", started),
        LocalQuotaSnapshot {
            requests_last_60s: 0,
            coverage_seconds: 0,
            status: LocalQuotaTrackingStatus::WarmingUp,
        }
    );
    assert!(tracker.record("provider", started));
    assert_eq!(tracker.snapshot("provider", started).requests_last_60s, 1);
    assert_eq!(
        tracker
            .snapshot("provider", started + Duration::from_secs(59))
            .status,
        LocalQuotaTrackingStatus::WarmingUp
    );
    assert_eq!(
        tracker
            .snapshot("provider", started + Duration::from_secs(60))
            .status,
        LocalQuotaTrackingStatus::Ready
    );
    assert_eq!(
        tracker
            .snapshot("provider", started + Duration::from_secs(60))
            .requests_last_60s,
        0
    );
}

#[test]
fn local_rpm_tracker_reports_capacity_without_fabricating_zero_data() {
    let started = Instant::now();
    let mut tracker = ProviderRpmTracker::new(started);
    for index in 0..MAX_LOCAL_PROVIDER_RPM_TRACKERS {
        assert!(tracker.record(&format!("provider-{index}"), started));
    }
    assert!(!tracker.record("overflow-provider", started));
    let snapshot = tracker.snapshot("overflow-provider", started);
    assert_eq!(snapshot.requests_last_60s, 0);
    assert_eq!(snapshot.status, LocalQuotaTrackingStatus::CapacityLimited);
}

#[tokio::test]
async fn expired_admin_access_token_is_removed_from_memory_cache() {
    let state = new_test_state().await;
    let token = state
        .issue_admin_access_token("test-family".to_owned(), false)
        .await
        .expect("access-token entropy");
    assert!(state.admin_session(&token).await.is_some());

    let digest = admin_token_digest(&token);
    if let Some(session) = state.admin.admin_sessions.write().await.get_mut(&digest) {
        session.expires_at = Instant::now() - Duration::from_secs(1);
    }
    assert!(state.admin_session(&token).await.is_none());
    assert!(state.admin.admin_sessions.read().await.is_empty());
}

#[tokio::test]
async fn unified_telemetry_writer_increments_api_key_counts() {
    let database = TestDatabase::open().await;
    for id in ["key-a", "key-b"] {
        let id = id.to_owned();
        let record = Record::new()
            .with("id", Field::Text(id.clone()))
            .with("name", Field::Text(id.clone()))
            .with("token_hash", Field::Bytes(id.as_bytes().to_vec()))
            .with("enabled", Field::Bool(true))
            .with("created_at", Field::Text("2026-01-01T00:00:00Z".to_owned()))
            .with("last_used_at", Field::Null)
            .with("request_count", Field::I64(0))
            .with("request_count_generation", Field::I64(0));
        let key = id.clone();
        database
            .db
            .write(move |transaction| transaction.put_if_absent(Table::ApiKeys, &key, &record))
            .await
            .expect("insert API key");
    }
    let state = AppState::new(database.config(), database.db.clone());
    state.record_api_key_request("key-a".to_owned());
    state.record_api_key_request("key-b".to_owned());
    state.record_api_key_request("key-a".to_owned());
    state
        .telemetry
        .flush()
        .await
        .expect("flush telemetry queue");
    let counts = database
        .db
        .read(|transaction| {
            Ok((
                transaction
                    .get::<Record>(Table::ApiKeys, "key-a")?
                    .ok_or(crate::infra::storage::StorageError::NotFound)?
                    .integer("request_count")?,
                transaction
                    .get::<Record>(Table::ApiKeys, "key-b")?
                    .ok_or(crate::infra::storage::StorageError::NotFound)?
                    .integer("request_count")?,
            ))
        })
        .await
        .expect("read API key request counts");
    assert_eq!(counts, (2, 1));
}

#[tokio::test]
async fn circuit_breaker_opens_and_allows_a_single_half_open_probe() {
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.circuit_breaker_threshold = 2;
    config.circuit_breaker_cooldown = Duration::from_secs(1);
    let state = AppState::new(config, database.db.clone());
    state
        .permit_provider("p")
        .expect("closed circuit admits initial failure")
        .failed();
    state
        .permit_provider("p")
        .expect("circuit remains closed before threshold")
        .failed();
    assert!(state.permit_provider("p").is_none());
    tokio::time::sleep(Duration::from_millis(1_025)).await;
    let probe = state
        .permit_provider("p")
        .expect("expired circuit admits one probe");
    assert!(state.permit_provider("p").is_none());
    drop(probe);
    let mut recovered = state
        .permit_provider("p")
        .expect("dropped probe released the half-open slot");
    recovered.succeeded();
    assert!(state.permit_provider("p").is_some());
}

#[tokio::test]
async fn antigravity_quota_breaker_blocks_after_three_strikes_and_clears() {
    let state = new_test_state().await;
    assert!(
        !state
            .antigravity_quota_is_blocked("antigravity", "account-1", "gemini-3.7-flash")
            .await
    );

    for _ in 0..2 {
        state
            .record_antigravity_quota_rejection(
                "antigravity",
                "account-1",
                "gemini-3.7-flash",
                None,
            )
            .await;
        assert!(
            !state
                .antigravity_quota_is_blocked("antigravity", "account-1", "gemini-3.7-flash")
                .await
        );
    }
    state
        .record_antigravity_quota_rejection("antigravity", "account-1", "gemini-3.7-flash", None)
        .await;
    assert!(
        state
            .antigravity_quota_is_blocked("antigravity", "account-1", "gemini-3.7-flash")
            .await
    );

    state
        .clear_antigravity_quota_breaker("antigravity", "account-1", "gemini-3.7-flash")
        .await;
    assert!(
        !state
            .antigravity_quota_is_blocked("antigravity", "account-1", "gemini-3.7-flash")
            .await
    );
    state
        .record_antigravity_quota_rejection(
            "antigravity",
            "account-1",
            "gemini-3.7-flash",
            Some(Duration::from_secs(60)),
        )
        .await;
    assert!(
        state
            .antigravity_quota_is_blocked("antigravity", "account-1", "gemini-3.7-flash")
            .await
    );
    assert!(
        !state
            .antigravity_quota_is_blocked("antigravity", "account-1", "claude-sonnet-4-6")
            .await
    );
    state.clear_provider_runtime_state("antigravity").await;
    assert!(
        !state
            .antigravity_quota_is_blocked("antigravity", "account-1", "gemini-3.7-flash")
            .await
    );
}

#[tokio::test]
async fn provider_bulkheads_are_bounded_independently() {
    let database = TestDatabase::open().await;
    let state = AppState::new(database.config(), database.db.clone());

    let max_concurrency = state.gateway_resource_limits().provider_max_concurrency;
    let mut held = Vec::with_capacity(max_concurrency);
    for _ in 0..max_concurrency {
        held.push(state.try_provider_permit("busy").await.unwrap());
    }
    assert!(state.try_provider_permit("busy").await.is_none());
    assert!(state.try_provider_permit("healthy").await.is_some());
    drop(held.pop());
    assert!(state.try_provider_permit("busy").await.is_some());
}

#[tokio::test]
async fn provider_concurrency_changes_to_unlimited_immediately_and_keeps_active_permits() {
    let database = TestDatabase::open().await;
    let state = AppState::new(database.config(), database.db.clone());
    let active = state
        .try_provider_permit("provider")
        .await
        .expect("initial provider permit");

    state.apply_gateway_resource_limits(GatewayResourceLimits {
        provider_max_concurrency: GatewayResourceLimits::UNLIMITED_PROVIDER_MAX_CONCURRENCY,
        ..state.gateway_resource_limits()
    });

    assert_eq!(
        state.gateway_resource_limits().provider_max_concurrency,
        GatewayResourceLimits::UNLIMITED_PROVIDER_MAX_CONCURRENCY
    );
    let mut additional = Vec::with_capacity(64);
    for _ in 0..64 {
        additional.push(
            state
                .try_provider_permit("provider")
                .await
                .expect("unlimited provider gate admits new work immediately"),
        );
    }

    state.apply_gateway_resource_limits(GatewayResourceLimits {
        provider_max_concurrency: 1,
        ..state.gateway_resource_limits()
    });
    assert!(state.try_provider_permit("provider").await.is_none());
    drop((active, additional));
    assert!(state.try_provider_permit("provider").await.is_some());
}

#[tokio::test]
async fn dynamic_semaphore_applies_decreases_after_existing_work_finishes() {
    let gate = Arc::new(DynamicSemaphore::new(2));
    let first = gate.try_acquire_owned().expect("first slot");
    let second = gate.try_acquire_owned().expect("second slot");

    gate.set_limit(1);
    assert!(gate.try_acquire_owned().is_none());
    drop(first);
    assert!(gate.try_acquire_owned().is_none());
    drop(second);
    assert!(gate.try_acquire_owned().is_some());
}

#[tokio::test]
async fn dynamic_semaphore_wakes_waiters_when_the_limit_increases() {
    let gate = Arc::new(DynamicSemaphore::new(1));
    let held = gate.try_acquire_owned().expect("initial slot");
    let waiter_gate = gate.clone();
    let waiter = tokio::spawn(async move { waiter_gate.acquire_owned().await });

    gate.set_limit(2);
    let acquired = tokio::time::timeout(Duration::from_secs(1), waiter)
        .await
        .expect("increased limit wakes waiter")
        .expect("waiter task");
    assert_eq!(gate.active_count(), 2);
    drop((held, acquired));
    assert_eq!(gate.active_count(), 0);
}

#[tokio::test]
async fn unlimited_dynamic_semaphore_wakes_waiters_and_admits_more_than_eight() {
    let gate = Arc::new(DynamicSemaphore::new(1));
    let held = gate.try_acquire_owned().expect("initial slot");
    let waiter_gate = gate.clone();
    let waiter = tokio::spawn(async move { waiter_gate.acquire_owned().await });
    tokio::task::yield_now().await;

    gate.set_limit(0);
    let acquired = tokio::time::timeout(Duration::from_secs(1), waiter)
        .await
        .expect("unlimited mode wakes waiter")
        .expect("waiter task");
    let additional = (0..64)
        .map(|_| gate.try_acquire_owned().expect("unlimited slot"))
        .collect::<Vec<_>>();
    assert_eq!(gate.active_count(), 66);
    drop((held, acquired, additional));
    assert_eq!(gate.active_count(), 0);
}

#[tokio::test]
async fn global_gateway_limit_changes_keep_active_permits_and_unlimited_is_cancellation_safe() {
    let state = new_test_state().await;
    let first = state
        .gates
        .gateway_in_flight
        .try_acquire_owned()
        .expect("default gateway permit");
    let second = state
        .gates
        .gateway_in_flight
        .try_acquire_owned()
        .expect("second gateway permit");
    let reduced = OperationalSettings {
        gateway_max_in_flight: 1,
        ..state.operational_settings().settings
    };
    state.apply_operational_settings(OperationalSettingsRecord {
        settings: reduced,
        revision: 1,
        overridden: true,
    });
    assert_eq!(state.gates.gateway_in_flight.active_count(), 2);
    assert!(state.gates.gateway_in_flight.try_acquire_owned().is_none());
    drop(first);
    assert!(state.gates.gateway_in_flight.try_acquire_owned().is_none());
    drop(second);
    assert!(state.gates.gateway_in_flight.try_acquire_owned().is_some());

    let unlimited = OperationalSettings {
        gateway_max_in_flight: 0,
        ..reduced
    };
    state.apply_operational_settings(OperationalSettingsRecord {
        settings: unlimited,
        revision: 2,
        overridden: true,
    });
    let permits = (0..66)
        .map(|_| {
            state
                .gates
                .gateway_in_flight
                .try_acquire_owned()
                .expect("unlimited gateway permit")
        })
        .collect::<Vec<_>>();
    assert!(state.gates.gateway_in_flight.active_count() >= 66);
    drop(permits);

    let permit = state
        .gates
        .gateway_in_flight
        .try_acquire_owned()
        .expect("permit before cancellation");
    let task = tokio::spawn(async move {
        let _permit = permit;
        std::future::pending::<()>().await;
    });
    task.abort();
    assert!(task.await.is_err());
    assert_eq!(state.gates.gateway_in_flight.active_count(), 0);

    state
        .gates
        .gateway_in_flight
        .active
        .store(usize::MAX, Ordering::Release);
    assert!(state.gates.gateway_in_flight.try_acquire_owned().is_none());
    state
        .gates
        .gateway_in_flight
        .active
        .store(0, Ordering::Release);
}

#[tokio::test]
async fn circuit_reconfiguration_preserves_failures_and_retimes_open_circuits() {
    let state = new_test_state().await;
    for _ in 0..4 {
        state
            .permit_provider("provider")
            .expect("circuit below old threshold")
            .failed();
    }
    let lowered_threshold = OperationalSettings {
        circuit_breaker_threshold: 2,
        circuit_breaker_cooldown: Duration::from_secs(20),
        ..state.operational_settings().settings
    };
    state.apply_operational_settings(OperationalSettingsRecord {
        settings: lowered_threshold,
        revision: 1,
        overridden: true,
    });
    {
        let breakers = state
            .gates
            .circuit_breakers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let circuit = breakers.get("provider").expect("preserved circuit");
        assert_eq!(circuit.consecutive_failures, 4);
        assert!(circuit.open_until.is_none());
    }
    state
        .permit_provider_with_settings("provider", lowered_threshold)
        .expect("next transition still admits the request")
        .failed();
    let opened_at = {
        let breakers = state
            .gates
            .circuit_breakers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let circuit = breakers.get("provider").expect("open circuit");
        assert_eq!(circuit.consecutive_failures, 5);
        circuit.opened_at.expect("open timestamp")
    };

    let longer_cooldown = OperationalSettings {
        circuit_breaker_cooldown: Duration::from_secs(60),
        ..lowered_threshold
    };
    state.apply_operational_settings(OperationalSettingsRecord {
        settings: longer_cooldown,
        revision: 2,
        overridden: true,
    });
    let breakers = state
        .gates
        .circuit_breakers
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert_eq!(
        breakers
            .get("provider")
            .and_then(|circuit| circuit.open_until),
        opened_at.checked_add(Duration::from_secs(60))
    );
}

#[tokio::test]
async fn disabled_circuit_breaker_permits_do_not_mutate_preserved_state() {
    let state = new_test_state().await;
    {
        let mut breakers = state
            .gates
            .circuit_breakers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        breakers.insert(
            "provider".to_owned(),
            CircuitState {
                generation: 7,
                consecutive_failures: 4,
                open_until: Some(Instant::now() + Duration::from_secs(30)),
                opened_at: Some(Instant::now()),
                half_open_probe: Some(99),
            },
        );
    }

    let disabled = OperationalSettings {
        circuit_breaker_enabled: false,
        ..state.operational_settings().settings
    };
    let mut failed = state
        .permit_provider_with_settings("provider", disabled)
        .expect("disabled breaker admits request");
    failed.failed();
    let mut succeeded = state
        .permit_provider_with_settings("provider", disabled)
        .expect("disabled breaker admits request");
    succeeded.succeeded();

    let breakers = state
        .gates
        .circuit_breakers
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let circuit = breakers.get("provider").expect("preserved circuit");
    assert_eq!(circuit.generation, 7);
    assert_eq!(circuit.consecutive_failures, 4);
    assert!(circuit.open_until.is_some());
    assert_eq!(circuit.half_open_probe, Some(99));
}

#[tokio::test]
async fn old_circuit_permits_cannot_mutate_a_recreated_provider_state() {
    let state = new_test_state().await;
    let mut old_permit = state
        .permit_provider("provider")
        .expect("old provider permit");
    state.clear_provider_runtime_state("provider").await;
    let current_permit = state
        .permit_provider("provider")
        .expect("new provider permit");
    let current_generation = {
        let breakers = state
            .gates
            .circuit_breakers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        breakers
            .get("provider")
            .expect("new provider circuit")
            .generation
    };
    old_permit.failed();
    drop(current_permit);

    let breakers = state
        .gates
        .circuit_breakers
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let circuit = breakers.get("provider").expect("new provider circuit");
    assert_eq!(circuit.generation, current_generation);
    assert_eq!(circuit.consecutive_failures, 0);
    assert!(circuit.open_until.is_none());
}

#[tokio::test]
async fn late_closed_request_cannot_release_a_half_open_probe() {
    let state = new_test_state().await;
    let mut late_closed_permit = state
        .permit_provider("provider")
        .expect("closed circuit admits request");
    for _ in 0..5 {
        state
            .permit_provider("provider")
            .expect("closed circuit admits failures until threshold")
            .failed();
    }
    {
        let mut breakers = state
            .gates
            .circuit_breakers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let circuit = breakers.get_mut("provider").expect("opened circuit");
        assert!(circuit.open_until.is_some());
        circuit.open_until = Some(Instant::now() - Duration::from_millis(1));
    }

    let probe = state
        .permit_provider("provider")
        .expect("cooldown elapsed; one probe admitted");
    late_closed_permit.succeeded();
    assert!(state.permit_provider("provider").is_none());
    drop(probe);
    assert!(state.permit_provider("provider").is_some());
}

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

#[tokio::test]
async fn applying_resource_limits_updates_all_runtime_gates_without_revoking_active_work() {
    let database = TestDatabase::open().await;
    let state = AppState::new(database.config(), database.db.clone());
    let body_holds = (0..4)
        .map(|_| {
            state
                .gates
                .gateway_body_processing
                .try_acquire_owned()
                .expect("body slot")
        })
        .collect::<Vec<_>>();
    let sse_holds = (0..4)
        .map(|_| {
            state
                .gates
                .gateway_sse_processing
                .try_acquire_owned()
                .expect("SSE slot")
        })
        .collect::<Vec<_>>();
    let provider_holds = vec![
        state.try_provider_permit("p").await.expect("provider slot"),
        state.try_provider_permit("p").await.expect("provider slot"),
    ];
    let continuity_holds = (0..GatewayResourceLimits::DEFAULT_STREAM_CONTINUITY_MAX_CONCURRENCY)
        .map(|_| {
            state
                .gates
                .stream_continuity_in_flight
                .try_acquire_owned()
                .expect("continuity slot")
        })
        .collect::<Vec<_>>();

    let updated = GatewayResourceLimits {
        gateway_body_processing_concurrency: 2,
        provider_max_concurrency: 1,
        stream_continuity_max_concurrency: 2,
        ..GatewayResourceLimits::default()
    };
    state.apply_gateway_resource_limits(updated);

    assert_eq!(state.gateway_resource_limits(), updated);
    assert!(
        state
            .gates
            .gateway_body_processing
            .try_acquire_owned()
            .is_none()
    );
    assert!(
        state
            .gates
            .gateway_sse_processing
            .try_acquire_owned()
            .is_none()
    );
    assert!(state.try_provider_permit("p").await.is_none());
    assert!(
        state
            .gates
            .stream_continuity_in_flight
            .try_acquire_owned()
            .is_none()
    );

    drop(body_holds);
    drop(sse_holds);
    drop(provider_holds);
    drop(continuity_holds);
    assert!(
        state
            .gates
            .gateway_body_processing
            .try_acquire_owned()
            .is_some()
    );
    assert!(
        state
            .gates
            .gateway_sse_processing
            .try_acquire_owned()
            .is_some()
    );
    assert!(state.try_provider_permit("p").await.is_some());
    assert!(
        state
            .gates
            .stream_continuity_in_flight
            .try_acquire_owned()
            .is_some()
    );

    state.apply_gateway_resource_limits(GatewayResourceLimits {
        gateway_body_processing_concurrency:
            GatewayResourceLimits::UNLIMITED_GATEWAY_BODY_PROCESSING_CONCURRENCY,
        ..updated
    });
    let unlimited_body_slots = (0..64)
        .map(|_| {
            state
                .gates
                .gateway_body_processing
                .try_acquire_owned()
                .expect("unlimited body slot")
        })
        .collect::<Vec<_>>();
    let unlimited_sse_slots = (0..64)
        .map(|_| {
            state
                .gates
                .gateway_sse_processing
                .try_acquire_owned()
                .expect("unlimited SSE slot")
        })
        .collect::<Vec<_>>();
    drop((unlimited_body_slots, unlimited_sse_slots));
}

#[tokio::test]
async fn resource_limit_save_publishes_after_the_request_handle_is_dropped() {
    let state = new_test_state().await;
    let updated = GatewayResourceLimits {
        gateway_body_processing_concurrency: 3,
        provider_max_concurrency: 7,
        ..state.gateway_resource_limits()
    };
    let guard = state
        .runtime
        .gateway_resource_limits_update_lock
        .clone()
        .lock_owned()
        .await;
    let request_waiter = state.spawn_gateway_resource_limits_update(updated, guard);
    drop(request_waiter);

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let saved = crate::infra::db::load_gateway_resource_limits(&state.db)
                .await
                .expect("load persisted resource limits");
            if saved == updated && state.gateway_resource_limits() == updated {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("detached config update commits and applies after disconnect");
}

#[tokio::test]
async fn cancelling_a_waiting_dynamic_semaphore_acquire_leaks_no_slot() {
    let gate = Arc::new(DynamicSemaphore::new(1));
    let held = gate.try_acquire_owned().expect("initial slot");
    let waiter_gate = gate.clone();
    let waiter = tokio::spawn(async move { waiter_gate.acquire_owned().await });
    tokio::task::yield_now().await;
    waiter.abort();
    assert!(waiter.await.is_err());
    assert_eq!(gate.active_count(), 1);

    drop(held);
    assert!(gate.try_acquire_owned().is_some());
}
