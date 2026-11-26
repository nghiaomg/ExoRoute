//! Meta Muse Code adapter.
//!
//! Muse signs in with the RFC 8628 device grant, which returns a durable `dca:`
//! device token, and then authenticates inference with a subscription key
//! minted from that token. The adapter stores both so a rejected inference key
//! can be replaced without asking the operator to sign in again, and it mints
//! the key during sign-in so a subscription problem is reported there instead
//! of on the first chat request.
//!
//! Muse Spark reasoning only round-trips on the Responses protocol, so
//! Responses is the default upstream protocol and Chat Completions is kept for
//! explicit per-model opt-in.

use super::auth::muse::{self as muse_oauth, MuseAccount};
use super::*;

mod catalog;
mod device;
mod probe;
#[cfg(test)]
mod tests;

use catalog::discover_models;
use probe::test_muse_oauth_model;

/// User agent the Muse inference API expects from its own build client. Muse
/// publishes no third-party inference contract, so the gateway identifies as
/// the Muse Code build client the endpoint is known to accept.
pub(super) const INFERENCE_USER_AGENT: &str =
    "muse-build/1.3.0 (interactive; macos-aarch64; build ac7280f2aca67769d1455a8847bb502b617d50f6)";

pub(super) struct MuseAdapter;

pub(super) static MUSE_ADAPTER: MuseAdapter = MuseAdapter;

/// How long after a mint the gateway refuses to mint again for the same
/// account. It bounds the work a persistently rejected key can cause.
const REMINT_COOLDOWN_SECONDS: u64 = 60;

/// How many inference keys the gateway mints for one account inside
/// [`REMINT_BUDGET_WINDOW_SECONDS`] before it stops and asks the operator to
/// sign in again.
///
/// The cooldown above bounds how *often* a mint runs; this bounds how *many*
/// run inside one window. A Muse inference key does not expire, so an account
/// that keeps needing a new one means the upstream is rejecting freshly minted
/// keys, and minting another would let client traffic drive mint traffic
/// indefinitely.
const REMINT_ATTEMPT_LIMIT: u32 = 3;

/// How long a mint attempt counts against [`REMINT_ATTEMPT_LIMIT`].
///
/// The budget rolls rather than being a lifetime cap, so an account that
/// legitimately needs a new key months apart is never locked out permanently and
/// does not need the operator to sign in again to clear a spent counter.
const REMINT_BUDGET_WINDOW_SECONDS: u64 = 24 * 60 * 60;

/// Floor the gateway applies to a Muse Spark output budget.
///
/// Muse Spark spends its output budget on hidden reasoning before emitting any
/// content, so a request whose budget is too small can return HTTP 200 with an
/// empty message — a silent failure the client cannot retry. Muse's own model
/// probe already asks for this many tokens for the same reason.
///
/// A client that asked for more is never lowered; one that asked for less, or
/// omitted the field, is raised to this floor.
const MUSE_SPARK_MIN_OUTPUT_TOKENS: u64 = 512;

