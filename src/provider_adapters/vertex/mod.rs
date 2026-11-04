//! Google Vertex AI.
//!
//! Vertex authenticates with a short-lived OAuth access token minted from a
//! service-account key, not with a static API key, and it addresses models
//! through a project and location path. The provider base URL carries
//! `{location}/{project}` so one installation can route several projects
//! without storing credentials per project.
//!
//! Request preparation signs an RS256 JWT, exchanges it for an access token,
//! and caches that token until shortly before it expires. The cache is keyed by
//! credential and bounded, so rotating or deleting a credential cannot grow it
//! without limit.

use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::signature::{RandomizedSigner, SignatureEncoding};
use sha2::Sha256;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const VERTEX_ADAPTER_ID: &str = "vertex_ai";
/// Refresh this long before the reported expiry so a request never starts with
/// a token that expires while it is in flight.
const TOKEN_REFRESH_MARGIN: Duration = Duration::from_secs(300);
/// Service-account tokens last an hour; cap the cache lifetime in case an
/// upstream replies with a longer window than expected.
const MAX_TOKEN_LIFETIME: Duration = Duration::from_secs(3600);
const MAX_CACHED_TOKENS: usize = 256;
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const VERTEX_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

#[derive(Debug)]
struct ServiceAccount {
    client_email: String,
    private_key_pem: String,
    token_uri: String,
}

/// Parsed service-account key. The credential is stored encrypted at rest; this
/// reads the decrypted JSON, so a malformed key is reported instead of being
/// treated as an absent credential.
fn parse_service_account(secret: &str) -> Result<ServiceAccount, String> {
    let value: Value = serde_json::from_str(secret)
        .map_err(|_| "Vertex AI credential is not a service-account JSON key".to_owned())?;
    let field = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| format!("Vertex AI service-account key is missing {name}"))
    };
    let account = ServiceAccount {
        client_email: field("client_email")?.to_owned(),
        private_key_pem: field("private_key")?.to_owned(),
        token_uri: value
            .get("token_uri")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .unwrap_or(GOOGLE_TOKEN_URL)
            .to_owned(),
    };
    if account.token_uri != GOOGLE_TOKEN_URL {
        return Err("Vertex AI service-account key uses an unexpected token endpoint".to_owned());
    }
    Ok(account)
}

fn base64url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Builds the RS256 assertion Google exchanges for an access token.
fn sign_jwt_assertion(account: &ServiceAccount, now_epoch: u64) -> Result<String, String> {
    let header = base64url(br#"{"alg":"RS256","typ":"JWT"}"#);
    let claims = json!({
        "iss": account.client_email,
        "scope": VERTEX_SCOPE,
        "aud": account.token_uri,
        "iat": now_epoch,
        // Google rejects assertions valid for more than one hour.
        "exp": now_epoch + 3600,
    });
    let payload = base64url(claims.to_string().as_bytes());
    let signing_input = format!("{header}.{payload}");
    let key = rsa::RsaPrivateKey::from_pkcs8_pem(&account.private_key_pem)
        .or_else(|_| rsa::RsaPrivateKey::from_pkcs1_pem(&account.private_key_pem))
        .map_err(|_| {
            "Vertex AI service-account private key is not a valid PEM RSA key".to_owned()
        })?;
    let signing_key = SigningKey::<Sha256>::new(key);
    let signature = signing_key
        .sign_with_rng(&mut rand::thread_rng(), signing_input.as_bytes())
        .to_bytes();
    Ok(format!("{signing_input}.{}", base64url(&signature)))
}

#[derive(Clone)]
struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

/// Process-wide token cache. Vertex tokens are minted per credential and cannot
/// be derived from the request, so they are kept here rather than in the
/// request scope. Bounded by credential count; entries expire on their own and
/// stale ones are evicted when the map is full.
fn token_cache() -> &'static Mutex<std::collections::HashMap<String, CachedToken>> {
    static CACHE: OnceLock<Mutex<std::collections::HashMap<String, CachedToken>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn cached_token(credential_id: &str) -> Option<String> {
    let cache = token_cache().lock().ok()?;
    let token = cache.get(credential_id)?;
    (token.expires_at > Instant::now()).then(|| token.access_token.clone())
}

fn store_token(credential_id: &str, access_token: String, lifetime: Duration) {
    let Ok(mut cache) = token_cache().lock() else {
        return;
    };
    let now = Instant::now();
    cache.retain(|_, token| token.expires_at > now);
    if cache.len() >= MAX_CACHED_TOKENS && !cache.contains_key(credential_id) {
        // Capacity policy: drop the entry closest to expiry rather than letting
        // a deleted credential pin a slot forever.
        if let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, token)| token.expires_at)
            .map(|(key, _)| key.clone())
        {
            cache.remove(&oldest);
        }
    }
    let lifetime = lifetime
        .min(MAX_TOKEN_LIFETIME)
        .saturating_sub(TOKEN_REFRESH_MARGIN);
    if lifetime.is_zero() {
        return;
    }
    cache.insert(
        credential_id.to_owned(),
        CachedToken {
            access_token,
            expires_at: now + lifetime,
        },
    );
}

