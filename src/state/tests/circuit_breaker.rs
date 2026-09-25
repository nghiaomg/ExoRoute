use super::support::*;
use super::*;

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
