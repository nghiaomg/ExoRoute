use super::{
    ANTIGRAVITY_ONBOARD_ATTEMPTS, ANTIGRAVITY_ONBOARD_RETRY_DELAY, AntigravityAccount,
    BOOTSTRAP_BASE_URL, MAX_OAUTH_BODY_BYTES, StatusCode, antigravity_ide_node_user_agent,
    antigravity_metadata, bootstrap_headers, bounded_text, extract_onboard_project_id,
    extract_project_id, extract_tier, oauth_client, onboard_user_body, post_json, read_json,
};
use serde_json::{Value, json};
use std::time::Duration;

pub(super) async fn enrich_account(
    account: &mut AntigravityAccount,
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<(), String> {
    let (userinfo_url, client) = oauth_client(
        super::USERINFO_URL,
        connect_timeout,
        request_timeout,
        upstream,
    )
    .await?;
    if let Ok(response) = client
        .get(userinfo_url)
        .bearer_auth(&account.access_token)
        .header("Accept", "application/json")
        .header("User-Agent", antigravity_ide_node_user_agent())
        .send()
        .await
        && response.status().is_success()
        && let Ok(value) = read_json(response, "Google user info", MAX_OAUTH_BODY_BYTES).await
    {
        account.email = value.get("email").and_then(Value::as_str).map(bounded_text);
    }

    let project = discover_project(account, connect_timeout, request_timeout, upstream).await?;
    account.project_id = project.project_id;
    account.tier = project.tier;
    if account.project_id.is_none() {
        return Err(
            "Google account has no Cloud Code project; complete Gemini Code Assist onboarding and reconnect Antigravity"
                .to_owned(),
        );
    }
    Ok(())
}

struct ProjectInfo {
    project_id: Option<String>,
    tier: Option<String>,
}

async fn discover_project(
    account: &AntigravityAccount,
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<ProjectInfo, String> {
    let metadata = antigravity_metadata();
    let headers = bootstrap_headers(&account.access_token);
    let load_body = json!({"metadata": metadata});
    let mut loaded = None;
    for base in [BOOTSTRAP_BASE_URL] {
        match post_json(
            base,
            "/v1internal:loadCodeAssist",
            &headers,
            &load_body,
            connect_timeout,
            request_timeout,
            upstream,
        )
        .await
        {
            Ok(value) => {
                loaded = Some(value);
                break;
            }
            Err(error)
                if matches!(
                    error.status,
                    Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
                ) =>
            {
                return Err(
                    "Antigravity OAuth account was rejected by Google; reconnect the account"
                        .to_owned(),
                );
            }
            Err(_) => {}
        }
    }
    let Some(value) = loaded else {
        return Ok(ProjectInfo {
            project_id: None,
            tier: None,
        });
    };
    let mut project_id = extract_project_id(&value);
    let tier = extract_tier(&value);
    if project_id.is_none() {
        for attempt in 0..ANTIGRAVITY_ONBOARD_ATTEMPTS {
            let onboard_body = onboard_user_body(tier.as_deref());
            match post_json(
                BOOTSTRAP_BASE_URL,
                "/v1internal:onboardUser",
                &headers,
                &onboard_body,
                connect_timeout,
                request_timeout,
                upstream,
            )
            .await
            {
                Ok(onboarded) => {
                    project_id = extract_onboard_project_id(&onboarded);
                    if project_id.is_none()
                        && let Ok(reloaded) = post_json(
                            BOOTSTRAP_BASE_URL,
                            "/v1internal:loadCodeAssist",
                            &headers,
                            &load_body,
                            connect_timeout,
                            request_timeout,
                            upstream,
                        )
                        .await
                    {
                        project_id = extract_project_id(&reloaded);
                    }
                }
                Err(error) => {
                    if matches!(
                        error.status,
                        Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
                    ) {
                        return Err(
                            "Antigravity OAuth account was rejected by Google; reconnect the account"
                                .to_owned(),
                        );
                    }
                    tracing::debug!(
                        attempt = attempt + 1,
                        status = ?error.status,
                        error = %error.message,
                        "Antigravity Cloud Code onboarding attempt failed"
                    );
                }
            }
            if project_id.is_some() || attempt + 1 == ANTIGRAVITY_ONBOARD_ATTEMPTS {
                break;
            }
            tokio::time::sleep(ANTIGRAVITY_ONBOARD_RETRY_DELAY).await;
        }
    }
    Ok(ProjectInfo { project_id, tier })
}
