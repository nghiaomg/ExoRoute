use super::*;
use crate::provider_adapters::auth::codex::usage::usage_core::unix_now;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::json;

#[test]
fn authorization_url_contains_pkce_and_expected_callback() {
    let url = reqwest::Url::parse(
        &authorization_url("random-state", &pkce_challenge("verifier")).unwrap(),
    )
    .unwrap();
    let query = url
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(query["state"], "random-state");
    assert_eq!(query["redirect_uri"], CALLBACK_URL);
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(
        query["code_challenge"],
        "iMnq5o6zALKXGivsnlom_0F5_WYda32GHkxlV7mq7hQ"
    );
}

#[test]
fn account_metadata_is_derived_without_trusting_decoded_claims_for_auth() {
    let payload = URL_SAFE_NO_PAD.encode(
            br#"{"email":"user@example.com","https://api.openai.com/auth":{"chatgpt_account_id":"acct-123","chatgpt_plan_type":"plus"}}"#,
        );
    let token = format!("header.{payload}.signature");
    let mut account = CodexAccount {
        access_token: token,
        refresh_token: Some("refresh".to_owned()),
        id_token: None,
        expires_at: 123,
        account_id: None,
        chatgpt_user_id: None,
        email: None,
        plan: None,
    };
    account.refresh_metadata();
    assert_eq!(account.email.as_deref(), Some("user@example.com"));
    assert_eq!(account.account_id.as_deref(), Some("acct-123"));
    assert_eq!(account.plan.as_deref(), Some("plus"));
}

#[test]
fn account_metadata_reads_per_user_identity_from_auth_claims_and_subject() {
    let access_payload =
        URL_SAFE_NO_PAD.encode(br#"{"https://api.openai.com/auth":{"user_id":"user-from-auth"}}"#);
    let access_token = format!("header.{access_payload}.signature");
    let id_payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"user-from-sub"}"#);
    let id_token = format!("header.{id_payload}.signature");
    let mut account = CodexAccount {
        access_token,
        refresh_token: None,
        id_token: Some(id_token),
        expires_at: 123,
        account_id: None,
        chatgpt_user_id: None,
        email: None,
        plan: None,
    };

    account.refresh_metadata();

    assert_eq!(account.chatgpt_user_id.as_deref(), Some("user-from-auth"));
}

#[test]
fn account_metadata_falls_back_to_jwt_subject_for_per_user_identity() {
    let payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"user-from-sub"}"#);
    let token = format!("header.{payload}.signature");
    let mut account = CodexAccount {
        access_token: token,
        refresh_token: None,
        id_token: None,
        expires_at: 123,
        account_id: None,
        chatgpt_user_id: None,
        email: None,
        plan: None,
    };

    account.refresh_metadata();

    assert_eq!(account.chatgpt_user_id.as_deref(), Some("user-from-sub"));
}

#[test]
fn usage_parser_reads_plan_windows_and_reset_credits() {
    let data = json!({
        "plan_type": "plus",
        "rate_limit": {
            "limit_reached": false,
            "primary_window": {
                "used_percent": 37.5,
                "reset_at": 1_800_000_000_000_i64,
                "limit_window_seconds": 10_800
            },
            "secondary_window": {
                "used_percent": "92",
                "reset_after_seconds": 3600,
                "limit_window_seconds": 604_800
            }
        },
        "rate_limit_reset_credits": { "available_count": 2 }
    });

    let usage = parse_usage_snapshot(&data);
    assert_eq!(usage.plan.as_deref(), Some("plus"));
    assert!(!usage.limit_reached);
    assert_eq!(usage.reset_credits_available, Some(2));
    assert_eq!(usage.quotas.len(), 2);
    assert_eq!(usage.quotas[0].id, "session");
    assert_eq!(usage.quotas[0].used_percent, 37.5);
    assert_eq!(usage.quotas[0].remaining_percent, 62.5);
    assert_eq!(usage.quotas[0].reset_at, Some(1_800_000_000));
    assert_eq!(usage.quotas[1].id, "weekly");
    assert_eq!(usage.quotas[1].used_percent, 92.0);
    assert_eq!(usage.quotas[1].label, "Weekly");
    assert!(
        usage.quotas[1]
            .reset_at
            .is_some_and(|reset| reset > unix_now())
    );
}

#[test]
fn usage_parser_includes_review_spark_and_additional_limits_once() {
    let data = json!({
        "rate_limit": {
            "primary_window": { "used_percent": 120 },
            "secondary_window": { "used_percent": -4 }
        },
        "additional_rate_limits": [
            {
                "limit_name": "code_review",
                "rate_limit": {
                    "primary_window": { "used_percent": 10 },
                    "secondary_window": { "used_percent": 20 }
                }
            },
            {
                "limit_name": "gpt-5.3-codex-spark",
                "rate_limit": {
                    "primary_window": { "used_percent": 30 }
                }
            },
            {
                "limit_name": "image_generation",
                "rate_limit": {
                    "primary_window": { "used_percent": 40 }
                }
            }
        ],
        "rate_limits_by_limit_id": {
            "codex": { "primary_window": { "used_percent": 99 } },
            "code_review": { "primary_window": { "used_percent": 11 } }
        }
    });

    let usage = parse_usage_snapshot(&data);
    let by_id = usage
        .quotas
        .iter()
        .map(|quota| (quota.id.as_str(), quota))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(by_id.len(), usage.quotas.len());
    assert_eq!(by_id["session"].used_percent, 100.0);
    assert_eq!(by_id["weekly"].used_percent, 0.0);
    assert_eq!(by_id["code_review_session"].used_percent, 11.0);
    assert_eq!(by_id["code_review_weekly"].label, "Code review · Weekly");
    assert_eq!(by_id["spark_session"].used_percent, 30.0);
    assert_eq!(by_id["image_generation_session"].used_percent, 40.0);
}

#[test]
fn usage_parser_labels_long_window_as_monthly() {
    let data = json!({
        "rate_limit": {
            "secondary_window": {
                "used_percent": 12,
                "limit_window_seconds": 2_592_000
            }
        }
    });

    let usage = parse_usage_snapshot(&data);
    assert_eq!(usage.quotas[0].id, "monthly");
    assert_eq!(usage.quotas[0].label, "Monthly");
}

#[test]
fn usage_parser_bounds_returned_quota_count() {
    let additional_rate_limits = (0..MAX_USAGE_QUOTAS + 20)
        .map(|index| {
            json!({
                "limit_name": format!("feature_{index}"),
                "rate_limit": { "primary_window": { "used_percent": index } }
            })
        })
        .collect::<Vec<_>>();
    let data = json!({ "additional_rate_limits": additional_rate_limits });

    let usage = parse_usage_snapshot(&data);
    assert_eq!(usage.quotas.len(), MAX_USAGE_QUOTAS);
}
