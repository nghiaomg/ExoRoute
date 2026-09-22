use super::*;

pub(super) struct CommandCodeAdapter;

pub(super) static COMMAND_CODE_ADAPTER: CommandCodeAdapter = CommandCodeAdapter;

impl ProviderAdapter for CommandCodeAdapter {
    fn adapter_id(&self) -> &'static str {
        COMMAND_CODE_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        let path = match protocol {
            Protocol::ChatCompletions => "chat/completions",
            Protocol::Messages => "messages",
            Protocol::Responses => {
                return Err("Command Code does not provide a native Responses endpoint".to_owned());
            }
        };
        command_code_url(base_url, "provider/v1", path)
    }

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        command_code_url(base_url, "provider/v1", "models")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn validate_config(
        &self,
        preset: &ProviderPreset,
        auth_type: &str,
        _base_url: &str,
        preferred_protocol: &str,
        supported_protocols: &[String],
        api_keys_present: bool,
    ) -> Result<(), &'static str> {
        if !preset.supported_auth_types.contains(&auth_type) {
            return Err("Command Code supports Bearer API keys or an unconfigured key flow");
        }
        if auth_type == "none" && api_keys_present {
            return Err("Command Code API keys require Bearer authentication");
        }
        if supported_protocols.is_empty()
            || supported_protocols
                .iter()
                .any(|protocol| !matches!(protocol.as_str(), "chat_completions" | "messages"))
            || !supported_protocols
                .iter()
                .any(|protocol| protocol == preferred_protocol)
        {
            return Err("Command Code supports Chat Completions and Anthropic Messages only");
        }
        Ok(())
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }

    fn test_api_key_credential<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, AdapterKeyTestOutcome> {
        Box::pin(test_command_code_api_key(
            request.state,
            request.base_url,
            request.custom_headers,
            request.credential,
        ))
    }

    fn fetch_api_key_usage<'a>(
        &'a self,
        state: &'a AppState,
        base_url: &'a str,
        credential: &'a str,
    ) -> AdapterFuture<'a, Result<ProviderUsageSnapshot, String>> {
        Box::pin(fetch_command_code_usage(state, base_url, credential))
    }

    fn api_key_auth_assist_start_url(
        &self,
        callback_url: &str,
        state: &str,
    ) -> Result<String, String> {
        let mut auth_url = reqwest::Url::parse("https://commandcode.ai/studio/auth/cli")
            .map_err(|_| "could not build the Command Code sign-in URL".to_owned())?;
        auth_url
            .query_pairs_mut()
            .append_pair("callback", callback_url)
            .append_pair("state", state);
        Ok(auth_url.to_string())
    }

    fn api_key_auth_assist_origins(&self) -> &'static [&'static str] {
        &[
            "https://commandcode.ai",
            "https://staging.commandcode.ai",
            "http://localhost:5173",
            "http://127.0.0.1:5173",
            "http://localhost:3000",
            "http://127.0.0.1:3000",
        ]
    }

    fn parse_api_key_auth_callback(&self, body: &[u8]) -> Result<ApiKeyAuthCallback, String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct CommandCodeCallbackPayload {
            api_key: String,
            state: String,
            user_id: Option<String>,
            user_name: Option<String>,
            key_name: Option<String>,
        }

        let payload: CommandCodeCallbackPayload = serde_json::from_slice(body)
            .map_err(|_| "invalid API-key auth callback payload".to_owned())?;
        Ok(ApiKeyAuthCallback {
            api_key: payload.api_key,
            state: payload.state,
            user_id: payload.user_id,
            user_name: payload.user_name,
            key_name: payload.key_name,
        })
    }
}

fn command_code_root_path(path: &str) -> &str {
    for suffix in [
        "/provider/v1/chat/completions",
        "/provider/v1/messages",
        "/provider/v1/models",
        "/provider/v1",
    ] {
        if let Some(root) = path.strip_suffix(suffix) {
            return root;
        }
    }
    path.strip_suffix('/').unwrap_or(path)
}

