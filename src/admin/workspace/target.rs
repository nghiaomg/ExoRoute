//! Workspace chat target preparation: saved model lookup, upstream protocol
//! resolution, thinking behavior, credential resolution, and canonical request
//! building for one dashboard chat turn.
//!
//! Everything here is read-only. Credentials are decrypted in memory and no
//! failure, usage, or timestamp state is ever written, which is the
//! no-persistence contract the workspace chat promises the admin.

use super::*;

use super::input::{ChatAttachment, ChatTextInput, ChatThinkingMode, fail_request};
use crate::infra::storage::{Record, StorageError, Table};
use crate::protocol::{
    CanonicalRequest, ContentBlock, DEFAULT_THINKING_MODE, DocumentSource, Message, Protocol, Role,
    ThinkingHandling, UpstreamProtocol, parse_thinking_handling,
};
use crate::provider_adapters;
use crate::provider_adapters::keys::ProviderApiKeyCursor;
use crate::security::decrypt_secret;
use serde_json::Value;

/// Bounded model-picker listing. The picker is advisory navigation, not a
/// catalog export, so the scan stops instead of materializing every row.
const MAX_WORKSPACE_PROVIDER_ROWS: usize = 2_000;
const MAX_WORKSPACE_MODELS_PER_PROVIDER: usize = 200;
const MAX_WORKSPACE_MODELS_TOTAL: usize = 20_000;

/// The saved provider row, whether the model is saved, and the model's saved
/// protocol override, read in one bounded transaction.
pub(super) struct ProviderChatTarget {
    provider: Record,
    model_saved: bool,
    saved_model_protocol: Option<UpstreamProtocol>,
}

/// Loads the saved provider row, the saved-model flag, and the model's saved
/// protocol override in one bounded read transaction.
pub(super) async fn load_provider_chat_target(
    state: &AppState,
    provider_id: &str,
    model: &str,
) -> Result<Option<ProviderChatTarget>, (StatusCode, Json<Value>)> {
    let provider_id = provider_id.to_owned();
    let model = model.to_owned();
    let loaded = state
        .db
        .read(move |transaction| {
            let Some(provider) = transaction.get::<Record>(Table::Providers, &provider_id)? else {
                return Ok(None);
            };
            if super::super::providers::provider_is_deleting(&provider)? {
                return Ok(None);
            }
            let index_key = crate::infra::db::provider_model_index_key(&provider_id, &model)?;
            let model_saved = transaction
                .get::<String>(Table::ProviderModelIndex, &index_key)?
                .is_some_and(|stored| stored == model);
            let saved_model_protocol = if model_saved {
                let model_key = crate::infra::db::provider_model_key(&provider_id, &model)?;
                transaction
                    .get::<Record>(Table::ProviderModels, &model_key)?
                    .and_then(|record| {
                        record
                            .optional_text("upstream_protocol")
                            .ok()
                            .flatten()
                            .and_then(|value| value.parse::<UpstreamProtocol>().ok())
                            .map(|protocol| protocol.to_owned())
                    })
            } else {
                None
            };
            Ok(Some(ProviderChatTarget {
                provider,
                model_saved,
                saved_model_protocol,
            }))
        })
        .await
        .map_err(internal)?;
    Ok(loaded)
}

/// Resolves the upstream protocol for one provider model with the same
/// precedence the gateway uses: saved per-model override, then the adapter's
/// static model mapping, then the provider's preferred protocol.
fn resolve_chat_upstream_protocol(
    provider: &Record,
    saved_model_protocol: Option<UpstreamProtocol>,
    model: &str,
) -> Result<UpstreamProtocol, (StatusCode, Json<Value>)> {
    let adapter_id = provider.text("adapter_id").map_err(internal)?.to_owned();
    let preferred = provider
        .text("preferred_protocol")
        .map_err(internal)?
        .to_owned();
    saved_model_protocol
        .or_else(|| provider_adapters::model_upstream_protocol(&adapter_id, model))
        .or_else(|| preferred.parse::<UpstreamProtocol>().ok())
        .ok_or_else(|| {
            fail_request(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!(
                    "provider '{}' model '{model}' has no configured upstream protocol; set it in the provider model settings",
                    provider.text("id").unwrap_or_default()
                ),
            )
        })
}