impl ProviderAdapter for MuseAdapter {
    fn adapter_id(&self) -> &'static str {
        MUSE_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        let suffix = match protocol {
            Protocol::Responses => "responses",
            Protocol::ChatCompletions => "chat/completions",
            Protocol::Messages => {
                return Err("Muse requires the Responses or Chat Completions protocol".to_owned());
            }
        };
        muse_url(base_url, suffix)
    }

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        muse_url(base_url, "models")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn prepare_upstream_request<'a>(
        &'a self,
        context: AdapterRequestContext<'a>,
        body: &'a mut Value,
    ) -> AdapterFuture<'a, Result<AdapterRequestPreparation, AdapterRequestError>> {
        Box::pin(async move {
            apply_muse_spark_output_floor(context.model, context.auth.protocol, body);
            Ok(AdapterRequestPreparation::default())
        })
    }

    fn apply_client_headers(
        &self,
        request: reqwest::RequestBuilder,
        _client_headers: &HeaderMap,
    ) -> reqwest::RequestBuilder {
        request
            .header(http::header::USER_AGENT, INFERENCE_USER_AGENT)
            .header(device::API_VERSION_HEADER, device::API_VERSION)
    }

    fn validate_config(
        &self,
        preset: &ProviderPreset,
        auth_type: &str,
        _base_url: &str,
        preferred_protocol: &str,
        supported_protocols: &[String],
        _api_keys_present: bool,
    ) -> Result<(), &'static str> {
        if !preset.supported_auth_types.contains(&auth_type) {
            return Err("Muse requires a connected Muse account or a Muse API key");
        }
        if !matches!(preferred_protocol, "responses" | "chat_completions") {
            return Err("Muse requires the Responses or Chat Completions protocol");
        }
        if !supported_protocols
            .iter()
            .all(|protocol| matches!(protocol.as_str(), "responses" | "chat_completions"))
        {
            return Err("Muse supports only the Responses and Chat Completions protocols");
        }
        Ok(())
    }

    fn apply_request_auth(
        &self,
        request: reqwest::RequestBuilder,
        auth_type: &str,
        _auth_header: Option<&str>,
        secret: Option<&str>,
        oauth_auth: Option<&OAuthRequestAuth>,
        _session_id: &str,
    ) -> Result<reqwest::RequestBuilder, String> {
        let request = request.header(device::API_VERSION_HEADER, device::API_VERSION);
        match oauth_auth {
            // A connected account authenticates with the minted subscription
            // key the account service resolved, never with the durable device
            // token.
            Some(oauth) => Ok(request.bearer_auth(&oauth.access_token)),
            None => match auth_type {
                "none" => Ok(request),
                "bearer" => {
                    let secret = secret.ok_or("Muse API key is missing")?;
                    Ok(request.bearer_auth(secret))
                }
                other => Err(format!("unsupported Muse auth type: {other}")),
            },
        }
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }

    fn discover_oauth_models<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Vec<String>, String>> {
        Box::pin(async move {
            match discover_models(state, credential_id).await? {
                ModelDiscoveryResult::Available { models, .. } => Ok(models),
                ModelDiscoveryResult::Unsupported => Ok(Vec::new()),
            }
        })
    }

    fn test_oauth_model<'a>(
        &'a self,
        state: &'a AppState,
        base_url: &'a str,
        model: &'a str,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, AdapterModelTestOutcome> {
        Box::pin(test_muse_oauth_model(
            self,
            state,
            base_url,
            model,
            credential_id,
        ))
    }

    fn uses_device_authorization(&self) -> bool {
        true
    }

    /// Muse's device grant lives on Meta's OIDC host rather than on the row's
    /// API host, so `base_url` is deliberately unused here.
    fn start_device_authorization<'a>(
        &'a self,
        state: &'a AppState,
        _base_url: &'a str,
    ) -> AdapterFuture<'a, Result<AdapterDeviceAuthorization, String>> {
        let operational = state.operational_settings().settings;
        let allow_local = state.config.allow_private_provider_urls;
        Box::pin(async move {
            device::start_authorization(
                allow_local,
                operational.connect_timeout,
                operational.request_timeout,
                operational.upstream,
            )
            .await
        })
    }

    fn poll_device_authorization<'a>(
        &'a self,
        state: &'a AppState,
        _base_url: &'a str,
        device_code: &'a str,
    ) -> AdapterFuture<'a, Result<AdapterDevicePoll, String>> {
        let operational = state.operational_settings().settings;
        let allow_local = state.config.allow_private_provider_urls;
        Box::pin(async move {
            let poll = device::poll_authorization(
                device_code,
                allow_local,
                operational.connect_timeout,
                operational.request_timeout,
                operational.upstream,
            )
            .await?;
            let approved = match poll {
                device::DevicePoll::Pending => return Ok(AdapterDevicePoll::Pending),
                device::DevicePoll::SlowDown => return Ok(AdapterDevicePoll::SlowDown),
                device::DevicePoll::Denied => return Ok(AdapterDevicePoll::Denied),
                device::DevicePoll::Expired => return Ok(AdapterDevicePoll::Expired),
                device::DevicePoll::Approved(payload) => payload,
            };
            // The grant is only half of a Muse sign-in; without the subscription
            // inference key the account cannot serve a request. Minting here
            // reports a subscription problem during sign-in.
            let mut account = MuseAccount::from_device_grant(&approved)?;
            // The sign-in mint is the operator's own provisioning step, so it is
            // not charged against the remint budget. If it fails for a transient
            // reason the approved grant must still be kept: the durable token is
            // stored on its own, and the first-use path that mints later is the
            // one that is throttled.
            if let Err(error) = mint_key(state, &mut account).await {
                tracing::warn!(
                    %error,
                    "Muse sign-in could not mint an inference key; it will be minted on first use"
                );
            }
            Ok(AdapterDevicePoll::Approved(AdapterOAuthAccount {
                payload: account.to_payload()?,
                display_name: account.display_name(),
            }))
        })
    }

    fn resolve_oauth_request_auth<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Option<OAuthRequestAuth>, String>> {
        Box::pin(async move {
            let loaded = muse_oauth::load_muse_account(state, credential_id).await?;
            // A sign-in whose mint failed has its key minted on first use rather
            // than making the operator sign in again.
            let account = match loaded.needs_remint {
                true => remint_and_store(state, credential_id, &loaded.account).await?,
                false => loaded.account,
            };
            let access_token = account
                .inference_key()
                .ok_or_else(|| "Muse account has no inference key".to_owned())?
                .to_owned();
            Ok(Some(OAuthRequestAuth {
                access_token,
                account_id: None,
                project_id: None,
            }))
        })
    }

    /// A rejected Muse inference key is recoverable: the durable device token
    /// stored beside it outlives the key, so the gateway mints a replacement and
    /// keeps the credential in rotation instead of invalidating it and making
    /// the operator sign in again.
    ///
    /// A pasted Muse API key has no durable token behind it, so it is not a Muse
    /// account record and is still invalidated normally. When the replacement
    /// cannot be minted this returns `false`, which hands the credential back to
    /// the standard invalidation path.
    ///
    /// The remint itself is throttled — by a cooldown and by a per-sign-in attempt
    /// budget — so an upstream that keeps rejecting freshly minted keys cannot
    /// turn client traffic into mint traffic.
    fn repair_rejected_credential<'a>(
        &'a self,
        state: &'a AppState,
        provider_id: &'a str,
        credential_id: &'a str,
        status: StatusCode,
    ) -> AdapterFuture<'a, bool> {
        Box::pin(async move {
            // The call site currently reaches this hook for HTTP 401; 403 is
            // accepted because it means the same thing for this provider.
            if !matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
                return false;
            }
            let Ok(loaded) = muse_oauth::load_muse_account(state, credential_id).await else {
                return false;
            };
            // `remint_and_store` owns the throttle: a key minted moments ago that
            // is rejected again, or an account that has already spent its mint
            // budget, is refused there, so the credential falls through to the
            // normal invalidation path below instead of minting again.
            match remint_and_store(state, credential_id, &loaded.account).await {
                Ok(_) => {
                    tracing::info!(
                        %provider_id,
                        "minted a replacement Muse inference key after the upstream rejected the stored key"
                    );
                    true
                }
                Err(error) => {
                    tracing::warn!(
                        %provider_id,
                        %error,
                        "could not mint a replacement Muse inference key"
                    );
                    false
                }
            }
        })
    }

    fn save_oauth_account<'a>(
        &'a self,
        state: &'a AppState,
        provider_id: &'a str,
        account: AdapterOAuthAccount,
    ) -> AdapterFuture<'a, Result<(), String>> {
        Box::pin(muse_oauth::save_muse_account(state, provider_id, account))
    }
}

