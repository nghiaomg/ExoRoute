use super::{
    ApiResult, AppState, OutputStylesInput, ResetOutputStylesInput, fail, internal_message,
};
use crate::support::output_styles::OutputStylesSnapshot;
use axum::{Json, extract::State, http::StatusCode};
use serde_json::{Value, json};

pub(crate) fn output_styles_response(snapshot: OutputStylesSnapshot) -> Value {
    json!({
        "styles": snapshot.styles.into_iter().map(|style| json!({
            "id": style.id.as_str(),
            "level": style.level.as_str(),
        })).collect::<Vec<_>>(),
        "revision": snapshot.revision,
        "overridden": snapshot.overridden,
        "source": if snapshot.overridden { "database" } else { "default" },
    })
}

pub(crate) async fn get_output_styles(State(state): State<AppState>) -> ApiResult {
    Ok(Json(output_styles_response(state.output_styles())))
}

pub(crate) async fn update_output_styles(
    State(state): State<AppState>,
    Json(input): Json<OutputStylesInput>,
) -> ApiResult {
    let (styles, expected_revision) = input
        .into_styles()
        .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    let snapshot = state
        .save_output_styles(styles, expected_revision, true)
        .await
        .map_err(internal_message)?
        .ok_or_else(|| {
            fail(
                StatusCode::CONFLICT,
                "Output styles changed in another session. Reload the latest values before saving.",
            )
        })?;
    Ok(Json(output_styles_response(snapshot)))
}

pub(crate) async fn reset_output_styles(
    State(state): State<AppState>,
    Json(input): Json<ResetOutputStylesInput>,
) -> ApiResult {
    if input.expected_revision < 0 || input.expected_revision == i64::MAX {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "output styles revision is invalid",
        ));
    }
    let snapshot = state
        .save_output_styles(Vec::new(), input.expected_revision, false)
        .await
        .map_err(internal_message)?
        .ok_or_else(|| {
            fail(
                StatusCode::CONFLICT,
                "Output styles changed in another session. Reload the latest values before resetting.",
            )
        })?;
    Ok(Json(output_styles_response(snapshot)))
}
