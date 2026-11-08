//! Antigravity CLI (`agy`) credential import endpoint.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AntigravityCliImportRequest {
    /// Pasted `agy` token-file JSON. When absent, the local CLI credential
    /// paths under the user's home directory are probed instead.
    #[serde(default)]
    content: Option<String>,
}

pub(crate) async fn import_antigravity_cli_login(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
    body: Option<Json<AntigravityCliImportRequest>>,
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
    if adapter_id != provider_adapters::ANTIGRAVITY_ADAPTER_ID {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "this provider does not support Antigravity CLI login import",
        ));
    }
    if state.config.master_key.is_none() {
        return Err(fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "set EXOROUTE_MASTER_KEY before connecting a provider account",
        ));
    }
    let content = body.and_then(|body| body.0.content);
    match provider_adapters::auth::antigravity::import_cli_login(
        &state,
        &provider_id,
        content.as_deref(),
    )
    .await
    {
        Ok(account) => Ok(Json(json!({
            "status": "connected",
            "account": account,
        }))),
        Err(message) => Err(fail(StatusCode::BAD_REQUEST, message)),
    }
}
