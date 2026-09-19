use super::*;
#[cfg(test)]
use crate::config::GatewayResourceLimits;
#[cfg(test)]
use crate::support::output_styles::{
    self as output_style_support, OutputStyleLevel, OutputStyleSelection,
};

mod models;
mod operational;
mod output_styles;
mod resource_limits;
#[cfg(test)]
use models::OutputStyleInputEntry;
use models::{
    GatewayResourceLimitsInput, OperationalSettingsInput, OutputStylesInput,
    ResetOperationalSettingsInput, ResetOutputStylesInput,
};
pub(super) use operational::{
    get_operational_settings, reset_operational_settings, update_operational_settings,
};
pub(super) use output_styles::{get_output_styles, reset_output_styles, update_output_styles};
pub(super) use resource_limits::{get_gateway_resource_limits, update_gateway_resource_limits};

pub(super) async fn settings(State(state): State<AppState>) -> ApiResult {
    let stored_auth = crate::infra::db::has_admin_auth(&state.db)
        .await
        .map_err(internal)?;
    let must_change_password = crate::infra::db::admin_password_change_required(&state.db)
        .await
        .map_err(internal)?
        .unwrap_or(false);
    let admin_password_configured = stored_auth || state.admin_key.read().await.is_some();
    let operational = state.operational_settings().settings;
    Ok(Json(
        json!({"host":state.config.bind.ip().to_string(),"port":state.config.bind.port(),"database_path":state.config.database_path.to_string_lossy(),"admin_password_configured":admin_password_configured,"must_change_password":must_change_password,"master_key_configured":state.config.master_key.is_some(),"connect_timeout_ms":operational.connect_timeout.as_millis(),"request_timeout_ms":operational.request_timeout.as_millis(),"circuit_breaker_enabled":operational.circuit_breaker_enabled}),
    ))
}

#[cfg(test)]
mod resource_limit_tests {
    use super::*;
    use crate::{state::AppState, support::test_support::TestDatabase};

    fn resource_limits_input(
        provider_max_concurrency: u64,
        confirm_unlimited_provider_concurrency: bool,
        gateway_body_limit_mib: u64,
    ) -> GatewayResourceLimitsInput {
        GatewayResourceLimitsInput {
            gateway_body_limit_mib,
            gateway_body_processing_concurrency: 4,
            sse_frame_limit_kib: 1024,
            sse_buffer_limit_kib: 2048,
            provider_max_concurrency,
            confirm_unlimited_provider_concurrency,
            stream_continuity_enabled: None,
            stream_continuity_max_concurrency: None,
            expected_saved: None,
        }
    }

    #[tokio::test]
    async fn provider_unlimited_requires_acknowledgement_then_persists_and_applies_immediately() {
        let database = TestDatabase::open().await;
        let state = AppState::new(database.config(), database.db.clone());

        let response = update_gateway_resource_limits(
            State(state.clone()),
            Json(resource_limits_input(
                GatewayResourceLimits::UNLIMITED_PROVIDER_MAX_CONCURRENCY as u64,
                false,
                16,
            )),
        )
        .await;
        let (status, _) = response.expect_err("unlimited requires explicit confirmation");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            crate::infra::db::load_gateway_resource_limits(&state.db)
                .await
                .expect("load unchanged resource limits")
                .provider_max_concurrency,
            GatewayResourceLimits::DEFAULT_PROVIDER_MAX_CONCURRENCY
        );

        update_gateway_resource_limits(
            State(state.clone()),
            Json(resource_limits_input(
                GatewayResourceLimits::UNLIMITED_PROVIDER_MAX_CONCURRENCY as u64,
                true,
                16,
            )),
        )
        .await
        .map(|_| ())
        .expect("confirmed unlimited setting is saved and applied");
        assert_eq!(
            crate::infra::db::load_gateway_resource_limits(&state.db)
                .await
                .expect("load saved resource limits")
                .provider_max_concurrency,
            GatewayResourceLimits::UNLIMITED_PROVIDER_MAX_CONCURRENCY
        );
        assert_eq!(
            state.gateway_resource_limits().provider_max_concurrency,
            GatewayResourceLimits::UNLIMITED_PROVIDER_MAX_CONCURRENCY
        );

        update_gateway_resource_limits(
            State(state.clone()),
            Json(resource_limits_input(
                GatewayResourceLimits::UNLIMITED_PROVIDER_MAX_CONCURRENCY as u64,
                false,
                15,
            )),
        )
        .await
        .map(|_| ())
        .expect("unrelated saves do not repeatedly require unlimited confirmation");
        assert_eq!(
            state.gateway_resource_limits().gateway_body_limit_bytes,
            15 * 1024 * 1024
        );

        let mut permits = Vec::with_capacity(64);
        for _ in 0..64 {
            permits.push(
                state
                    .try_provider_permit("provider")
                    .await
                    .expect("confirmed unlimited provider gate admits requests"),
            );
        }
    }

    #[tokio::test]
    async fn continuity_capacity_is_configurable_but_zero_is_rejected() {
        let database = TestDatabase::open().await;
        let state = AppState::new(database.config(), database.db.clone());

        let mut input = resource_limits_input(32, false, 16);
        input.stream_continuity_max_concurrency = Some(64);
        update_gateway_resource_limits(State(state.clone()), Json(input))
            .await
            .map(|_| ())
            .expect("finite continuity capacity is accepted");
        assert_eq!(
            state
                .gateway_resource_limits()
                .stream_continuity_max_concurrency,
            64
        );

        let mut permits = Vec::with_capacity(64);
        for _ in 0..64 {
            permits.push(
                state
                    .gates
                    .stream_continuity_in_flight
                    .try_acquire_owned()
                    .expect("configured continuity capacity admits work"),
            );
        }
        assert!(
            state
                .gates
                .stream_continuity_in_flight
                .try_acquire_owned()
                .is_none()
        );
        drop(permits);

        let mut invalid = resource_limits_input(32, false, 16);
        invalid.stream_continuity_max_concurrency = Some(0);
        let (status, _) = update_gateway_resource_limits(State(state.clone()), Json(invalid))
            .await
            .expect_err("zero continuity capacity must be rejected");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            crate::infra::db::load_gateway_resource_limits(&state.db)
                .await
                .expect("load unchanged resource limits")
                .stream_continuity_max_concurrency,
            64
        );
    }
}

