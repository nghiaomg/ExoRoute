use super::account::{KiroAccount, email_from_jwt, unix_now};
use serde_json::json;

#[test]
fn parses_a_builder_id_token_payload() {
    let account = KiroAccount::from_token_payload(&json!({
        "accessToken": "access-token",
        "refreshToken": "refresh-token",
        "expiresIn": 3600,
        "clientId": "client-id",
        "clientSecret": "client-secret",
        "region": "us-east-1",
        "email": "dev@example.com",
    }))
    .expect("builder-id payload parses");
    assert_eq!(account.access_token, "access-token");
    assert_eq!(account.refresh_token.as_deref(), Some("refresh-token"));
    assert_eq!(account.auth_method, "builder-id");
    assert_eq!(account.client_id.as_deref(), Some("client-id"));
    assert_eq!(account.email.as_deref(), Some("dev@example.com"));
    assert!(account.expires_at > 0);
}

#[test]
fn parses_snake_case_and_social_payloads() {
    let account = KiroAccount::from_token_payload(&json!({
        "access_token": "access-token",
        "refresh_token": "refresh-token",
        "expires_in": 60,
        "auth_method": "imported",
        "profile_arn": "arn:aws:codewhisperer:us-east-1:123456789012:profile/abc",
    }))
    .expect("snake_case payload parses");
    assert_eq!(account.auth_method, "imported");
    assert_eq!(
        account.profile_arn.as_deref(),
        Some("arn:aws:codewhisperer:us-east-1:123456789012:profile/abc")
    );
    // expires_in clamps to the 60s floor.
    assert!(account.expires_at >= unix_now() + 55);
}

#[test]
fn rejects_payloads_without_an_access_token() {
    assert!(KiroAccount::from_token_payload(&json!({"refreshToken": "x"})).is_err());
    assert!(KiroAccount::from_token_payload(&json!({"accessToken": "  "})).is_err());
    assert!(KiroAccount::from_token_payload(&json!({})).is_err());
}

#[test]
fn rejects_oversized_and_control_character_tokens() {
    let huge = "a".repeat(super::MAX_TOKEN_BYTES + 1);
    assert!(KiroAccount::from_token_payload(&json!({"accessToken": huge})).is_err());
    assert!(KiroAccount::from_token_payload(&json!({"accessToken": "bad\ntoken"})).is_err());
}

#[test]
fn identity_prefers_email_then_arn_then_token_digest() {
    let base = json!({"accessToken": "token"});
    let mut account = KiroAccount::from_token_payload(&base).expect("parses");
    account.email = Some("Dev@Example.com".to_owned());
    assert_eq!(account.identity(), "email:dev@example.com");

    account.email = None;
    account.profile_arn = Some("arn:aws:profile".to_owned());
    assert_eq!(account.identity(), "arn:arn:aws:profile");

    account.profile_arn = None;
    let token_identity = account.identity();
    assert!(token_identity.starts_with("token:"));
    assert_eq!(token_identity.len(), "token:".len() + 32);
}

#[test]
fn display_name_falls_back_to_kiro_account() {
    let account = KiroAccount::from_token_payload(&json!({"accessToken": "t"})).expect("parses");
    assert_eq!(account.display_name(), "Kiro account");
}

#[test]
fn request_extras_pack_profile_arn_for_the_runtime() {
    let mut account =
        KiroAccount::from_token_payload(&json!({"accessToken": "t"})).expect("parses");
    account.profile_arn = Some("arn:x".to_owned());
    let extras = account.request_extras();
    assert_eq!(extras["profile_arn"], json!("arn:x"));
    assert_eq!(extras["auth_method"], json!("builder-id"));
}

#[test]
fn email_from_jwt_reads_the_payload_segment_only() {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let payload = URL_SAFE_NO_PAD.encode(br#"{"email":"dev@example.com"}"#);
    let token = format!("header.{payload}.signature");
    assert_eq!(email_from_jwt(&token).as_deref(), Some("dev@example.com"));
    assert!(email_from_jwt("not-a-jwt").is_none());
    assert!(email_from_jwt("a.b.c").is_none());
}
