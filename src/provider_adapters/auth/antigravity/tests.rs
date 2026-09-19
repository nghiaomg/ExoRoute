use super::*;

#[test]
fn authorization_scopes_are_google_code_assist_scopes_without_openid() {
    assert!(
        SCOPES
            .iter()
            .all(|scope| scope.starts_with("https://www.googleapis.com/auth/"))
    );
    assert!(!SCOPES.contains(&"openid"));
    assert_eq!(default_models().len(), 14);
    for model in [
        "gemini-3.6-flash-tiered",
        "gemini-3.8-flash-tiered",
        "gemini-3.5-flash-lite",
        "gemini-3-flash",
    ] {
        assert!(default_models().contains(&model));
        assert!(!is_stale_unsupported_model(model), "{model}");
    }
    assert_eq!(
        normalize_model_id("gemini-3.1-pro-high"),
        "gemini-pro-agent"
    );
}

#[test]
fn authorization_url_uses_the_requested_dashboard_callback() {
    let callback = "http://localhost:8686/callback";
    let url = reqwest::Url::parse(
        &super::oauth::authorization_url_with_client_id(
            "test-client-id",
            "state",
            "challenge",
            Some(callback),
        )
        .expect("auth URL"),
    )
    .expect("parse auth URL");
    assert_eq!(
        url.query_pairs()
            .find(|(name, _)| name == "redirect_uri")
            .map(|(_, value)| value.into_owned())
            .as_deref(),
        Some(callback)
    );
}

#[test]
fn oauth_client_id_requires_configuration() {
    assert!(resolve_required_oauth_client_value(CLIENT_ID_ENV, None).is_err());
    assert!(resolve_required_oauth_client_value(CLIENT_ID_ENV, Some(String::new())).is_err());
}

#[test]
fn oauth_client_values_are_trimmed_and_bounded() {
    assert_eq!(
        resolve_required_oauth_client_value(
            CLIENT_ID_ENV,
            Some("  custom-client-id  ".to_owned()),
        )
        .unwrap(),
        "custom-client-id"
    );
    assert!(resolve_required_oauth_client_value(CLIENT_ID_ENV, Some("x".repeat(513)),).is_err());
    assert!(
        resolve_required_oauth_client_value(CLIENT_ID_ENV, Some("client\n-id".to_owned()),)
            .is_err()
    );
    assert_eq!(
        resolve_optional_oauth_client_value(CLIENT_SECRET_ENV, None).unwrap(),
        None
    );
    assert_eq!(
        resolve_optional_oauth_client_value(
            CLIENT_SECRET_ENV,
            Some("  custom-secret  ".to_owned())
        )
        .unwrap(),
        Some("custom-secret".to_owned())
    );
    assert_eq!(
        resolve_optional_oauth_client_value(CLIENT_SECRET_ENV, Some(String::new())).unwrap(),
        None
    );
    assert!(resolve_optional_oauth_client_value(CLIENT_SECRET_ENV, Some("x".repeat(513))).is_err());
}

#[test]
fn model_discovery_accepts_array_and_object_map_and_filters_non_chat_models() {
    let array = json!({"models":[
        {"modelId":"gemini-3.7-flash","quotaInfo":{"remainingFraction":0.5}},
        {"id":"gemini-3.7-flash-image","quotaInfo":{"remainingFraction":0.5}},
        {"id":"gemini-3.7-flash-low"}
    ]});
    assert_eq!(parse_model_list(&array), vec!["gemini-3.7-flash-tiered"]);
    let map = json!({
        "claude-sonnet-4-6":{"displayName":"Claude","quotaInfo":{"remainingFraction":0.4}},
        "internal-test":{}
    });
    assert_eq!(parse_model_list(&map), vec!["claude-sonnet-4-6"]);
    let nested_map = json!({"models":{
        "gemini-3.7-flash":{"displayName":"Flash","quotaInfo":{"remainingFraction":0.5}}
    }});
    assert_eq!(
        parse_model_list(&nested_map),
        vec!["gemini-3.7-flash-tiered"]
    );
}

