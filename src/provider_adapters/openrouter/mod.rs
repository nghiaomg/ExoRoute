use super::*;
use std::time::Duration;

pub(super) struct OpenRouterAdapter;

pub(super) static OPENROUTER_ADAPTER: OpenRouterAdapter = OpenRouterAdapter;

#[derive(Clone, Copy, Debug)]
enum OpenRouterRequestError {
    Egress,
    Network,
    Read,
    TooLarge,
    InvalidJson,
    Timeout,
}

impl OpenRouterRequestError {
    fn message(self) -> &'static str {
        match self {
            Self::Egress => "OpenRouter provider egress validation failed",
            Self::Network => "could not reach OpenRouter usage API",
            Self::Read => "could not read OpenRouter usage response",
            Self::TooLarge => "OpenRouter usage response exceeded the configured size limit",
            Self::InvalidJson => "OpenRouter returned an invalid API key usage response",
            Self::Timeout => "OpenRouter usage request timed out",
        }
    }

    fn from_reqwest(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::Timeout
        } else {
            Self::Network
        }
    }
}

impl ProviderAdapter for OpenRouterAdapter {
    fn adapter_id(&self) -> &'static str {
        OPENROUTER_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        let path = match protocol {
            Protocol::ChatCompletions => "chat/completions",
            Protocol::Responses => "responses",
            Protocol::Messages => "messages",
        };
        openrouter_api_url(base_url, path)
    }

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        openrouter_api_url(base_url, "models")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn validate_config(
        &self,
        preset: &ProviderPreset,
        auth_type: &str,
        _base_url: &str,
        preferred_protocol: &str,
        supported_protocols: &[String],
        _api_keys_present: bool,
    ) -> Result<(), &'static str> {
        if auth_type != "bearer" || !preset.supported_auth_types.contains(&auth_type) {
            return Err("OpenRouter requires Bearer API-key authentication");
        }
        let valid_protocol =
            |protocol: &str| matches!(protocol, "chat_completions" | "responses" | "messages");
        if supported_protocols.is_empty()
            || !supported_protocols
                .iter()
                .any(|protocol| protocol == preferred_protocol)
            || supported_protocols
                .iter()
                .any(|protocol| !valid_protocol(protocol))
        {
            return Err("OpenRouter supports Chat Completions, Responses and Anthropic Messages");
        }
        Ok(())
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        let options = GenericModelDiscoveryRequest {
            auth_type: request.auth_type,
            auth_header: request.auth_header,
            custom_headers: request.custom_headers,
            preferred_protocol: request.preferred_protocol,
            credential: request.credential,
        };
        Box::pin(discover_generic_api_key_models_with_timeout(
            self,
            request.state,
            request.base_url,
            options,
            request
                .state
                .operational_settings()
                .settings
                .upstream
                .discovery_request_timeout,
        ))
    }

    fn test_api_key_credential<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, AdapterKeyTestOutcome> {
        Box::pin(test_openrouter_api_key(
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
        Box::pin(fetch_openrouter_usage(state, base_url, credential))
    }
}

fn openrouter_api_url(base_url: &str, endpoint: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base_url)
        .map_err(|_| "OpenRouter provider URL is invalid".to_owned())?;
    let mut path = url.path().trim_end_matches('/').to_owned();
    for suffix in [
        "/chat/completions",
        "/responses",
        "/messages",
        "/models",
        "/key",
    ] {
        if let Some(root) = path.strip_suffix(suffix) {
            path = root.to_owned();
            break;
        }
    }
    path = path.trim_end_matches('/').to_owned();
    if !path.ends_with("/v1") {
        path.push_str("/v1");
    }
    url.set_path(&format!("{path}/{endpoint}"));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

async fn openrouter_get_json(
    state: &AppState,
    base_url: &str,
    endpoint: &str,
    custom_headers: &std::collections::BTreeMap<String, String>,
    credential: &str,
) -> Result<(http::StatusCode, Option<Value>), OpenRouterRequestError> {
    let endpoint =
        openrouter_api_url(base_url, endpoint).map_err(|_| OpenRouterRequestError::Egress)?;
    let operational = state.operational_settings().settings;
    let upstream = operational.upstream;
    tokio::time::timeout(upstream.opencode_usage_timeout, async {
        let (endpoint, client) = egress::provider_client(
            endpoint.as_str(),
            state.config.allow_private_provider_urls,
            operational.connect_timeout.min(Duration::from_secs(3)),
            operational
                .request_timeout
                .min(upstream.opencode_usage_timeout),
            false,
            concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
            operational.upstream,
        )
        .await
        .map_err(|_| OpenRouterRequestError::Egress)?;
        let request =
            crate::provider_adapters::apply_custom_headers(client.get(endpoint), custom_headers)
                .map_err(|_| OpenRouterRequestError::Egress)?;
        let mut response = request
            .bearer_auth(credential)
            .header(http::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(OpenRouterRequestError::from_reqwest)?;
        let status = response.status();
        if !status.is_success() {
            return Ok((status, None));
        }
        if response
            .content_length()
            .is_some_and(|length| length > upstream.usage_response_max_bytes as u64)
        {
            return Err(OpenRouterRequestError::TooLarge);
        }
        let expected = response
            .content_length()
            .map(|length| length as usize)
            .unwrap_or(0);
        let mut body = Vec::with_capacity(expected.min(upstream.usage_response_max_bytes));
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            if error.is_timeout() {
                OpenRouterRequestError::Timeout
            } else {
                OpenRouterRequestError::Read
            }
        })? {
            if body.len().saturating_add(chunk.len()) > upstream.usage_response_max_bytes {
                return Err(OpenRouterRequestError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        let value =
            serde_json::from_slice(&body).map_err(|_| OpenRouterRequestError::InvalidJson)?;
        Ok((status, Some(value)))
    })
    .await
    .map_err(|_| OpenRouterRequestError::Timeout)?
}

fn read_optional_number(
    data: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<Option<f64>, String> {
    let Some(value) = data.get(field) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let number = value
        .as_f64()
        .filter(|number| number.is_finite() && *number >= 0.0)
        .ok_or_else(|| "OpenRouter API key usage response has an invalid shape".to_owned())?;
    Ok(Some(number))
}

fn read_required_nullable_number(
    data: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<Option<f64>, String> {
    if !data.contains_key(field) {
        return Err("OpenRouter API key usage response has an invalid shape".to_owned());
    }
    read_optional_number(data, field)
}

pub(super) fn parse_openrouter_key_usage(value: &Value) -> Result<ProviderUsageSnapshot, String> {
    let data = value
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| "OpenRouter API key usage response has an invalid shape".to_owned())?;
    let limit = read_required_nullable_number(data, "limit")?;
    let limit_remaining = read_required_nullable_number(data, "limit_remaining")?;
    let usage = read_optional_number(data, "usage")?;

    let reset_period = match data.get("limit_reset") {
        None | Some(Value::Null) => None,
        Some(Value::String(period)) => match period.as_str() {
            "daily" => Some(ProviderUsageResetPeriod::Daily),
            "weekly" => Some(ProviderUsageResetPeriod::Weekly),
            "monthly" => Some(ProviderUsageResetPeriod::Monthly),
            _ => None,
        },
        Some(_) => {
            return Err("OpenRouter API key usage response has an invalid shape".to_owned());
        }
    };
    let is_free_tier = match data.get("is_free_tier") {
        None | Some(Value::Null) => false,
        Some(value) => value
            .as_bool()
            .ok_or_else(|| "OpenRouter API key usage response has an invalid shape".to_owned())?,
    };

    let (used_percent, remaining_percent, used_amount, limit_amount, uncapped, limit_reached) =
        match (limit, limit_remaining) {
            (None, None) => (0.0, 0.0, usage, None, true, false),
            (Some(limit), Some(remaining)) if remaining <= limit && limit > 0.0 => {
                let used = limit - remaining;
                if let Some(reported_usage) = usage {
                    let tolerance = f64::EPSILON * limit.abs().max(1.0) * 16.0;
                    if reported_usage > limit || (reported_usage - used).abs() > tolerance {
                        return Err(
                            "OpenRouter API key usage response has an invalid shape".to_owned()
                        );
                    }
                }
                let used_percent = (used / limit * 100.0).clamp(0.0, 100.0);
                (
                    used_percent,
                    100.0 - used_percent,
                    Some(used),
                    Some(limit),
                    false,
                    remaining == 0.0,
                )
            }
            (Some(0.0), Some(0.0)) if usage.is_none_or(|reported| reported == 0.0) => {
                (100.0, 0.0, Some(0.0), Some(0.0), false, true)
            }
            _ => {
                return Err("OpenRouter API key usage response has an invalid shape".to_owned());
            }
        };

    Ok(ProviderUsageSnapshot {
        plan: Some(if is_free_tier {
            "OpenRouter Free Tier".to_owned()
        } else {
            "OpenRouter".to_owned()
        }),
        limit_reached,
        reset_credits_available: None,
        quotas: vec![ProviderUsageQuota {
            id: "openrouter_key_spending_cap".to_owned(),
            label: "API key spending cap".to_owned(),
            used_percent,
            remaining_percent,
            used_amount,
            limit_amount,
            unit: Some("USD".to_owned()),
            reset_at: None,
            window_seconds: None,
            uncapped,
            reset_period,
        }],
        credit_balance: None,
    })
}

pub(super) async fn test_openrouter_api_key(
    state: &AppState,
    base_url: &str,
    custom_headers: &std::collections::BTreeMap<String, String>,
    credential: &str,
) -> AdapterKeyTestOutcome {
    match openrouter_get_json(state, base_url, "key", custom_headers, credential).await {
        Ok((status, Some(value))) if status.is_success() => {
            match parse_openrouter_key_usage(&value) {
                Ok(_) => AdapterKeyTestOutcome {
                    test_passed: true,
                    status: Some(status.as_u16()),
                    message: "OpenRouter API key was verified with the account usage endpoint"
                        .to_owned(),
                },
                Err(error) => AdapterKeyTestOutcome {
                    test_passed: false,
                    status: Some(status.as_u16()),
                    message: error,
                },
            }
        }
        Ok((status, _)) => AdapterKeyTestOutcome {
            test_passed: false,
            status: Some(status.as_u16()),
            message: format!(
                "OpenRouter API key could not be verified (HTTP {})",
                status.as_u16()
            ),
        },
        Err(error) => AdapterKeyTestOutcome {
            test_passed: false,
            status: None,
            message: error.message().to_owned(),
        },
    }
}

pub(super) async fn fetch_openrouter_usage(
    state: &AppState,
    base_url: &str,
    credential: &str,
) -> Result<ProviderUsageSnapshot, String> {
    let (status, value) = openrouter_get_json(
        state,
        base_url,
        "key",
        &std::collections::BTreeMap::new(),
        credential,
    )
    .await
    .map_err(|error| error.message().to_owned())?;
    if status == http::StatusCode::UNAUTHORIZED || status == http::StatusCode::FORBIDDEN {
        return Err(format!(
            "OpenRouter rejected this API key while reading usage (HTTP {})",
            status.as_u16()
        ));
    }
    if !status.is_success() {
        return Err(format!(
            "OpenRouter usage endpoint returned HTTP {}",
            status.as_u16()
        ));
    }
    let value =
        value.ok_or_else(|| "OpenRouter API key usage response has an invalid shape".to_owned())?;
    parse_openrouter_key_usage(&value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn endpoints_preserve_api_version_and_protocol_paths() {
        let adapter = OpenRouterAdapter;
        let cases = [
            (
                Protocol::ChatCompletions,
                "https://openrouter.ai/api/v1/chat/completions",
            ),
            (
                Protocol::Responses,
                "https://openrouter.ai/api/v1/responses",
            ),
            (Protocol::Messages, "https://openrouter.ai/api/v1/messages"),
        ];

        for (protocol, expected) in cases {
            assert_eq!(
                adapter
                    .endpoint("https://openrouter.ai/api/v1", protocol, None)
                    .expect("protocol endpoint")
                    .as_str(),
                expected
            );
        }
        assert_eq!(
            adapter
                .model_list_endpoint("https://openrouter.ai/api/v1/key")
                .expect("models endpoint")
                .as_str(),
            "https://openrouter.ai/api/v1/models"
        );
        assert_eq!(
            openrouter_api_url("https://openrouter.ai", "key")
                .expect("key endpoint")
                .as_str(),
            "https://openrouter.ai/v1/key"
        );
        assert_eq!(
            openrouter_api_url("https://openrouter.ai/api/v1/chat/completions", "responses")
                .expect("protocol override endpoint")
                .as_str(),
            "https://openrouter.ai/api/v1/responses"
        );
    }

    #[test]
    fn capped_key_quota_uses_key_cap_fields_and_reset_period() {
        for (period, expected) in [
            ("daily", ProviderUsageResetPeriod::Daily),
            ("weekly", ProviderUsageResetPeriod::Weekly),
            ("monthly", ProviderUsageResetPeriod::Monthly),
        ] {
            let snapshot = parse_openrouter_key_usage(&json!({"data": {
                "limit": 100.0,
                "limit_remaining": 74.5,
                "usage": 25.5,
                "limit_reset": period,
                "is_free_tier": false
            }}))
            .expect("valid per-key spending cap");
            assert_eq!(snapshot.plan.as_deref(), Some("OpenRouter"));
            assert!(!snapshot.limit_reached);
            assert_eq!(snapshot.quotas.len(), 1);
            let quota = &snapshot.quotas[0];
            assert!(!quota.uncapped);
            assert_eq!(quota.used_amount, Some(25.5));
            assert_eq!(quota.limit_amount, Some(100.0));
            assert_eq!(quota.unit.as_deref(), Some("USD"));
            assert!((quota.used_percent - 25.5).abs() < f64::EPSILON);
            assert!((quota.remaining_percent - 74.5).abs() < f64::EPSILON);
            assert_eq!(quota.reset_period, Some(expected));
            assert_eq!(quota.reset_at, None);
        }
    }

    #[test]
    fn uncapped_free_tier_key_has_usage_but_no_progress_bar_values() {
        let snapshot = parse_openrouter_key_usage(&json!({"data": {
            "limit": null,
            "limit_remaining": null,
            "usage": 4.25,
            "is_free_tier": true,
            "limit_reset": null
        }}))
        .expect("valid uncapped key");
        assert_eq!(snapshot.plan.as_deref(), Some("OpenRouter Free Tier"));
        let quota = &snapshot.quotas[0];
        assert!(quota.uncapped);
        assert_eq!(quota.used_amount, Some(4.25));
        assert_eq!(quota.limit_amount, None);
        assert_eq!(quota.used_percent, 0.0);
        assert_eq!(quota.remaining_percent, 0.0);
        assert_eq!(quota.reset_period, None);
        assert_eq!(quota.reset_at, None);
    }

    #[test]
    fn zero_cap_is_a_reached_cap_and_unknown_reset_period_is_not_guessed() {
        let snapshot = parse_openrouter_key_usage(&json!({"data": {
            "limit": 0.0,
            "limit_remaining": 0.0,
            "usage": 0.0,
            "limit_reset": "yearly"
        }}))
        .expect("valid zero cap");
        assert!(snapshot.limit_reached);
        assert_eq!(snapshot.quotas[0].used_percent, 100.0);
        assert_eq!(snapshot.quotas[0].remaining_percent, 0.0);
        assert_eq!(snapshot.quotas[0].reset_period, None);
    }

    #[test]
    fn malformed_or_conflicting_cap_data_is_rejected() {
        let invalid = [
            json!({}),
            json!({"data": {}}),
            json!({"data": {"limit": 10.0}}),
            json!({"data": {"limit_remaining": 5.0}}),
            json!({"data": {"limit": -1.0, "limit_remaining": 0.0}}),
            json!({"data": {"limit": 10.0, "limit_remaining": -1.0}}),
            json!({"data": {"limit": 10.0, "limit_remaining": 11.0}}),
            json!({"data": {"limit": 0.0, "limit_remaining": 0.01}}),
            json!({"data": {"limit": "10", "limit_remaining": 5.0}}),
            json!({"data": {"limit": null, "limit_remaining": 0.0}}),
            json!({"data": {"limit": 10.0, "limit_remaining": null}}),
            json!({"data": {"limit": null, "limit_remaining": null, "usage": -0.1}}),
            json!({"data": {"limit": 10.0, "limit_remaining": 5.0, "usage": 6.0}}),
            json!({"data": {"limit": 10.0, "limit_remaining": 5.0, "usage": "5"}}),
            json!({"data": {"limit": 0.0, "limit_remaining": 0.0, "usage": 1.0}}),
            json!({"data": {"limit": 10.0, "limit_remaining": 5.0, "limit_reset": 30}}),
            json!({"data": {"limit": 10.0, "limit_remaining": 5.0, "is_free_tier": "true"}}),
        ];

        for value in invalid {
            assert!(
                parse_openrouter_key_usage(&value).is_err(),
                "expected malformed key usage to be rejected: {value}"
            );
        }
        assert!(serde_json::from_str::<Value>(r#"{"data":{"limit":NaN}}"#).is_err());
    }
}