fn command_code_url(
    base_url: &str,
    api_path: &str,
    endpoint: &str,
) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base_url)
        .map_err(|_| "Command Code provider URL is invalid".to_owned())?;
    let root = command_code_root_path(url.path().trim_end_matches('/'));
    let root = if root == "/" { "" } else { root };
    url.set_path(&format!("{root}/{api_path}/{endpoint}"));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn command_code_aux_url(base_url: &str, endpoint: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base_url)
        .map_err(|_| "Command Code provider URL is invalid".to_owned())?;
    let root = command_code_root_path(url.path().trim_end_matches('/'));
    let root = if root == "/" { "" } else { root };
    url.set_path(&format!("{root}/{endpoint}"));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

pub(super) async fn test_command_code_api_key(
    state: &AppState,
    base_url: &str,
    custom_headers: &std::collections::BTreeMap<String, String>,
    credential: &str,
) -> AdapterKeyTestOutcome {
    let failed = |status, message: String| AdapterKeyTestOutcome {
        test_passed: false,
        status,
        message,
    };
    let endpoint = match command_code_aux_url(base_url, "alpha/whoami") {
        Ok(endpoint) => endpoint,
        Err(error) => return failed(None, error),
    };
    let operational = state.operational_settings().settings;
    let (endpoint, client) = match egress::provider_client(
        endpoint.as_str(),
        state.config.allow_private_provider_urls,
        operational
            .connect_timeout
            .min(std::time::Duration::from_secs(3)),
        operational
            .request_timeout
            .min(operational.upstream.command_code_usage_timeout),
        false,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        operational.upstream,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => return failed(None, format!("Provider egress check failed: {error}")),
    };
    let request = match crate::provider_adapters::apply_custom_headers(
        client.get(endpoint),
        custom_headers,
    ) {
        Ok(request) => request,
        Err(error) => return failed(None, error),
    };
    match tokio::time::timeout(
        operational.upstream.command_code_usage_timeout,
        request
            .bearer_auth(credential)
            .header(http::header::ACCEPT, "application/json")
            .send(),
    )
    .await
    {
        Ok(Ok(response)) if response.status().is_success() => AdapterKeyTestOutcome {
            test_passed: true,
            status: Some(response.status().as_u16()),
            message: "Command Code API key was verified by the account endpoint".to_owned(),
        },
        Ok(Ok(response)) => {
            let status = response.status().as_u16();
            failed(
                Some(status),
                format!("Command Code API key could not be verified (HTTP {status})"),
            )
        }
        Ok(Err(error)) => failed(
            None,
            format!(
                "Could not verify Command Code API key: {}",
                error.without_url()
            ),
        ),
        Err(_) => failed(
            None,
            "Command Code API key verification timed out".to_owned(),
        ),
    }
}