#[test]
fn incomplete_discovery_is_supplemented_with_default_models() {
    let models = with_default_models(vec!["gpt-oss-120b-medium".to_owned()]);

    assert_eq!(models.len(), default_models().len());
    assert!(models.iter().all(|model| {
        default_models().contains(&model.as_str()) || model == "gpt-oss-120b-medium"
    }));
    for model in [
        "gemini-3.6-flash-tiered",
        "gemini-3.8-flash-tiered",
        "gemini-3.5-flash-lite",
        "gemini-3-flash",
    ] {
        assert!(models.iter().any(|candidate| candidate == model));
    }
}

#[test]
fn model_discovery_requires_quota_info_on_object_rows() {
    let payload = json!({
        "models": {
            "gemini-3.7-flash-high": {"displayName": "Gemini 3.7 Flash (High)"},
            "gemini-3.7-flash-medium": {
                "displayName": "Gemini 3.7 Flash (Medium)",
                "quotaInfo": {}
            },
            "gemini-3.7-flash-low": {
                "displayName": "Gemini 3.7 Flash (Low)",
                "quotaInfo": {"remainingFraction": 0.5}
            }
        }
    });
    assert_eq!(parse_model_list(&payload), vec!["gemini-3.7-flash-low"]);
}

#[test]
fn model_discovery_drops_retired_chat_models_and_unknown_uppercase_slots() {
    let payload = json!({
        "models": {
            "gemini-3.6-flash-high": {
                "displayName": "Gemini 3.6 Flash (High)",
                "quotaInfo": {"remainingFraction": 0.5}
            },
            "gemini-3.5-flash": {
                "displayName": "Gemini 3.5 Flash",
                "quotaInfo": {"remainingFraction": 0.5}
            },
            "MODEL_GOOGLE_GEMINI_3_9_FLASH": {
                "displayName": "Unknown enum slot",
                "quotaInfo": {"remainingFraction": 0.5}
            },
            "gemini-3.7-flash-high": {
                "displayName": "Gemini 3.7 Flash (High)",
                "quotaInfo": {"remainingFraction": 0.5}
            }
        }
    });
    assert_eq!(parse_model_list(&payload), vec!["gemini-3.7-flash-high"]);
}

#[test]
fn stale_placeholder_and_legacy_ids_are_pruned_from_stored_catalogs() {
    for stale in [
        "MODEL_PLACEHOLDER_M26",
        "MODEL_GOOGLE_GEMINI_2_5_FLASH",
        "gemini-3.6-flash-low",
        "gemini-3.7-flash-image",
    ] {
        assert!(is_stale_unsupported_model(stale), "{stale}");
    }
    for live in [
        "gemini-3.7-flash-high",
        "claude-sonnet-4-6",
        "gpt-oss-120b-medium",
    ] {
        assert!(!is_stale_unsupported_model(live), "{live}");
    }
}

#[test]
fn model_discovery_accepts_string_ids_without_exposing_internal_placeholders() {
    let payload = json!({
        "models": [
            "gemini-3.7-flash-high",
            "gemini-3.7-flash-medium",
            "gemini-3.7-flash-low",
            "MODEL_PLACEHOLDER_M26",
            "ag/claude-sonnet-4-6",
            "gemini-3.7-flash",
            "MODEL_OPENAI_GPT_OSS_120B_MEDIUM"
        ]
    });

    assert_eq!(
        parse_model_list(&payload),
        vec![
            "gemini-3.7-flash-high",
            "gemini-3.7-flash-medium",
            "gemini-3.7-flash-low",
            "claude-sonnet-4-6",
            "gemini-3.7-flash-tiered",
            "gpt-oss-120b-medium"
        ]
    );
}

#[test]
fn model_discovery_rejects_payload_containing_only_internal_placeholders() {
    let payload = json!({
        "models": [
            "MODEL_PLACEHOLDER_M26",
            "MODEL_GOOGLE_GEMINI_2_5_FLASH"
        ]
    });

    assert!(parse_model_list(&payload).is_empty());
}

#[test]
fn model_discovery_drops_hidden_rows_without_quota_info() {
    let payload = json!({
        "models": {
            "gemini-3.7-flash-high": {
                "displayName": "Gemini 3.7 Flash (High)",
                "quotaInfo": {"remainingFraction": 0.5}
            },
            "MODEL_PLACEHOLDER_M26": {"displayName": "Placeholder"},
            "hidden-preview": {
                "displayName": "Hidden",
                "hidden": true,
                "quotaInfo": {"remainingFraction": 0.5}
            }
        }
    });
    assert_eq!(parse_model_list(&payload), vec!["gemini-3.7-flash-high"]);
}