pub(super) struct VertexAdapter;

pub(super) static VERTEX_ADAPTER: VertexAdapter = VertexAdapter;

/// Splits the provider base URL into the location and project Vertex addresses.
/// The operator supplies `https://{location}-aiplatform.googleapis.com/v1/projects/{project}`
/// because both values are deployment-specific and cannot come from the catalog.
fn vertex_location_and_project(base_url: &str) -> Result<(String, String), String> {
    let url = reqwest::Url::parse(base_url.trim())
        .map_err(|error| format!("invalid provider URL: {error}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| "Vertex AI provider URL must include a host".to_owned())?;
    let location = host
        .strip_suffix("-aiplatform.googleapis.com")
        .filter(|location| !location.is_empty() && !location.contains('.'))
        .ok_or_else(|| {
            "Vertex AI provider URL must use a regional endpoint such as https://us-central1-aiplatform.googleapis.com"
                .to_owned()
        })?;
    let segments: Vec<&str> = url
        .path_segments()
        .map(|segments| segments.filter(|segment| !segment.is_empty()).collect())
        .unwrap_or_default();
    // Expect /v1/projects/{project}; tolerate a trailing slash or extra suffix.
    let project = segments
        .windows(2)
        .find(|pair| pair[0] == "projects")
        .map(|pair| pair[1])
        .filter(|project| !project.is_empty())
        .ok_or_else(|| "Vertex AI provider URL must include /v1/projects/{project}".to_owned())?;
    if !segments
        .first()
        .is_some_and(|version| version.starts_with('v'))
    {
        return Err("Vertex AI provider URL must include an API version path".to_owned());
    }
    if !project
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("Vertex AI project id contains invalid characters".to_owned());
    }
    Ok((location.to_owned(), project.to_owned()))
}

/// Builds the model path for one upstream protocol. Vertex serves Gemini
/// through `generateContent` rather than the OpenAI-compatible routes.
fn vertex_endpoint(base_url: &str, model: &str, streaming: bool) -> Result<reqwest::Url, String> {
    let (location, project) = vertex_location_and_project(base_url)?;
    let model = model.trim();
    if model.is_empty() {
        return Err("Vertex AI requires a model name".to_owned());
    }
    if !model
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'@'))
    {
        return Err("Vertex AI model name contains invalid characters".to_owned());
    }
    // A publisher-qualified id addresses a third-party model; a bare id is a
    // first-party Gemini model. Both live under the same publishers path, and a
    // slash is rejected above so the path cannot be redirected.
    let model_path = format!("publishers/google/models/{model}");
    let action = if streaming {
        "streamGenerateContent"
    } else {
        "generateContent"
    };
    let mut url = reqwest::Url::parse(&format!("https://{location}-aiplatform.googleapis.com"))
        .map_err(|error| format!("invalid Vertex AI endpoint: {error}"))?;
    url.set_path(&format!(
        "/v1/projects/{project}/locations/{location}/{model_path}:{action}"
    ));
    if streaming {
        // Vertex streams JSON objects; the SSE-compatible framing needs `alt=sse`.
        url.query_pairs_mut().append_pair("alt", "sse");
    }
    Ok(url)
}

