use super::*;
use provider::{Provider, ProviderCredential};
use provider_adapters::keys::{ProviderApiKeyRow, ProviderCredentialType};

/// Outcome of resolving the auth material for one credential.
pub(super) enum ResolvedAuth {
    /// The credential produced usable auth material.
    Ready {
        secret: Option<String>,
        oauth_auth: Option<provider_adapters::OAuthRequestAuth>,
    },
    /// The credential was unusable; rotate to the next one, if any remain.
    /// Carries the diagnostic the original pipeline recorded as `last_error`.
    Rotate(String),
}

/// Returns true when this credential should be skipped because the provider
/// account is in an Antigravity quota cooldown for the requested model.
pub(super) async fn quota_cooldown_skip(
    state: &AppState,
    provider: &Provider,
    credential: &ProviderCredential,
    model: &str,
) -> bool {
    if provider.adapter_id != provider_adapters::ANTIGRAVITY_ADAPTER_ID {
        return false;
    }
    let Some(key_id) = credential.id.as_deref() else {
        return false;
    };
    state
        .antigravity_quota_is_blocked(&provider.id, key_id, model)
        .await
}

/// Resolves the auth material for one credential: OAuth account lookup for
/// OAuth credentials, master-key decryption for stored API keys. An unusable
/// credential yields [`ResolvedAuth::Rotate`] with the same diagnostic the
/// original inline pipeline produced.
pub(super) async fn resolve_credential_auth(
    state: &AppState,
    provider: &Provider,
    credential: &ProviderCredential,
) -> ResolvedAuth {
    if credential.credential_type == Some(ProviderCredentialType::OAuth) {
        let Some(key_id) = credential.id.as_deref() else {
            return ResolvedAuth::Rotate(format!(
                "provider '{}' has an invalid provider OAuth account record",
                provider.id
            ));
        };
        return match provider_adapters::resolve_oauth_request_auth(
            &provider.adapter_id,
            state,
            key_id,
        )
        .await
        {
            Ok(Some(auth)) => ResolvedAuth::Ready {
                secret: None,
                oauth_auth: Some(auth),
            },
            Ok(None) => ResolvedAuth::Rotate(format!(
                "provider '{}' OAuth account is unavailable",
                provider.id
            )),
            Err(error) => ResolvedAuth::Rotate(error),
        };
    }
    match credential.encrypted_secret.as_deref() {
        Some(encrypted) => {
            match crate::security::decrypt_secret(state.config.master_key.as_ref(), Some(encrypted))
            {
                Ok(secret) => ResolvedAuth::Ready {
                    secret,
                    oauth_auth: None,
                },
                Err(_) => ResolvedAuth::Rotate(format!(
                    "provider '{}' has an unreadable API key",
                    provider.id
                )),
            }
        }
        None => ResolvedAuth::Ready {
            secret: credential.secret.clone(),
            oauth_auth: None,
        },
    }
}

/// Rotates through a provider's credentials (stored API keys, then the
/// provider's own secret fallback), skipping rejected or unusable ones.
///
/// This stage owns the key-cursor lifecycle that previously appeared as a
/// repeated inline block at four places in the gateway pipeline: starting
/// from stored keys, falling back to the configured secret, and advancing
/// after each rejection. A rotation whose [`Self::current`] is `None` right
/// after [`Self::start`] means the provider has no usable credential.
///
/// A provider configured for round robin starts at the key that follows the one
/// that served the previous request, so consecutive requests and streams use
/// different keys.
pub(super) struct CredentialRotation {
    cursor: Option<ProviderApiKeyCursor>,
    current: Option<ProviderCredential>,
}

impl CredentialRotation {
    /// Starts rotation from the provider's credential storage, falling back
    /// to the provider's own configured secret when no stored keys exist.
    pub(super) async fn start(state: &AppState, provider: &Provider) -> Result<Self, StorageError> {
        if provider.auth_type == "none" {
            return Ok(Self {
                cursor: None,
                current: Some(ProviderCredential {
                    id: None,
                    secret: None,
                    encrypted_secret: None,
                    credential_type: None,
                }),
            });
        }
        if provider.key_strategy == crate::admin::providers::KEY_STRATEGY_ROUND_ROBIN {
            return Self::start_rotating(state, provider).await;
        }
        let mut rotation = Self {
            cursor: None,
            current: None,
        };
        if let Some((cursor, key)) = ProviderApiKeyCursor::start(&state.db, &provider.id).await? {
            rotation.cursor = Some(cursor);
            rotation.current = Some(credential_from_row(key));
        } else {
            rotation.current = provider_secret_credential(provider);
        }
        // No cursor and no provider secret leaves `current` empty: the
        // provider has no usable credential at all.
        Ok(rotation)
    }

    /// Starts a round-robin rotation at the key after the one that started the
    /// previous request, restarting from the preferred key when the stored
    /// position was the last one.
    async fn start_rotating(state: &AppState, provider: &Provider) -> Result<Self, StorageError> {
        let previous = state.provider_key_cursor(&provider.id).await;
        let started =
            ProviderApiKeyCursor::start_rotating(&state.db, &provider.id, previous.as_deref())
                .await?;
        let started = match started {
            Some(started) => Some(started),
            None if previous.is_some() => {
                ProviderApiKeyCursor::start_rotating(&state.db, &provider.id, None).await?
            }
            None => None,
        };
        let Some((cursor, key)) = started else {
            // Rotating a provider without stored keys keeps the same secret
            // fallback as the ordered strategy.
            return Ok(Self {
                cursor: None,
                current: provider_secret_credential(provider),
            });
        };
        // Store the position this request starts from, so the next request
        // continues with the following key.
        if let Some(position) = cursor.position().map(str::to_owned) {
            state.set_provider_key_cursor(&provider.id, &position).await;
        }
        Ok(Self {
            cursor: Some(cursor),
            current: Some(credential_from_row(key)),
        })
    }

    /// The credential to attempt now, if any remain.
    pub(super) fn current(&self) -> Option<&ProviderCredential> {
        self.current.as_ref()
    }

    /// Advances to the next stored credential, if any.
    pub(super) async fn advance(&mut self, state: &AppState) -> Result<(), StorageError> {
        self.current = match self.cursor.as_mut() {
            Some(cursor) => cursor.next(&state.db).await?.map(credential_from_row),
            None => None,
        };
        Ok(())
    }
}

/// Turns one stored provider key into the credential the pipeline resolves.
fn credential_from_row(row: ProviderApiKeyRow) -> ProviderCredential {
    ProviderCredential {
        id: Some(row.id),
        secret: None,
        encrypted_secret: Some(row.encrypted_secret),
        credential_type: Some(row.credential_type),
    }
}

/// The provider's own configured secret, used when no stored credential can
/// serve a request.
fn provider_secret_credential(provider: &Provider) -> Option<ProviderCredential> {
    provider.secret.as_ref().map(|secret| ProviderCredential {
        id: None,
        secret: Some(secret.clone()),
        encrypted_secret: None,
        credential_type: Some(ProviderCredentialType::ApiKey),
    })
}