#[test]
fn stale_raw_model_slot_is_pruned_from_stored_catalogs() {
    assert!(is_stale_unsupported_model(
        "MODEL_OPENAI_GPT_OSS_120B_MEDIUM"
    ));
}

#[test]
fn onboarding_uses_cloud_code_field_names_and_reads_the_completed_project() {
    let body = onboard_user_body(Some("free-tier"));
    assert_eq!(
        body.get("tierId").and_then(Value::as_str),
        Some("free-tier")
    );
    assert!(body.get("tier_id").is_none());

    let response = json!({
        "done": true,
        "response": {"cloudaicompanionProject": {"id": "project-123"}}
    });
    assert_eq!(
        extract_onboard_project_id(&response).as_deref(),
        Some("project-123")
    );

    let loaded = json!({"allowedTiers":[{"id":"free-tier"}]});
    assert_eq!(extract_tier(&loaded).as_deref(), Some("free-tier"));

    let paid = json!({"paidTier":{"id":"pro-tier"}});
    assert_eq!(extract_tier(&paid).as_deref(), Some("pro-tier"));

    let paid_tier_overrides_current_tier =
        json!({"currentTier":{"id":"free-tier"},"paidTier":{"id":"pro-tier"}});
    assert_eq!(
        extract_tier(&paid_tier_overrides_current_tier).as_deref(),
        Some("pro-tier")
    );
}

#[test]
fn quota_fractions_are_clamped_and_weekly_groups_are_normalized() {
    let available =
        json!({"buckets":[{"modelId":"gemini-3.7-flash","quotaInfo":{"remainingFraction":1.4}}]});
    let summary = json!({"groups":[{"displayName":"Gemini Models","buckets":[{"bucketId":"gemini-weekly","displayName":"Weekly Limit","remainingFraction":-1.0,"resetAt":1800000000}]}]});
    let mut quotas = parse_model_quotas(Some(&available), None);
    append_weekly_quotas(&mut quotas, Some(&summary));
    assert_eq!(quotas[0].remaining_percent, 100.0);
    assert_eq!(quotas[1].remaining_percent, 0.0);
    assert_eq!(
        quotas[1].reset_period,
        Some(ProviderUsageResetPeriod::Weekly)
    );
}

#[test]
fn model_quotas_deduplicate_aliases_after_model_normalization() {
    let quota = json!({
        "buckets": [
            {"modelId":"gemini-3.7-flash-high","quotaInfo":{"remainingFraction":0.2}},
            {"modelId":"gemini-3.7-flash-low","quotaInfo":{"remainingFraction":0.8}}
        ]
    });

    let quotas = parse_model_quotas(None, Some(&quota));

    assert_eq!(quotas.len(), 1);
    assert_eq!(quotas[0].id, "gemini-3.7-flash-tiered");
    assert_eq!(quotas[0].remaining_percent, 20.0);
}

#[test]
fn quota_response_is_aggregated_into_four_stable_bars_for_paid_accounts() {
    let models = json!({
        "buckets": [
            {"modelId":"gemini-3.7-flash-high","quotaInfo":{"remainingFraction":0.2,"resetAt":1800000000}},
            {"modelId":"claude-sonnet-4-6","quotaInfo":{"remainingFraction":0.9,"resetAt":1800000001}},
            {"modelId":"gpt-oss-120b-medium","quotaInfo":{"remainingFraction":0.8,"resetAt":1800000002}}
        ]
    });
    let summary = json!({
        "groups": [
            {"displayName":"Gemini Models","buckets":[{"bucketId":"gemini-weekly","displayName":"Weekly Limit","remainingFraction":0.4,"resetAt":1800000010}]},
            {"displayName":"Claude and GPT models","buckets":[{"bucketId":"claude-gpt-weekly","displayName":"Weekly Limit","remainingFraction":0.9,"resetAt":1800000020}]}
        ]
    });
    let model_quotas = parse_model_quotas(None, Some(&models));
    let mut weekly_quotas = Vec::new();
    append_weekly_quotas(&mut weekly_quotas, Some(&summary));

    let quotas = summarize_antigravity_quotas(&model_quotas, &weekly_quotas, Some("paid-tier"));

    assert_eq!(
        quotas
            .iter()
            .map(|quota| quota.id.as_str())
            .collect::<Vec<_>>(),
        ["five_hour", "weekly", "gpt", "claude"]
    );
    assert_eq!(quotas[0].remaining_percent, 20.0);
    assert_eq!(quotas[1].remaining_percent, 40.0);
    assert_eq!(quotas[2].remaining_percent, 80.0);
    assert_eq!(quotas[3].remaining_percent, 90.0);
    assert_eq!(quotas[0].window_seconds, Some(5 * 60 * 60));
    assert_eq!(quotas[1].window_seconds, Some(7 * 24 * 60 * 60));
}