pub(super) async fn fetch_command_code_usage(
    state: &AppState,
    base_url: &str,
    credential: &str,
) -> Result<ProviderUsageSnapshot, String> {
    let whoami_url = command_code_aux_url(base_url, "alpha/whoami")?;
    let operational = state.operational_settings().settings;
    let (whoami_url, client) = egress::provider_client(
        whoami_url.as_str(),
        state.config.allow_private_provider_urls,
        operational
            .connect_timeout
            .min(std::time::Duration::from_secs(3)),
        operational
            .request_timeout
            .min(operational.upstream.command_code_usage_timeout),
        false,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        operational.upstream,
    )
    .await?;

    // Keep the timeout per upstream call. A single deadline around this whole
    // sequence would let an optional enrichment request hide the required
    // credits response when it consumes the entire budget.
    let whoami = match command_code_usage_json(
        &client,
        whoami_url.clone(),
        credential,
        operational.upstream.command_code_optional_usage_timeout,
        operational.upstream.usage_response_max_bytes,
    )
    .await
    {
        Ok((status @ (401 | 403), _)) => {
            return Err(format!(
                "Command Code rejected this API key while reading usage (HTTP {status})"
            ));
        }
        Ok((200..=299, value)) => value,
        Ok((_, _)) => None,
        Err(error) => {
            tracing::debug!(%error, "optional Command Code account endpoint is unavailable");
            None
        }
    };
    let org_id = whoami
        .as_ref()
        .and_then(|value| value.get("org"))
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .map(str::to_owned);

    let mut credits_url = command_code_aux_url(base_url, "alpha/billing/credits")?;
    if let Some(org_id) = org_id.as_deref() {
        credits_url.query_pairs_mut().append_pair("orgId", org_id);
    }
    let (credits_status, credits) = command_code_usage_json(
        &client,
        credits_url,
        credential,
        operational.upstream.command_code_usage_timeout,
        operational.upstream.usage_response_max_bytes,
    )
    .await?;
    if matches!(credits_status, 401 | 403) {
        return Err(format!(
            "Command Code rejected this API key while reading usage (HTTP {credits_status})"
        ));
    }
    if !matches!(credits_status, 200..=299) {
        return Err(format!(
            "Command Code credits endpoint returned HTTP {credits_status}"
        ));
    }
    let credits = credits
        .ok_or_else(|| "Command Code credits endpoint returned an empty response".to_owned())?;

    let mut subscriptions_url = command_code_aux_url(base_url, "alpha/billing/subscriptions")?;
    if let Some(org_id) = org_id.as_deref() {
        subscriptions_url
            .query_pairs_mut()
            .append_pair("orgId", org_id);
    }
    let subscriptions = command_code_usage_json(
        &client,
        subscriptions_url,
        credential,
        operational.upstream.command_code_optional_usage_timeout,
        operational.upstream.usage_response_max_bytes,
    )
    .await
    .ok()
    .filter(|(status, value)| (200..=299).contains(status) && value.is_some())
    .and_then(|(_, value)| value);

    let subscription_data = subscriptions.as_ref().and_then(|value| value.get("data"));
    let period_start = subscription_data
        .and_then(|value| value.get("currentPeriodStart"))
        .and_then(command_code_value_string);
    let mut summary_url = command_code_aux_url(base_url, "alpha/usage/summary")?;
    {
        let mut pairs = summary_url.query_pairs_mut();
        if let Some(org_id) = org_id.as_deref() {
            pairs.append_pair("orgId", org_id);
        }
        if let Some(period_start) = period_start.as_deref() {
            pairs.append_pair("since", period_start);
        }
    }
    let summary = command_code_usage_json(
        &client,
        summary_url,
        credential,
        operational.upstream.command_code_optional_usage_timeout,
        operational.upstream.usage_response_max_bytes,
    )
    .await
    .ok()
    .filter(|(status, value)| (200..=299).contains(status) && value.is_some())
    .and_then(|(_, value)| value);

    parse_command_code_usage(&credits, subscriptions.as_ref(), summary.as_ref())
        .ok_or_else(|| "Command Code credits response did not contain usage data".to_owned())
}

async fn command_code_usage_json(
    client: &reqwest::Client,
    url: reqwest::Url,
    credential: &str,
    request_timeout: std::time::Duration,
    max_bytes: usize,
) -> Result<(u16, Option<Value>), String> {
    let mut response = client
        .get(url)
        .timeout(request_timeout)
        .bearer_auth(credential)
        .header(http::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| {
            format!(
                "could not reach Command Code usage API: {}",
                error.without_url()
            )
        })?;
    let status = response.status().as_u16();
    if !(200..=299).contains(&status) {
        return Ok((status, None));
    }

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        format!(
            "could not read Command Code usage response: {}",
            error.without_url()
        )
    })? {
        if body.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!(
                "Command Code usage response exceeded the {} KiB limit",
                max_bytes / 1024
            ));
        }
        body.extend_from_slice(&chunk);
    }
    if body.is_empty() {
        return Ok((status, None));
    }
    let value = serde_json::from_slice(&body)
        .map_err(|_| "Command Code usage API returned invalid JSON".to_owned())?;
    Ok((status, Some(value)))
}