/// Mints an inference key into `account`. Touches neither storage nor the
/// throttle counters, so each caller decides whether the attempt counts.
async fn mint_key(state: &AppState, account: &mut MuseAccount) -> Result<(), String> {
    let operational = state.operational_settings().settings;
    let minted = device::mint_api_key(
        device::MINT_URL,
        &account.dca_token,
        state.config.allow_private_provider_urls,
        operational.connect_timeout,
        operational.request_timeout,
        operational.upstream,
    )
    .await?;
    account.apply_minted_key(&minted.api_key)?;
    account.apply_mint_metadata(&minted.metadata);
    Ok(())
}

/// Mints an inference key and stores it, so the repaired account serves the
/// next request without minting again.
///
/// Every path that can remint goes through here — a request whose account has
/// no key yet, catalog discovery, a model probe, and repairing a rejected key —
/// so the throttle in this one place covers all of them.
pub(super) async fn remint_and_store(
    state: &AppState,
    credential_id: &str,
    account: &MuseAccount,
) -> Result<MuseAccount, String> {
    let clock_error = || "could not read the clock for the Muse inference key".to_owned();
    let now_timestamp = crate::infra::db::utc_timestamp_now().map_err(|_| clock_error())?;
    let now = crate::infra::db::parse_utc_timestamp(&now_timestamp).ok_or_else(clock_error)?;
    if let Some(reason) = account.remint_blocked(
        now,
        REMINT_COOLDOWN_SECONDS,
        REMINT_ATTEMPT_LIMIT,
        REMINT_BUDGET_WINDOW_SECONDS,
    ) {
        return Err(reason.to_owned());
    }
    let mut attempted = account.clone();
    // Counted before the network call so a mint that fails still advances the
    // throttle; otherwise a mint that never succeeds is retried on every request
    // that reaches this account.
    attempted.record_mint_attempt(now, REMINT_BUDGET_WINDOW_SECONDS)?;
    match mint_key(state, &mut attempted).await {
        Ok(()) => {
            muse_oauth::persist_account(state, credential_id, &attempted).await?;
            Ok(attempted)
        }
        Err(error) => {
            if let Err(persist_error) =
                muse_oauth::persist_remint_attempt(state, credential_id, &attempted).await
            {
                tracing::warn!(
                    %persist_error,
                    "could not persist the Muse remint throttle"
                );
            }
            Err(error)
        }
    }
}