#[test]
fn free_tier_does_not_expose_misleading_model_window_bars() {
    let models = json!({
        "buckets": [{"modelId":"gemini-3.7-flash-high","quotaInfo":{"remainingFraction":0.2}}]
    });
    let summary = json!({
        "groups": [{"displayName":"Gemini Models","buckets":[{"bucketId":"gemini-weekly","displayName":"Weekly Limit","remainingFraction":0.4}]}]
    });
    let model_quotas = parse_model_quotas(None, Some(&models));
    let mut weekly_quotas = Vec::new();
    append_weekly_quotas(&mut weekly_quotas, Some(&summary));

    let quotas = summarize_antigravity_quotas(&model_quotas, &weekly_quotas, Some("free-tier"));

    assert_eq!(
        quotas
            .iter()
            .map(|quota| quota.id.as_str())
            .collect::<Vec<_>>(),
        ["weekly"]
    );
}

#[test]
fn duplicate_weekly_buckets_keep_the_lowest_remaining_value() {
    let summary = json!({
        "groups": [{
            "displayName":"Gemini Models",
            "buckets":[
                {"bucketId":"gemini-weekly-a","displayName":"Weekly Limit","remainingFraction":0.8,"resetAt":1800000020},
                {"bucketId":"gemini-weekly-b","displayName":"Weekly Limit","remainingFraction":0.4,"resetAt":1800000010}
            ]
        }]
    });
    let mut weekly_quotas = Vec::new();
    append_weekly_quotas(&mut weekly_quotas, Some(&summary));

    assert_eq!(weekly_quotas.len(), 1);
    assert_eq!(weekly_quotas[0].id, "gemini_weekly");
    assert_eq!(weekly_quotas[0].remaining_percent, 40.0);
    assert_eq!(weekly_quotas[0].reset_at, Some(1800000010));
}

#[test]
fn account_validation_rejects_invalid_profile() {
    let account = AntigravityAccount {
        access_token: "token".to_owned(),
        refresh_token: None,
        expires_at: 0,
        scope: None,
        email: None,
        project_id: None,
        tier: None,
        client_profile: "unknown".to_owned(),
    };
    assert!(validate_account(&account).is_err());
}

#[test]
fn cloud_code_bootstrap_headers_match_the_antigravity_ide_contract() {
    let bootstrap = bootstrap_headers("token");
    let runtime = runtime_headers("token");
    assert_eq!(bootstrap.len(), 3);
    assert!(
        bootstrap
            .iter()
            .any(|(name, value)| { *name == "User-Agent" && *value == antigravity_user_agent() })
    );
    assert!(bootstrap.iter().all(|(name, _)| {
        !matches!(
            *name,
            "Accept" | "X-Client-Name" | "X-Client-Version" | "X-Goog-Api-Client"
        )
    }));
    assert!(
        runtime
            .iter()
            .any(|(name, value)| *name == "X-Client-Name" && value == "antigravity")
    );
    assert!(runtime.iter().any(
        |(name, value)| *name == "X-Client-Version" && *value == antigravity_ide_version()
    ));
}

#[test]
fn ide_version_override_is_validated_and_bounded() {
    assert_eq!(resolve_ide_version(None), ANTIGRAVITY_IDE_VERSION);
    assert_eq!(
        resolve_ide_version(Some(String::new())),
        ANTIGRAVITY_IDE_VERSION
    );
    assert_eq!(
        resolve_ide_version(Some("  v2.12.0  ".to_owned())),
        "2.12.0"
    );
    for invalid in ["2.11", "2.11.0.1", "2.11.x", "2.11.0\n", &"9".repeat(33)] {
        assert_eq!(
            resolve_ide_version(Some(invalid.to_owned())),
            ANTIGRAVITY_IDE_VERSION,
            "{invalid}"
        );
    }
}