/// Builds the canonical request for one workspace turn. Attachments are
/// inlined as base64 so the relay never fetches remote content.
fn build_chat_request(text: &ChatTextInput, attachments: &[ChatAttachment]) -> CanonicalRequest {
    let mut content = Vec::with_capacity(attachments.len().saturating_add(1));
    for attachment in attachments {
        if attachment.is_image {
            content.push(ContentBlock::Image {
                url: format!(
                    "data:{};base64,{}",
                    attachment.media_type, attachment.base64_data
                ),
                detail: None,
            });
        } else {
            content.push(ContentBlock::Document {
                source: DocumentSource::Base64 {
                    media_type: attachment.media_type.clone(),
                    data: attachment.base64_data.clone(),
                },
                filename: attachment.filename.clone(),
            });
        }
    }
    content.push(ContentBlock::Text {
        text: text.message.clone(),
    });
    let mut messages = Vec::with_capacity(2);
    if let Some(system_prompt) = &text.system_prompt {
        messages.push(Message {
            role: Role::System,
            content: vec![ContentBlock::Text {
                text: system_prompt.clone(),
            }],
            name: None,
        });
    }
    messages.push(Message {
        role: Role::User,
        content,
        name: None,
    });
    CanonicalRequest {
        source_protocol: Protocol::ChatCompletions,
        model: String::new(),
        messages,
        tools: Vec::new(),
        tool_choice: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        stop: Vec::new(),
        stream: false,
        metadata: std::collections::BTreeMap::new(),
        output_styles_applied: false,
    }
}

/// Chooses the thinking behavior for this turn as an owned value: an
/// explicit workspace choice wins, otherwise the provider's stored settings
/// apply unchanged. Validation reuses the protocol's canonical parser.
enum ChatThinkingChoice {
    Preserve,
    Remove,
    Override(String),
}

fn chat_thinking_handling(
    provider: &Record,
    text: &ChatTextInput,
) -> Result<ChatThinkingChoice, (StatusCode, Json<Value>)> {
    match text.thinking_mode {
        ChatThinkingMode::Default => {
            let mode = provider
                .optional_text("thinking_mode")
                .map_err(internal)?
                .unwrap_or(DEFAULT_THINKING_MODE)
                .to_owned();
            let override_text = provider
                .optional_text("thinking_override")
                .map_err(internal)?
                .map(str::to_owned);
            match parse_thinking_handling(&mode, override_text.as_deref())
                .map_err(|message| fail_request(StatusCode::BAD_REQUEST, message))?
            {
                ThinkingHandling::Preserve => Ok(ChatThinkingChoice::Preserve),
                ThinkingHandling::Remove => Ok(ChatThinkingChoice::Remove),
                ThinkingHandling::Override(text) => {
                    Ok(ChatThinkingChoice::Override(text.to_owned()))
                }
            }
        }
        ChatThinkingMode::Preserve => Ok(ChatThinkingChoice::Preserve),
        ChatThinkingMode::Remove => Ok(ChatThinkingChoice::Remove),
        ChatThinkingMode::Override => Ok(ChatThinkingChoice::Override(
            text.thinking_override.clone().unwrap_or_default(),
        )),
    }
}

/// The credential one chat request uses. Resolution is read-only: keys are
/// decrypted in memory and no failure, usage, or timestamp state is written.
pub(super) enum WorkspaceCredential {
    None,
    ApiKey(String),
    OAuth(provider_adapters::OAuthRequestAuth),
}

