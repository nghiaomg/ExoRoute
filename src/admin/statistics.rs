use super::*;

const DAY_MINUTES: i64 = 24 * 60;
const WEEK_MINUTES: i64 = 7 * DAY_MINUTES;
const MONTH_MINUTES: i64 = 30 * DAY_MINUTES;

#[derive(Debug, Deserialize)]
pub(super) struct StatisticsQuery {
    #[serde(default = "default_range")]
    range: String,
}

fn default_range() -> String {
    "1d".to_owned()
}

fn range_window(range: &str) -> Option<(&'static str, i64)> {
    match range {
        "1d" => Some(("1d", DAY_MINUTES)),
        "7d" => Some(("7d", WEEK_MINUTES)),
        "30d" => Some(("30d", MONTH_MINUTES)),
        _ => None,
    }
}

pub(super) async fn get_statistics(
    State(state): State<AppState>,
    Query(query): Query<StatisticsQuery>,
) -> ApiResult {
    let Some((range, minutes)) = range_window(query.range.as_str()) else {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "Statistics range must be 1d, 7d, or 30d.",
        ));
    };
    let snapshot =
        crate::infra::telemetry::statistics_snapshot(&state.db, range, minutes, &state.telemetry)
            .await
            .map_err(internal)?;
    Ok(Json(snapshot))
}

pub(super) async fn get_api_key_statistics(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<StatisticsQuery>,
) -> ApiResult {
    if id.is_empty()
        || id.len() > 128
        || id
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control() || byte == b'/')
    {
        return Err(fail(StatusCode::BAD_REQUEST, "API key ID is invalid"));
    }
    let Some((range, minutes)) = range_window(query.range.as_str()) else {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "Statistics range must be 1d, 7d, or 30d.",
        ));
    };
    let api_key_id = id.clone();
    let api_key_exists = state
        .db
        .read(move |transaction| {
            transaction.get::<crate::infra::storage::Record>(
                crate::infra::storage::Table::ApiKeys,
                &api_key_id,
            )
        })
        .await
        .map_err(internal)?
        .is_some();
    if !api_key_exists {
        return Err(fail(StatusCode::NOT_FOUND, "API key not found"));
    }
    let snapshot = crate::infra::telemetry::api_key_statistics_snapshot(
        &state.db,
        &id,
        range,
        minutes,
        &state.telemetry,
    )
    .await
    .map_err(internal)?;
    Ok(Json(snapshot))
}
