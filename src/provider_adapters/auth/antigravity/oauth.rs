use super::{
    AUTHORIZE_URL, AntigravityAccount, CALLBACK_URL, MAX_OAUTH_BODY_BYTES,
    MAX_OAUTH_REDIRECT_URI_BYTES, SCOPES, TOKEN_URL, enrich_account, oauth_client, oauth_client_id,
    oauth_client_secret, read_json,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Url;
use sha2::{Digest, Sha256};
use std::time::Duration;

pub(crate) fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub(crate) fn authorization_url(
    state: &str,
    challenge: &str,
    redirect_uri: Option<&str>,
) -> Result<String, String> {
    let client_id = oauth_client_id()?;
    authorization_url_with_client_id(&client_id, state, challenge, redirect_uri)
}

pub(crate) fn authorization_url_with_client_id(
    client_id: &str,
    state: &str,
    challenge: &str,
    redirect_uri: Option<&str>,
) -> Result<String, String> {
    let redirect_uri = redirect_uri.unwrap_or(CALLBACK_URL);
    let mut url = Url::parse(AUTHORIZE_URL)
        .map_err(|_| "Antigravity authorization URL is invalid".to_owned())?;
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", &SCOPES.join(" "))
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state);
    Ok(url.to_string())
}

pub(crate) async fn exchange_code(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<AntigravityAccount, String> {
    let redirect_uri = redirect_uri.trim();
    if code.trim().is_empty()
        || code.len() > 8192
        || verifier.is_empty()
        || verifier.len() > 256
        || redirect_uri.is_empty()
        || redirect_uri.len() > MAX_OAUTH_REDIRECT_URI_BYTES
        || redirect_uri.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err("Antigravity authorization response is invalid".to_owned());
    }
    let client_id = oauth_client_id()?;
    let client_secret = oauth_client_secret()?;
    let (url, client) = oauth_client(TOKEN_URL, connect_timeout, request_timeout, upstream).await?;
    let mut form = vec![
        ("grant_type", "authorization_code".to_owned()),
        ("client_id", client_id),
        ("code", code.to_owned()),
        ("redirect_uri", redirect_uri.to_owned()),
        ("code_verifier", verifier.to_owned()),
    ];
    if let Some(secret) = client_secret {
        form.push(("client_secret", secret));
    }
    let response = client
        .post(url)
        .header("Accept", "application/json")
        .form(&form)
        .send()
        .await
        .map_err(|error| {
            format!(
                "could not reach Google OAuth service: {}",
                error.without_url()
            )
        })?;
    let value = read_json(
        response,
        "Google OAuth token exchange",
        MAX_OAUTH_BODY_BYTES,
    )
    .await
    .map_err(|error| error.message)?;
    let mut account = AntigravityAccount::from_token_response(&value)?;
    enrich_account(&mut account, connect_timeout, request_timeout, upstream).await?;
    Ok(account)
}