/// Resolves one usable credential in the provider's fallback order.
async fn resolve_chat_credential(
    state: &AppState,
    provider: &Record,
    provider_id: &str,
    adapter_id: &str,
    auth_type: &str,
) -> Result<WorkspaceCredential, (StatusCode, Json<Value>)> {
    if auth_type == "none" {
        return Ok(WorkspaceCredential::None);
    }
    let (mut cursor, mut current) = match ProviderApiKeyCursor::start(&state.db, provider_id)
        .await
        .map_err(internal)?
    {
        Some((cursor, first_key)) => (Some(cursor), Some(first_key)),
        None => (None, None),
    };
    // Visit the fallback order once. Every step decrypts in memory only; the
    // loop ends when the cursor is exhausted, like the gateway credential loop.
    while let Some(key) = current.take() {
        match key.credential_type {
            crate::provider_adapters::keys::ProviderCredentialType::OAuth => {
                if let Some(auth) =
                    provider_adapters::resolve_oauth_request_auth(adapter_id, state, &key.id)
                        .await
                        .map_err(|message| fail_request(StatusCode::BAD_GATEWAY, message))?
                {
                    return Ok(WorkspaceCredential::OAuth(auth));
                }
            }
            crate::provider_adapters::keys::ProviderCredentialType::ApiKey => {
                if let Some(secret) = decrypt_secret(
                    state.config.master_key.as_ref(),
                    Some(&key.encrypted_secret),
                )
                .map_err(|message| fail_request(StatusCode::INTERNAL_SERVER_ERROR, message))?
                {
                    return Ok(WorkspaceCredential::ApiKey(secret));
                }
            }
        }
        current = match cursor.as_mut() {
            Some(cursor) => cursor.next(&state.db).await.map_err(internal)?,
            None => None,
        };
    }
    provider_secret_credential(state, provider, auth_type)
}

/// Falls back to the provider's own configured secret when no stored
/// credential can serve the chat.
fn provider_secret_credential(
    state: &AppState,
    provider: &Record,
    auth_type: &str,
) -> Result<WorkspaceCredential, (StatusCode, Json<Value>)> {
    let legacy_secret = provider.optional_bytes("secret").map_err(internal)?;
    let secret = decrypt_secret(state.config.master_key.as_ref(), legacy_secret)
        .map_err(|message| fail_request(StatusCode::INTERNAL_SERVER_ERROR, message))?;
    match secret {
        Some(secret) => Ok(WorkspaceCredential::ApiKey(secret)),
        None if auth_type == "none" => Ok(WorkspaceCredential::None),
        None => Err(fail_request(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider has no usable credential; add an API key or connect an account first",
        )),
    }
}

/// The fully prepared context for one workspace chat relay: everything the
/// dispatcher needs to encode, send, and decode one upstream inference.
pub(super) struct PreparedChatTurn {
    pub(super) provider_id: String,
    pub(super) model: String,
    pub(super) upstream_protocol: UpstreamProtocol,
    pub(super) client_protocol: Protocol,
    pub(super) base_url: String,
    pub(super) auth_type: String,
    pub(super) auth_header: Option<String>,
    pub(super) adapter_id: String,
    pub(super) custom_headers: std::collections::BTreeMap<String, String>,
    pub(super) upstream_body: Value,
}