/// Whether a model is one of the Muse Spark reasoning models, which spend their
/// output budget on hidden reasoning before emitting content.
pub(super) fn is_muse_spark_model(model: &str) -> bool {
    model.to_ascii_lowercase().contains("muse-spark")
}

/// Raises a Muse Spark request's output budget to
/// [`MUSE_SPARK_MIN_OUTPUT_TOKENS`] when the client asked for less, or omitted
/// the field entirely.
///
/// Only the Responses and Chat Completions wire formats the adapter can send are
/// shaped, and a budget the client set higher is left exactly as it is. The model
/// is read from the resolved target and, failing that, from the body, because a
/// caller that only has the translated body must still get the floor.
///
/// Raising the budget rewrites a field the client sent, so the change is logged
/// (at debug, since it runs on the request path) rather than applied silently.
pub(super) fn apply_muse_spark_output_floor(
    model: &str,
    protocol: UpstreamProtocol,
    body: &mut Value,
) {
    let field = match protocol {
        UpstreamProtocol::Responses => "max_output_tokens",
        UpstreamProtocol::ChatCompletions => "max_tokens",
        // The adapter never sends Messages or Google formats, so there is no
        // field name to shape and adding one would invent an upstream contract.
        UpstreamProtocol::Messages | UpstreamProtocol::GoogleGenerateContent => return,
    };
    if !is_muse_spark_model(model)
        && !body
            .get("model")
            .and_then(Value::as_str)
            .is_some_and(is_muse_spark_model)
    {
        return;
    }
    let requested = body.get(field).and_then(Value::as_u64);
    if requested.is_none_or(|tokens| tokens < MUSE_SPARK_MIN_OUTPUT_TOKENS) {
        tracing::debug!(
            model,
            field,
            requested_tokens = requested.unwrap_or_default(),
            applied_tokens = MUSE_SPARK_MIN_OUTPUT_TOKENS,
            "raised the Muse Spark output budget so a reasoning turn is not returned empty"
        );
        body[field] = json!(MUSE_SPARK_MIN_OUTPUT_TOKENS);
    }
}

/// Builds an inference URL from the stored base URL. The `/v1` root the preset
/// installs and a row that already names a full endpoint both resolve.
pub(super) fn muse_url(base_url: &str, suffix: &str) -> Result<reqwest::Url, String> {
    let mut url =
        reqwest::Url::parse(base_url).map_err(|_| "Muse provider URL is invalid".to_owned())?;
    if url.host_str().is_none_or(str::is_empty) {
        return Err("Muse provider URL must include a host".to_owned());
    }
    let current = url.path().trim_end_matches('/').to_owned();
    let root = ["/chat/completions", "/responses", "/models"]
        .into_iter()
        .find_map(|ending| current.strip_suffix(ending))
        .unwrap_or(&current);
    url.set_path(&format!("{}/{}", root.trim_end_matches('/'), suffix));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}
