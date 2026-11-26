//! Dashboard overview counters.
//!
//! Reads provider/combo/request totals in a single bounded transaction so
//! the overview card never issues per-table fan-out.

use super::errors::{ApiResult, internal};
use crate::state::AppState;
use axum::{Json, extract::State};
use serde_json::{Value, json};

pub(crate) const MAX_OVERVIEW_COMBO_COUNT: usize = 10_000;

pub(crate) async fn overview(State(state): State<AppState>) -> ApiResult {
    Ok(Json(overview_snapshot(&state).await?))
}

pub(super) async fn overview_snapshot(
    state: &AppState,
) -> Result<Value, (axum::http::StatusCode, Json<Value>)> {
    let (providers, combos, requests) = state
        .db
        .read(|transaction| {
            let combo_indexes = transaction.scan_prefix::<String>(
                crate::infra::storage::Table::RouteNameIndex,
                "name/",
                MAX_OVERVIEW_COMBO_COUNT.saturating_add(1),
            )?;
            if combo_indexes.len() > MAX_OVERVIEW_COMBO_COUNT {
                return Err(crate::infra::storage::StorageError::Busy);
            }
            let combo_count = i64::try_from(combo_indexes.len()).map_err(|_| {
                crate::infra::storage::StorageError::Invalid(
                    "combo count is outside the supported range".to_owned(),
                )
            })?;
            Ok((
                transaction
                    .get::<i64>(crate::infra::storage::Table::Meta, "provider_count")?
                    .unwrap_or(0),
                combo_count,
                transaction
                    .get::<i64>(crate::infra::storage::Table::Meta, "request_log_count")?
                    .unwrap_or(0),
            ))
        })
        .await
        .map_err(internal)?;
    Ok(
        json!({"provider_count":providers,"combo_count":combos,"route_count":combos,"request_count":requests,"dropped_request_logs":state.telemetry.drop_counters().request_logs(),"uptime_seconds":state.started_at.elapsed().as_secs()}),
    )
}

#[cfg(test)]
mod tests {
    use super::overview;
    use crate::{
        infra::storage::{Table, WriteTxn},
        state::AppState,
        support::test_support::TestDatabase,
    };
    use axum::Json;
    use axum::extract::State as AxumState;

    #[tokio::test]
    async fn overview_counts_indexed_combos_when_route_metadata_is_stale() {
        let database = TestDatabase::open().await;
        database
            .db
            .write(|transaction: &mut WriteTxn<'_, '_>| {
                for (id, name) in [("combo-1", "First combo"), ("combo-2", "Second combo")] {
                    let index_key = format!("name/{name}\u{1}{id}");
                    transaction.put(Table::RouteNameIndex, &index_key, &id.to_owned())?;
                }
                Ok(())
            })
            .await
            .expect("seed indexed combos");

        let state = AppState::new(database.config(), database.db.clone());
        let Json(body) = overview(AxumState(state)).await.expect("overview responds");

        assert_eq!(body["combo_count"], 2);
        assert_eq!(body["route_count"], 2);
    }
}
