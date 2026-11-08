//! Kiro refresh-token import endpoint.
//!
//! Kiro IDE and social sign-ins produce a refresh token the operator can paste
//! instead of running the AWS Builder ID device flow. The token is validated
//! once against the Kiro auth service before it is stored.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KiroImportRequest {
    /// The Kiro IDE / social refresh token.
    refresh_token: String,
}

pub(crate) async fn import_kiro_login(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
    Json(body): Json<KiroImportRequest>,
) -> ApiResult {
    let lookup_id = provider_id.clone();
    let adapter_id = state
        .db
        .read(move |transaction| {
            transaction
                .get::<crate::infra::storage::Record>(
                    crate::infra::storage::Table::Providers,
                    &lookup_id,
                )?
                .map(|provider| provider.text("adapter_id").map(str::to_owned))
                .transpose()
        })
        .await
        .map_err(internal)?
        .ok_or_else(|| fail(StatusCode::NOT_FOUND, "provider not found"))?;
    if adapter_id != provider_adapters::KIRO_ADAPTER_ID {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "this provider does not support Kiro login import",
        ));
    }
    if state.config.master_key.is_none() {
        return Err(fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "set EXOROUTE_MASTER_KEY before connecting a provider account",
        ));
    }
    match crate::provider_adapters::kiro::import_token(&state, &provider_id, &body.refresh_token)
        .await
    {
        Ok(account) => Ok(Json(json!({
            "status": "connected",
            "account": account,
        }))),
        Err(message) => Err(fail(StatusCode::BAD_REQUEST, message)),
    }
}
