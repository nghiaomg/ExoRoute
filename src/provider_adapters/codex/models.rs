use super::auth::codex::{self as codex_oauth, CodexAccount};
use crate::{security::egress, state::AppState};
use futures_util::StreamExt;
use http::{HeaderValue, StatusCode};
use serde_json::Value;
const MAX_CODEX_MODEL_BYTES: usize = 4 * 1024 * 1024;
const MAX_CODEX_MODELS: usize = 10_000;

fn codex_model_rows(value: &Value) -> Result<Vec<(&Value, Option<&str>)>, String> {
    if let Some(rows) = value.as_array() {
        return Ok(rows.iter().map(|row| (row, None)).collect());
    }

    if let Some(rows) = value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(Value::as_array)
    {
        return Ok(rows.iter().map(|row| (row, None)).collect());
    }

    let Some(object) = value.as_object() else {
        return Err("provider response does not contain a supported models list".to_owned());
    };
    if object.contains_key("data") || object.contains_key("models") {
        return Err("provider response does not contain a supported models list".to_owned());
    }

    let rows = object
        .iter()
        .filter(|(_, row)| row.is_object())
        .map(|(id, row)| (row, Some(id.as_str())))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Err("provider response does not contain a supported models list".to_owned());
    }
    Ok(rows)
}

fn codex_model_identifier<'a>(row: &'a Value, fallback: Option<&'a str>) -> Option<&'a str> {
    row.get("slug")
        .and_then(Value::as_str)
        .or_else(|| row.get("id").and_then(Value::as_str))
        .or_else(|| row.get("model").and_then(Value::as_str))
        .or_else(|| row.get("name").and_then(Value::as_str))
        .or(fallback)
}

pub(crate) fn parse_codex_model_list(value: &Value) -> Result<Vec<String>, String> {
    let rows = codex_model_rows(value)?;
    let mut seen = std::collections::HashSet::new();
    let mut models = Vec::new();
    for (row, fallback_id) in rows {
        if row
            .get("visibility")
            .and_then(Value::as_str)
            .is_some_and(|visibility| visibility.eq_ignore_ascii_case("hide"))
            || row.get("supported_in_api").and_then(Value::as_bool) == Some(false)
            || row.get("supportedInApi").and_then(Value::as_bool) == Some(false)
        {
            continue;
        }
        let Some(model) = codex_model_identifier(row, fallback_id)
            .map(str::trim)
            .filter(|model| !model.is_empty())
        else {
            continue;
        };
        if seen.insert(model.to_owned()) {
            models.push(model.to_owned());
            if models.len() == MAX_CODEX_MODELS {
                break;
            }
        }
    }
    Ok(models)
}

pub(crate) fn build_codex_model_request(
    client: &reqwest::Client,
    url: &reqwest::Url,
    account: &CodexAccount,
) -> Result<reqwest::RequestBuilder, String> {
    let mut request = client
        .get(url.clone())
        .bearer_auth(&account.access_token)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("originator", "codex_cli_rs")
        .header("User-Agent", codex_oauth::CODEX_USER_AGENT)
        .header("Version", codex_oauth::CODEX_CLI_VERSION)
        .header("Openai-Beta", "responses=experimental")
        .header("X-Codex-Beta-Features", "responses_websockets");
    if let Some(account_id) = account.account_id.as_deref() {
        let account_id = HeaderValue::from_str(account_id)
            .map_err(|_| "OpenAI Codex account ID is invalid".to_owned())?;
        request = request.header("ChatGPT-Account-ID", account_id);
    }

    Ok(request)
}

async fn send_codex_model_request(
    client: &reqwest::Client,
    url: &reqwest::Url,
    account: &CodexAccount,
) -> Result<reqwest::Response, String> {
    build_codex_model_request(client, url, account)?
        .send()
        .await
        .map_err(|error| {
            format!(
                "Could not reach OpenAI Codex models: {}",
                error.without_url()
            )
        })
}

pub(crate) async fn discover_codex_models(
    state: &AppState,
    credential_id: &str,
) -> Result<Vec<String>, String> {
    let mut account = codex_oauth::account_for_use(state, credential_id).await?;
    let mut url = reqwest::Url::parse(codex_oauth::BASE_URL)
        .map_err(|_| "OpenAI Codex model endpoint is invalid".to_owned())?;
    url.set_path("/backend-api/codex/models");
    url.query_pairs_mut()
        .append_pair("client_version", codex_oauth::CODEX_CLI_VERSION);
    let upstream = state.operational_settings().settings.upstream;
    let (url, client) = egress::provider_client(
        url.as_str(),
        false,
        state
            .config
            .connect_timeout
            .min(std::time::Duration::from_secs(3)),
        state
            .config
            .request_timeout
            .min(upstream.discovery_request_timeout),
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        upstream,
    )
    .await?;
    let mut response = send_codex_model_request(&client, &url, &account).await?;
    if matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        account = codex_oauth::force_refresh_account_for_use(state, credential_id).await?;
        response = send_codex_model_request(&client, &url, &account).await?;
    }
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "OpenAI Codex returned HTTP {} while listing models",
            status.as_u16()
        ));
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "OpenAI Codex model list could not be read".to_owned())?;
        if bytes.len().saturating_add(chunk.len()) > MAX_CODEX_MODEL_BYTES {
            return Err("OpenAI Codex model list exceeded the 4 MiB size limit".to_owned());
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "OpenAI Codex returned an invalid model list".to_owned())?;
    let models = parse_codex_model_list(&value)?;
    if models.is_empty() {
        return Err("OpenAI Codex returned no supported models".to_owned());
    }

    Ok(models)
}
