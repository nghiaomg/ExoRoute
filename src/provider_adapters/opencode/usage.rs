use super::*;
use crate::security::egress;
use std::time::Duration;
pub(super) async fn fetch_opencode_go_usage(
    state: &AppState,
    base_url: &str,
    credential: &str,
) -> Result<ProviderUsageSnapshot, String> {
    let upstream = state.operational_settings().settings.upstream;
    tokio::time::timeout(upstream.opencode_usage_timeout, async {
        let endpoint = open_code_aux_endpoint(base_url, "usage")?;
        let operational = state.operational_settings().settings;
        let (endpoint, client) = egress::provider_client(
            endpoint.as_str(),
            state.config.allow_private_provider_urls,
            operational.connect_timeout.min(Duration::from_secs(3)),
            operational
                .request_timeout
                .min(upstream.opencode_usage_timeout),
            concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
            operational.upstream,
        )
        .await?;

        let mut response = client
            .get(endpoint)
            .bearer_auth(credential)
            .header(http::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|error| {
                format!("OpenCode Go usage request failed: {}", error.without_url())
            })?;
        let status = response.status();
        if status == http::StatusCode::UNAUTHORIZED || status == http::StatusCode::FORBIDDEN {
            return Err(format!(
                "OpenCode Go rejected this API key while reading usage (HTTP {})",
                status.as_u16()
            ));
        }
        if !status.is_success() {
            return Err(format!(
                "OpenCode Go usage endpoint returned HTTP {}",
                status.as_u16()
            ));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            format!(
                "could not read OpenCode Go usage response: {}",
                error.without_url()
            )
        })? {
            if body.len().saturating_add(chunk.len()) > upstream.usage_response_max_bytes {
                return Err(format!(
                    "OpenCode Go usage response exceeded the {} KiB limit",
                    upstream.usage_response_max_bytes / 1024
                ));
            }
            body.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&body)
            .map_err(|_| "OpenCode Go returned an invalid usage response".to_owned())?;
        parse_opencode_go_usage(&value)
    })
    .await
    .map_err(|_| {
        format!(
            "OpenCode Go usage request timed out after {} seconds",
            upstream.opencode_usage_timeout.as_secs()
        )
    })?
}

pub(super) fn parse_opencode_go_usage(value: &Value) -> Result<ProviderUsageSnapshot, String> {
    let usage = value
        .get("usage")
        .ok_or_else(|| "OpenCode Go usage response does not contain usage windows".to_owned())?;
    let windows = [
        ("rolling", "5-hour", Some(5 * 60 * 60)),
        ("weekly", "Weekly", Some(7 * 24 * 60 * 60)),
        ("monthly", "Monthly", None),
    ];
    let mut quotas = Vec::with_capacity(windows.len());
    let mut limit_reached = false;
    for (id, label, seconds) in windows {
        let window = usage
            .get(id)
            .ok_or_else(|| format!("OpenCode Go usage response is missing the {id} window"))?;
        let status = window
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !matches!(status, "ok" | "rate-limited") {
            return Err(format!(
                "OpenCode Go {id} usage window has an unsupported status"
            ));
        }
        let percent = if status == "rate-limited" {
            100.0
        } else {
            window
                .get("percent")
                .and_then(Value::as_f64)
                .filter(|percent| percent.is_finite() && (0.0..=100.0).contains(percent))
                .ok_or_else(|| format!("OpenCode Go {id} usage percentage is invalid"))?
        };
        let reset_at = window
            .get("resetsAt")
            .and_then(Value::as_str)
            .and_then(super::super::command_code::parse_rfc3339_epoch)
            .ok_or_else(|| format!("OpenCode Go {id} reset timestamp is invalid"))?;
        let saturated = status == "rate-limited" || percent >= 100.0;
        limit_reached |= saturated;
        quotas.push(ProviderUsageQuota {
            id: id.to_owned(),
            label: label.to_owned(),
            used_percent: percent,
            remaining_percent: (100.0 - percent).clamp(0.0, 100.0),
            used_amount: None,
            limit_amount: None,
            unit: Some("percent".to_owned()),
            reset_at: Some(reset_at),
            window_seconds: seconds,
            uncapped: false,
            reset_period: None,
        });
    }
    Ok(ProviderUsageSnapshot {
        plan: None,
        limit_reached,
        reset_credits_available: None,
        quotas,
        credit_balance: None,
    })
}