impl ProviderAdapter for VertexAdapter {
    fn adapter_id(&self) -> &'static str {
        VERTEX_ADAPTER_ID
    }

    /// Vertex has no OpenAI-compatible endpoint, so a bare `endpoint` call has
    /// no path to build; routing always supplies a model.
    fn endpoint(
        &self,
        _base_url: &str,
        _protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        Err("Vertex AI requires a model; route through a model".to_owned())
    }

    fn upstream_endpoint(
        &self,
        base_url: &str,
        protocol: UpstreamProtocol,
        model: &str,
        streaming: bool,
        adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != UpstreamProtocol::GoogleGenerateContent {
            return Err(
                "Vertex AI requires the Google Generate Content upstream protocol".to_owned(),
            );
        }
        let _ = adapter_base_url_override;
        vertex_endpoint(base_url, model, streaming)
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn prepare_upstream_request<'a>(
        &'a self,
        context: AdapterRequestContext<'a>,
        _body: &'a mut Value,
    ) -> AdapterFuture<'a, Result<AdapterRequestPreparation, AdapterRequestError>> {
        Box::pin(async move {
            let Some(secret) = context.auth.secret else {
                return Err(AdapterRequestError::new(
                    Some(StatusCode::UNAUTHORIZED),
                    None,
                    "Vertex AI service-account credential is unavailable",
                ));
            };
            let account = parse_service_account(secret).map_err(|message| {
                AdapterRequestError::new(Some(StatusCode::UNAUTHORIZED), None, message)
            })?;
            let access_token = access_token_for(&account).await.map_err(|message| {
                AdapterRequestError::new(Some(StatusCode::BAD_GATEWAY), None, message)
            })?;
            let mut headers = HeaderMap::new();
            let value = HeaderValue::from_str(&format!("Bearer {access_token}")).map_err(|_| {
                AdapterRequestError::new(
                    Some(StatusCode::INTERNAL_SERVER_ERROR),
                    None,
                    "Vertex AI access token contains invalid header characters",
                )
            })?;
            headers.insert(http::header::AUTHORIZATION, value);
            Ok(AdapterRequestPreparation::new(headers, None, None))
        })
    }

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        let (location, project) = vertex_location_and_project(base_url)?;
        let mut url = reqwest::Url::parse(&format!("https://{location}-aiplatform.googleapis.com"))
            .map_err(|error| format!("invalid Vertex AI endpoint: {error}"))?;
        url.set_path(&format!(
            "/v1/projects/{project}/locations/{location}/publishers/google/models"
        ));
        Ok(url)
    }

    fn validate_config(
        &self,
        preset: &ProviderPreset,
        auth_type: &str,
        base_url: &str,
        preferred_protocol: &str,
        supported_protocols: &[String],
        api_keys_present: bool,
    ) -> Result<(), &'static str> {
        if !preset.supported_auth_types.contains(&auth_type) {
            return Err("Vertex AI requires a service-account credential");
        }
        if preferred_protocol != "google_generate_content"
            || supported_protocols
                .iter()
                .any(|protocol| protocol != "google_generate_content")
        {
            return Err("Vertex AI supports only the Google Generate Content protocol");
        }
        if !api_keys_present {
            return Err("Vertex AI requires a service-account credential");
        }
        vertex_location_and_project(base_url).map(|_| ()).map_err(
            |_| "Vertex AI provider URL must be a regional endpoint with /v1/projects/{project}",
        )
    }

    fn is_key_rejection_status(&self, status: StatusCode) -> bool {
        matches!(
            status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
        )
    }
}

/// Exchanges the service-account assertion for an access token, reusing a
/// cached token while it remains valid. The cache key is the account identity
/// plus its token endpoint, so rotating a key for the same account reuses the
/// live token and a different account never inherits one.
async fn access_token_for(account: &ServiceAccount) -> Result<String, String> {
    let cache_key = format!("{}:{}", account.client_email, account.token_uri);
    if let Some(token) = cached_token(&cache_key) {
        return Ok(token);
    }
    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|_| "system clock is before the Unix epoch".to_owned())?;
    let assertion = sign_jwt_assertion(account, now_epoch)?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("could not build the Vertex AI token client: {error}"))?;
    let response = client
        .post(&account.token_uri)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", assertion.as_str()),
        ])
        .send()
        .await
        .map_err(|error| format!("Vertex AI token exchange failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Vertex AI token exchange returned HTTP {}",
            status.as_u16()
        ));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|_| "Vertex AI token response was not JSON".to_owned())?;
    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| "Vertex AI token response contained no access token".to_owned())?
        .to_owned();
    let lifetime = body
        .get("expires_in")
        .and_then(Value::as_u64)
        .map(Duration::from_secs)
        .unwrap_or(MAX_TOKEN_LIFETIME);
    store_token(&cache_key, access_token.clone(), lifetime);
    Ok(access_token)
}

#[cfg(test)]
mod tests;
