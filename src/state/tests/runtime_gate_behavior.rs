use super::support::*;
use super::*;

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