pub(super) fn parse_command_code_usage(
    credits_response: &Value,
    subscriptions_response: Option<&Value>,
    summary_response: Option<&Value>,
) -> Option<ProviderUsageSnapshot> {
    let credits = credits_response.get("credits")?.as_object()?;
    let window_limits = credits_response
        .get("windowLimits")
        .and_then(Value::as_object);
    let subscription_data = subscriptions_response.and_then(|value| value.get("data"));
    let plan = subscription_data
        .and_then(|value| value.get("planId"))
        .and_then(command_code_value_string);
    let mut quotas = Vec::with_capacity(3);
    if let Some(window_limits) = window_limits {
        for (id, label, seconds) in [
            ("five_hour", "5-hour window", 5 * 60 * 60),
            ("weekly", "Weekly window", 7 * 24 * 60 * 60),
            ("monthly", "Monthly window", 30 * 24 * 60 * 60),
        ] {
            let window = match id {
                "five_hour" => window_limits
                    .get("fiveHour")
                    .or_else(|| window_limits.get("five_hour"))
                    .or_else(|| window_limits.get("rolling")),
                "weekly" => window_limits
                    .get("weekly")
                    .or_else(|| window_limits.get("week")),
                "monthly" => window_limits
                    .get("monthly")
                    .or_else(|| window_limits.get("month")),
                _ => None,
            };
            let Some(window) = window else {
                continue;
            };
            let Some(limit) = window.get("cap").and_then(command_code_number) else {
                continue;
            };
            let Some(used) = window.get("used").and_then(command_code_number) else {
                continue;
            };
            if !limit.is_finite() || limit <= 0.0 || !used.is_finite() || used < 0.0 {
                continue;
            }
            let used_percent = (used / limit * 100.0).clamp(0.0, 100.0);
            quotas.push(ProviderUsageQuota {
                id: id.to_owned(),
                label: label.to_owned(),
                used_percent,
                remaining_percent: 100.0 - used_percent,
                used_amount: Some(used),
                limit_amount: Some(limit),
                unit: Some("upstream units".to_owned()),
                reset_at: window.get("resetAt").and_then(command_code_epoch_seconds),
                window_seconds: Some(seconds),
                uncapped: false,
                reset_period: None,
            });
        }
    }

    let monthly_remaining = credits
        .get("monthlyCredits")
        .and_then(command_code_number)
        .filter(|value| *value >= 0.0);
    let purchased_remaining = credits
        .get("purchasedCredits")
        .and_then(command_code_number)
        .filter(|value| *value >= 0.0);
    let free_remaining = credits
        .get("freeCredits")
        .and_then(command_code_number)
        .filter(|value| *value >= 0.0);
    let period_used = summary_response
        .and_then(|value| value.get("totalMonthlyCredits"))
        .and_then(command_code_number)
        .filter(|value| *value >= 0.0);
    let has_credit_data = monthly_remaining.is_some()
        || purchased_remaining.is_some()
        || free_remaining.is_some()
        || period_used.is_some();
    let period_ends_at = subscription_data
        .and_then(|value| value.get("currentPeriodEnd"))
        .and_then(command_code_epoch_seconds);

    if !quotas.iter().any(|quota| quota.id == "monthly")
        && let (Some(used), Some(remaining)) = (period_used, monthly_remaining)
    {
        let limit = used + remaining;
        if limit > 0.0 {
            let used_percent = (used / limit * 100.0).clamp(0.0, 100.0);
            quotas.push(ProviderUsageQuota {
                id: "monthly".to_owned(),
                label: "Monthly window".to_owned(),
                used_percent,
                remaining_percent: 100.0 - used_percent,
                used_amount: Some(used),
                limit_amount: Some(limit),
                unit: Some("credits".to_owned()),
                reset_at: period_ends_at,
                window_seconds: Some(30 * 24 * 60 * 60),
                uncapped: false,
                reset_period: None,
            });
        }
    }
    let quotas_saturated = quotas.iter().any(|quota| {
        quota.used_percent >= 100.0
            || quota.limit_amount.is_some_and(|limit| {
                limit > 0.0 && quota.used_amount.is_some_and(|used| used >= limit)
            })
    });
    let credits_exhausted = has_credit_data
        && monthly_remaining.is_some_and(|rem| rem <= 0.0)
        && purchased_remaining.unwrap_or(0.0) <= 0.0
        && free_remaining.unwrap_or(0.0) <= 0.0;
    let window_flag_limited = window_limits.is_some_and(|windows| {
        ["limited", "exceeded"]
            .iter()
            .any(|key| windows.get(*key).is_some_and(command_code_flag_is_true))
    });
    let limited = if !quotas.is_empty() {
        quotas_saturated || credits_exhausted
    } else {
        window_flag_limited || credits_exhausted
    };

    Some(ProviderUsageSnapshot {
        plan,
        limit_reached: limited,
        reset_credits_available: None,
        quotas,
        credit_balance: has_credit_data.then_some(ProviderUsageCreditBalance {
            unit: "credits".to_owned(),
            monthly_remaining,
            purchased_remaining,
            free_remaining,
            period_used,
            period_ends_at,
        }),
    })
}

