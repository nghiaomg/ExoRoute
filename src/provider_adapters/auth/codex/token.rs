use super::account::TokenResponse;
use super::*;

pub(super) async fn oauth_client(
    connect_timeout: std::time::Duration,
    request_timeout: std::time::Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<(reqwest::Url, reqwest::Client), String> {
    let (url, client) = egress::provider_client(
        TOKEN_URL,
        false,
        connect_timeout,
        request_timeout,
        false,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        upstream,
    )
    .await?;
    Ok((url, client))
}

pub(super) async fn read_token_response(
    response: reqwest::Response,
) -> Result<CodexAccount, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(format!("OpenAI OAuth returned HTTP {}", status.as_u16()));
    }
    let mut body = Vec::new();
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "OpenAI OAuth response could not be read".to_owned())?
    {
        if body.len().saturating_add(chunk.len()) > 128 * 1024 {
            return Err("OpenAI OAuth response exceeded its size limit".to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    let tokens: TokenResponse = serde_json::from_slice(&body)
        .map_err(|_| "OpenAI OAuth response did not contain valid token data".to_owned())?;
    CodexAccount::from_tokens(tokens)
}

pub(super) fn validate_token(token: &str) -> Result<(), String> {
    if token.is_empty() || token.len() > MAX_TOKEN_BYTES || token.contains('\0') {
        return Err("OpenAI OAuth returned an invalid token".to_owned());
    }
    Ok(())
}

pub(super) fn decode_jwt_claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}