/// Prepares everything about one chat turn that can fail before the upstream
/// call. Reads provider configuration only; nothing is persisted.
pub(super) async fn prepare_chat_turn(
    state: &AppState,
    provider_id: &str,
    model: &str,
    text: &ChatTextInput,
    attachments: &[ChatAttachment],
) -> Result<PreparedChatTurn, (StatusCode, Json<Value>)> {
    let Some(target) = load_provider_chat_target(state, provider_id, model).await? else {
        return Err(fail_request(
            StatusCode::NOT_FOUND,
            "provider was not found or is being deleted",
        ));
    };
    let provider = target.provider;
    if !provider.boolean("enabled").map_err(internal)? {
        return Err(fail_request(
            StatusCode::BAD_REQUEST,
            "provider is disabled; enable it before chatting with its models",
        ));
    }
    if !target.model_saved {
        return Err(fail_request(
            StatusCode::NOT_FOUND,
            "model is not saved for this provider",
        ));
    }
    let adapter_id = provider.text("adapter_id").map_err(internal)?.to_owned();
    if provider_adapters::adapter(&adapter_id).is_none() {
        return Err(fail_request(
            StatusCode::BAD_REQUEST,
            "provider uses an unregistered adapter",
        ));
    }
    let upstream_protocol =
        resolve_chat_upstream_protocol(&provider, target.saved_model_protocol, model)?;
    if !provider_adapters::supports_upstream_protocol(&adapter_id, upstream_protocol) {
        return Err(fail_request(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "provider '{provider_id}' adapter does not support {}",
                upstream_protocol.as_str()
            ),
        ));
    }
    let declared_protocols = provider
        .text("supported_protocols")
        .map_err(internal)?
        .to_owned();
    let declared = serde_json::from_str::<Vec<String>>(&declared_protocols)
        .map_err(|error| internal(StorageError::Codec(error.to_string())))?;
    if !declared
        .iter()
        .any(|value| value == upstream_protocol.as_str())
    {
        return Err(fail_request(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "provider '{provider_id}' does not declare support for {}",
                upstream_protocol.as_str()
            ),
        ));
    }
    let Some(client_protocol) = upstream_protocol.client_protocol() else {
        return Err(fail_request(
            StatusCode::UNPROCESSABLE_ENTITY,
            "provider upstream protocol cannot carry a chat request",
        ));
    };
    let thinking_choice = chat_thinking_handling(&provider, text)?;
    let thinking_handling = match &thinking_choice {
        ChatThinkingChoice::Preserve => ThinkingHandling::Preserve,
        ChatThinkingChoice::Remove => ThinkingHandling::Remove,
        ChatThinkingChoice::Override(text) => ThinkingHandling::Override(text.as_str()),
    };
    let canonical = build_chat_request(text, attachments);
    let mut upstream_body = crate::protocol::encode_upstream_request_with_thinking(
        upstream_protocol,
        &canonical,
        model,
        thinking_handling,
    )
    .map_err(|error| fail_request(StatusCode::UNPROCESSABLE_ENTITY, error))?;
    if let Err(error) =
        provider_adapters::prepare_request_body(&adapter_id, &mut upstream_body, "workspace-chat")
    {
        return Err(fail_request(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("provider adapter rejected the request: {error}"),
        ));
    }
    if let Err(error) =
        provider_adapters::prepare_provider_images(&adapter_id, state, &mut upstream_body).await
    {
        return Err(fail_request(
            StatusCode::BAD_GATEWAY,
            format!("provider could not prepare adapter input: {error}"),
        ));
    }
    let base_url = provider.text("base_url").map_err(internal)?.to_owned();
    let auth_type = provider.text("auth_type").map_err(internal)?.to_owned();
    let auth_header = provider
        .optional_text("auth_header")
        .map_err(internal)?
        .map(str::to_owned);
    let custom_headers =
        super::super::providers::stored_custom_headers(&provider, state.config.master_key.as_ref())
            .map_err(|message| internal(StorageError::Codec(message)))?;
    Ok(PreparedChatTurn {
        provider_id: provider_id.to_owned(),
        model: model.to_owned(),
        upstream_protocol,
        client_protocol,
        base_url,
        auth_type,
        auth_header,
        adapter_id,
        custom_headers,
        upstream_body,
    })
}

/// Resolves the credential for a prepared turn. Separated from
/// [`prepare_chat_turn`] because the secret must not outlive the send.
pub(super) async fn resolve_turn_credential(
    state: &AppState,
    turn: &PreparedChatTurn,
) -> Result<WorkspaceCredential, (StatusCode, Json<Value>)> {
    let provider = state
        .db
        .read({
            let provider_id = turn.provider_id.clone();
            move |transaction| transaction.get::<Record>(Table::Providers, &provider_id)
        })
        .await
        .map_err(internal)?
        .ok_or_else(|| {
            fail_request(
                StatusCode::NOT_FOUND,
                "provider was not found or is being deleted",
            )
        })?;
    resolve_chat_credential(
        state,
        &provider,
        &turn.provider_id,
        &turn.adapter_id,
        &turn.auth_type,
    )
    .await
}