#[cfg(test)]
mod output_styles_settings_tests {
    use super::*;
    use crate::{state::AppState, support::test_support::TestDatabase};

    #[test]
    fn output_styles_input_is_strict_and_normalizes_catalog_order() {
        let input: OutputStylesInput = serde_json::from_value(json!({
            "styles": [
                { "id": "ponytail", "level": "ultra" },
                { "id": "terse-prose", "level": "lite" }
            ],
            "expected_revision": 7
        }))
        .expect("valid output style payload");
        let (styles, revision) = input.into_styles().expect("valid styles");
        assert_eq!(revision, 7);
        assert_eq!(
            styles[0].id,
            output_style_support::OutputStyleId::TerseProse
        );
        assert_eq!(styles[1].id, output_style_support::OutputStyleId::Ponytail);

        assert!(
            serde_json::from_value::<OutputStylesInput>(json!({
                "styles": [],
                "expected_revision": 0,
                "unrecognized": true
            }))
            .is_err()
        );
        for payload in [
            json!({
                "styles": [{ "id": " ponytail", "level": "full" }],
                "expected_revision": 0
            }),
            json!({
                "styles": [{ "id": "unknown", "level": "full" }],
                "expected_revision": 0
            }),
            json!({
                "styles": [{ "id": "ponytail", "level": "unknown" }],
                "expected_revision": 0
            }),
            json!({
                "styles": [
                    { "id": "ponytail", "level": "full" },
                    { "id": "ponytail", "level": "lite" }
                ],
                "expected_revision": 0
            }),
            json!({ "styles": [], "expected_revision": i64::MAX }),
            json!({
                "styles": [
                    { "id": "terse-prose", "level": "full" },
                    { "id": "less-code", "level": "full" },
                    { "id": "ponytail", "level": "full" },
                    { "id": "ponytail", "level": "ultra" }
                ],
                "expected_revision": 0
            }),
        ] {
            assert!(
                serde_json::from_value::<OutputStylesInput>(payload)
                    .expect("known payload shape")
                    .into_styles()
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn output_styles_settings_commit_apply_immediately_and_reject_stale_revision() {
        let database = TestDatabase::open().await;
        let state = AppState::new(database.config(), database.db.clone());
        let input = OutputStylesInput {
            styles: vec![OutputStyleInputEntry {
                id: "less-code".to_owned(),
                level: "full".to_owned(),
            }],
            expected_revision: 0,
        };
        let Json(saved) = update_output_styles(State(state.clone()), Json(input))
            .await
            .expect("save output styles");
        assert_eq!(saved["revision"], 1);
        assert_eq!(saved["source"], "database");
        assert_eq!(state.output_styles().revision, 1);
        assert_eq!(
            crate::infra::db::load_output_styles(&state.db)
                .await
                .expect("read committed styles"),
            state.output_styles()
        );

        let stale = OutputStylesInput {
            styles: Vec::new(),
            expected_revision: 0,
        };
        let (status, _) = update_output_styles(State(state.clone()), Json(stale))
            .await
            .expect_err("stale revision conflicts");
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(state.output_styles().revision, 1);
    }

    #[tokio::test]
    async fn output_styles_commit_and_publish_survive_caller_cancellation() {
        let database = TestDatabase::open().await;
        let state = AppState::new(database.config(), database.db.clone());
        let database_for_blocker = state.db.clone();
        let (writer_started_sender, mut writer_started_receiver) =
            tokio::sync::watch::channel(false);
        let writer_barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let barrier_for_writer = writer_barrier.clone();
        let blocker = tokio::spawn(async move {
            database_for_blocker
                .write(move |_| {
                    let _ = writer_started_sender.send(true);
                    barrier_for_writer.wait();
                    barrier_for_writer.wait();
                    Ok(())
                })
                .await
        });
        // Hold the LMDB writer transaction while the output-style worker
        // queues its commit, then cancel only the HTTP-side waiter.
        writer_started_receiver
            .changed()
            .await
            .expect("writer blocker transaction starts");
        writer_barrier.wait();
        let request_state = state.clone();
        let request = tokio::spawn(async move {
            request_state
                .save_output_styles(
                    vec![OutputStyleSelection {
                        id: output_style_support::OutputStyleId::Ponytail,
                        level: OutputStyleLevel::Ultra,
                    }],
                    0,
                    true,
                )
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if state.runtime.output_styles_update_lock.try_lock().is_err() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("detached update worker acquires its serialization lock");
        request.abort();
        assert!(request.await.is_err());
        writer_barrier.wait();
        blocker
            .await
            .expect("writer blocker task")
            .expect("writer blocker commits");

        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let persisted = crate::infra::db::load_output_styles(&state.db)
                    .await
                    .expect("read output styles after cancelled request");
                if persisted.revision == 1 && state.output_styles() == persisted {
                    break persisted;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("detached update commits and publishes after caller cancellation");
    }
}
