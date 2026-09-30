//! Dashboard workspace chat relay: one-off model conversations for the admin.
//!
//! The workspace chat is an interactive scratchpad: the dashboard sends a
//! system prompt, message text, optional inline images and documents, and
//! per-request thinking preferences to one saved provider model. Unlike the
//! gateway pipeline this endpoint writes nothing to storage: no request log,
//! no telemetry event, no usage meter, and no credential state change. The
//! only side effect is the upstream inference call itself, so a conversation
//! never reaches the LMDB environment.
//!
//! Handlers live here; target preparation, credential resolution, and the
//! model picker listing live in `target.rs`.

use super::*;
use crate::protocol::ContentBlock;
use serde_json::{Value, json};
use std::time::Instant;

mod input;
mod target;

use input::{parse_chat_attachments, parse_chat_target_ids, parse_chat_text_input};
use target::{prepare_chat_turn, resolve_turn_credential, workspace_model_options};

/// Router body limit for the chat POST: attachments are inlined as base64, so
/// the route needs more headroom than the default 2 MiB admin body limit while
/// staying an order of magnitude below the gateway's 16 MiB request ceiling.
pub(crate) const MAX_WORKSPACE_CHAT_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Relays one dashboard chat message to the selected provider model and
/// returns the decoded assistant reply. This handler persists nothing: no
/// request log, no telemetry, no usage meter, and no credential state change,
/// which is the contract the workspace chat promises the admin.
pub(crate) async fn send_workspace_chat(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> ApiResult {
    let (provider_id, model) = parse_chat_target_ids(&input)?;
    let text = parse_chat_text_input(&input)?;
    let attachments = parse_chat_attachments(&input)?;
    let turn = prepare_chat_turn(&state, &provider_id, &model, &text, &attachments).await?;
    let credential = resolve_turn_credential(&state, &turn).await?;
    let auth = credential.as_turn_auth();
    let started = Instant::now();
    let outcome = provider_adapters::relay_workspace_inference(
        &turn.adapter_id,
        provider_adapters::AdapterWorkspaceChatRequest {
            state: &state,
            base_url: &turn.base_url,
            provider_id: &turn.provider_id,
            model: &turn.model,
            auth_type: &turn.auth_type,
            auth_header: turn.auth_header.as_deref(),
            custom_headers: &turn.custom_headers,
            protocol: turn.client_protocol,
            secret: auth.secret,
            oauth_auth: auth.oauth_auth,
            body: turn.upstream_body,
        },
    )
    .await;
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            return Err(fail(
                error.status.unwrap_or(StatusCode::BAD_GATEWAY),
                error.message,
            ));
        }
    };
    let duration_ms = started.elapsed().as_millis() as i64;
    let decoded =
        crate::protocol::decode_upstream_response(turn.upstream_protocol, outcome.body(), &model)
            .map_err(|error| {
            fail(
                StatusCode::BAD_GATEWAY,
                format!("could not decode provider response: {error}"),
            )
        })?;
    let reply = decoded
        .message
        .content
        .iter()
        .filter_map(|block| {
            if let ContentBlock::Text { text } = block {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("");
    let reasoning = decoded
        .message
        .content
        .iter()
        .filter_map(|block| {
            if let ContentBlock::Reasoning { text } = block {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Json(json!({
        "reply": reply,
        "reasoning": if reasoning.is_empty() { None } else { Some(reasoning) },
        "model": decoded.model,
        "finish_reason": decoded.finish_reason,
        "usage": decoded.usage.as_ref().map(|usage| json!({
            "input_tokens": usage.input_tokens,
            "output_tokens": usage.output_tokens,
        })),
        "duration_ms": duration_ms,
    })))
}

/// Lists the workspace model picker options in `{prefix}/{model}` form,
/// mirroring the gateway's public alias resolution. Read-only and bounded.
pub(crate) async fn workspace_chat_models(State(state): State<AppState>) -> ApiResult {
    let (models, truncated) = workspace_model_options(&state).await?;
    let models = models
        .into_iter()
        .map(|option| {
            json!({
                "provider_id": option.provider_id,
                "model": option.model,
                "id": option.id,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({"models": models, "truncated": truncated})))
}

#[cfg(test)]
mod tests;