/// The secret material for one send, extracted from a resolved credential.
pub(super) struct TurnAuth<'a> {
    pub(super) secret: Option<&'a str>,
    pub(super) oauth_auth: Option<&'a provider_adapters::OAuthRequestAuth>,
}

impl WorkspaceCredential {
    pub(super) fn as_turn_auth(&self) -> TurnAuth<'_> {
        match self {
            WorkspaceCredential::ApiKey(secret) => TurnAuth {
                secret: Some(secret.as_str()),
                oauth_auth: None,
            },
            WorkspaceCredential::OAuth(auth) => TurnAuth {
                secret: None,
                oauth_auth: Some(auth),
            },
            WorkspaceCredential::None => TurnAuth {
                secret: None,
                oauth_auth: None,
            },
        }
    }
}

/// Lists the model picker options: every enabled provider with its saved
/// models in `{prefix}/{model}` form, mirroring the gateway's public alias
/// resolution. Read-only and bounded.
pub(super) fn workspace_model_options(
    state: &AppState,
) -> impl std::future::Future<
    Output = Result<(Vec<WorkspaceModelOption>, bool), (StatusCode, Json<Value>)>,
> + Send {
    let db = state.db.clone();
    async move {
        db.read(|transaction| {
            let providers = transaction.scan_prefix::<Record>(
                Table::Providers,
                "",
                MAX_WORKSPACE_PROVIDER_ROWS + 1,
            )?;
            let provider_truncated = providers.len() > MAX_WORKSPACE_PROVIDER_ROWS;
            let mut enabled = Vec::new();
            for (_, provider) in providers.into_iter().take(MAX_WORKSPACE_PROVIDER_ROWS) {
                if !provider.boolean("enabled")? {
                    continue;
                }
                if super::super::providers::provider_is_deleting(&provider)? {
                    continue;
                }
                let id = provider.text("id")?.to_owned();
                let prefix = provider
                    .optional_text("model_prefix")?
                    .filter(|value| !value.trim().is_empty())
                    .map(str::trim)
                    .map(str::to_ascii_lowercase)
                    .unwrap_or_else(|| id.to_ascii_lowercase());
                enabled.push((id, prefix));
            }
            let mut models = Vec::new();
            let mut truncated = provider_truncated;
            for (provider_id, prefix) in enabled {
                let index_prefix = crate::infra::db::provider_model_index_prefix(&provider_id)?;
                let rows = transaction.scan_prefix::<String>(
                    Table::ProviderModelIndex,
                    &index_prefix,
                    MAX_WORKSPACE_MODELS_PER_PROVIDER + 1,
                )?;
                if rows.len() > MAX_WORKSPACE_MODELS_PER_PROVIDER {
                    truncated = true;
                }
                for (_, model) in rows.into_iter().take(MAX_WORKSPACE_MODELS_PER_PROVIDER) {
                    if models.len() >= MAX_WORKSPACE_MODELS_TOTAL {
                        truncated = true;
                        break;
                    }
                    models.push(WorkspaceModelOption {
                        id: format!("{prefix}/{model}"),
                        provider_id: provider_id.clone(),
                        model,
                    });
                }
                if models.len() >= MAX_WORKSPACE_MODELS_TOTAL {
                    break;
                }
            }
            models.sort_by_key(|option| option.id.to_lowercase());
            Ok((models, truncated))
        })
        .await
        .map_err(internal)
    }
}

/// One row of the workspace chat model picker.
pub(super) struct WorkspaceModelOption {
    pub(super) provider_id: String,
    pub(super) model: String,
    pub(super) id: String,
}
