//! Local RPM quota reads and target updates.
//!
//! Quota percentages are derived from the in-memory RPM snapshot; only the
//! configured target persists in the provider record.

use super::keys::unix_time_millis;
use super::validation::{adapter_has_local_quota_tracking, stored_local_rpm_target};
use super::*;
use crate::infra::storage::{Field, Record, StorageError, Table};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderQuotaTargetInput {
    pub(crate) local_rpm_target: u32,
}

#[derive(Debug, Serialize)]
struct ProviderQuotaView {
    provider_id: String,
    window_seconds: u64,
    requests_last_60_seconds: u64,
    target_rpm: u32,
    /// Percentage of the local RPM target still available: 0 means exhausted,
    /// 100 means no requests have consumed the target.
    remaining_percent: f64,
    /// Backward-compatible consumed percentage. Prefer `remaining_percent`.
    used_percent: f64,
    coverage_seconds: u64,
    sampled_at_ms: i64,
    status: &'static str,
    source: &'static str,
    nvidia_source: &'static str,
}

pub(crate) async fn provider_quota(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult {
    let provider_id = id.clone();
    let (provider, adapter_id) = state
        .db
        .read(move |transaction| {
            let provider = transaction.get::<Record>(Table::Providers, &provider_id)?;
            let Some(provider) = provider else {
                return Ok((None, None));
            };
            if provider_is_deleting(&provider)? {
                return Ok((None, None));
            }
            let adapter_id = provider.text("adapter_id")?.to_owned();
            Ok((Some(provider), Some(adapter_id)))
        })
        .await
        .map_err(internal)?;
    let Some(provider) = provider else {
        return Err(fail(StatusCode::NOT_FOUND, "provider not found"));
    };
    let adapter_id = adapter_id.ok_or_else(|| fail(StatusCode::NOT_FOUND, "provider not found"))?;
    if !adapter_has_local_quota_tracking(&adapter_id) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "this provider adapter does not support local quota tracking",
        ));
    }
    let target = stored_local_rpm_target(&provider, &adapter_id)
        .map_err(internal)?
        .ok_or_else(|| fail(StatusCode::BAD_REQUEST, "local quota target is unavailable"))?;
    let snapshot = state.local_quota_snapshot(&id).await;
    let used_percent = ((snapshot.requests_last_60s as f64 / target as f64) * 100.0).min(100.0);
    let remaining_percent = (100.0 - used_percent).clamp(0.0, 100.0);
    let status = match snapshot.status {
        crate::state::LocalQuotaTrackingStatus::Ready => "ready",
        crate::state::LocalQuotaTrackingStatus::WarmingUp => "warming_up",
        crate::state::LocalQuotaTrackingStatus::CapacityLimited => "capacity_limited",
    };
    Ok(Json(json!(ProviderQuotaView {
        provider_id: id,
        window_seconds: 60,
        requests_last_60_seconds: snapshot.requests_last_60s,
        target_rpm: target,
        remaining_percent,
        used_percent,
        coverage_seconds: snapshot.coverage_seconds,
        sampled_at_ms: unix_time_millis(),
        status,
        source: "exoroute_local",
        nvidia_source: "not_documented",
    })))
}

pub(crate) async fn update_provider_quota_target(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderQuotaTargetInput>,
) -> ApiResult {
    if input.local_rpm_target == 0 {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "local_rpm_target must be a positive integer",
        ));
    }

    let provider_id = id.clone();
    let target = input.local_rpm_target;
    state
        .db
        .write(move |transaction| {
            let mut provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            ensure_provider_not_deleting(&provider)?;
            let adapter_id = provider.text("adapter_id")?;
            if !adapter_has_local_quota_tracking(adapter_id) {
                return Err(StorageError::Invalid(
                    "this provider adapter does not support local quota tracking".to_owned(),
                ));
            }
            provider.insert("local_rpm_target", Field::I64(i64::from(target)));
            transaction.put(Table::Providers, &provider_id, &provider)?;
            Ok(())
        })
        .await
        .map_err(|error| match error {
            StorageError::NotFound => fail(StatusCode::NOT_FOUND, "provider not found"),
            StorageError::Conflict => fail(StatusCode::CONFLICT, "provider is being deleted"),
            StorageError::Invalid(message)
                if message == "this provider adapter does not support local quota tracking" =>
            {
                fail(StatusCode::BAD_REQUEST, message)
            }
            other => internal(other),
        })?;

    Ok(Json(
        json!({ "ok": true, "provider_id": id, "local_rpm_target": target }),
    ))
}