fn command_code_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

fn command_code_flag_is_true(value: &Value) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_i64().is_some_and(|number| number != 0),
        Value::String(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "true" | "1" | "yes" | "limited" | "exceeded"
        ),
        _ => false,
    }
}

fn command_code_value_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .map(str::to_owned)
}

fn command_code_epoch_seconds(value: &Value) -> Option<i64> {
    if let Some(timestamp) = command_code_number(value) {
        if timestamp <= 0.0 || timestamp > i64::MAX as f64 {
            return None;
        }
        let timestamp = timestamp.trunc() as i64;
        return Some(if timestamp >= 1_000_000_000_000 {
            timestamp / 1000
        } else {
            timestamp
        });
    }
    parse_rfc3339_epoch(value.as_str()?)
}

pub(super) fn parse_rfc3339_epoch(value: &str) -> Option<i64> {
    let (date, time) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i64>().ok()?;
    let month = date_parts.next()?.parse::<i64>().ok()?;
    let day = date_parts.next()?.parse::<i64>().ok()?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day < 1 || day > days_in_month {
        return None;
    }

    let zone_start = time
        .char_indices()
        .skip(1)
        .find_map(|(index, character)| matches!(character, '+' | '-').then_some(index));
    let (clock, offset_seconds) = if let Some(index) = zone_start {
        let (clock, zone) = time.split_at(index);
        let sign = if zone.starts_with('-') { -1 } else { 1 };
        let mut zone_parts = zone[1..].split(':');
        let hours = zone_parts.next()?.parse::<i64>().ok()?;
        let minutes = zone_parts.next()?.parse::<i64>().ok()?;
        if zone_parts.next().is_some() || hours > 23 || minutes > 59 {
            return None;
        }
        (clock, sign * (hours * 3600 + minutes * 60))
    } else if let Some(clock) = time.strip_suffix('Z') {
        (clock, 0)
    } else {
        return None;
    };
    let clock = clock.split('.').next()?;
    let mut time_parts = clock.split(':');
    let hour = time_parts.next()?.parse::<i64>().ok()?;
    let minute = time_parts.next()?.parse::<i64>().ok()?;
    let second = time_parts.next()?.parse::<i64>().ok()?;
    if time_parts.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return None;
    }

    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days_since_epoch = era * 146_097 + day_of_era - 719_468;
    Some(days_since_epoch * 86_400 + hour * 3600 + minute * 60 + second - offset_seconds)
}
