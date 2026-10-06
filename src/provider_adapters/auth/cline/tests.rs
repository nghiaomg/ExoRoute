use super::*;
use crate::provider_adapters::auth::cline::payload::decode_embedded_payload;

#[test]
fn authorization_url_contains_loopback_callback_state_and_pkce() {
    let url = authorization_url("state-123", "challenge-456").expect("authorization URL");
    let parsed = reqwest::Url::parse(&url).expect("parse authorization URL");
    assert_eq!(
        parsed.origin().ascii_serialization(),
        "https://api.cline.bot"
    );
    let query = parsed
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        query.get("response_type").map(|value| value.as_ref()),
        Some("code")
    );
    assert_eq!(
        query.get("client_type").map(|value| value.as_ref()),
        Some("extension")
    );
    assert_eq!(
        query.get("redirect_uri").map(|value| value.as_ref()),
        Some(CALLBACK_URL)
    );
    assert_eq!(
        query.get("callback_url").map(|value| value.as_ref()),
        Some(CALLBACK_URL)
    );
    assert_eq!(
        query.get("code_challenge").map(|value| value.as_ref()),
        Some("challenge-456")
    );
    assert_eq!(
        query
            .get("code_challenge_method")
            .map(|value| value.as_ref()),
        Some("S256")
    );
    assert_eq!(
        query.get("state").map(|value| value.as_ref()),
        Some("state-123")
    );
}

#[test]
fn pkce_challenge_matches_rfc7636_vector() {
    assert_eq!(
        pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn token_payload_accepts_camel_and_snake_case_and_bounds_expiry() {
    let now = 1_700_000_000_i64;
    let account = ClineAccount::parse_at(
        &json!({
            "accessToken": "access",
            "refreshToken": "refresh",
            "expiresIn": 120,
            "accountId": "account-1",
            "email": "user@example.com"
        }),
        now,
    )
    .expect("parse Cline token payload");
    assert_eq!(account.access_token, "access");
    assert_eq!(account.refresh_token.as_deref(), Some("refresh"));
    assert_eq!(account.account_id.as_deref(), Some("account-1"));
    assert!(account.expires_at >= now + 60);
    assert!(account.expires_at <= now + 120);

    let snake = ClineAccount::parse(&json!({
        "access_token": "access-2",
        "refresh_token": "refresh-2",
        "expires_at": "2026-09-14T12:00:00Z",
        "account_id": "account-2"
    }))
    .expect("parse snake-case Cline token payload");
    assert_eq!(snake.account_id.as_deref(), Some("account-2"));
    assert_eq!(snake.refresh_token.as_deref(), Some("refresh-2"));
    assert!(snake.expires_at > 0);

    let numeric = ClineAccount::parse_at(
        &json!({
            "accessToken": "access-3",
            "expiresAt": (now + 600) * 1000
        }),
        now,
    )
    .expect("parse numeric Cline expiry");
    assert!(numeric.expires_at >= now + 599);
    assert!(numeric.expires_at <= now + 601);
}

#[test]
fn token_payload_rejects_missing_or_control_character_tokens() {
    assert!(ClineAccount::parse(&json!({"refreshToken": "refresh"})).is_err());
    assert!(
        ClineAccount::parse(&json!({
            "accessToken": "access\ninjected",
            "refreshToken": "refresh"
        }))
        .is_err()
    );
}

#[test]
fn embedded_authorization_payload_is_bounded_and_decoded() {
    let payload_bytes = serde_json::to_vec(&json!({
        "accessToken": "access",
        "refreshToken": "refresh",
        "expiresIn": 3600
    }))
    .expect("encode payload");
    let payload = general_purpose::URL_SAFE_NO_PAD.encode(&payload_bytes);
    let decoded = decode_embedded_payload(&payload).expect("embedded OAuth payload");
    assert_eq!(decoded["accessToken"], "access");
    assert!(is_embedded_callback_code(&payload));
    assert!(!is_embedded_callback_code("authorization-code"));
    assert!(decode_embedded_payload(&"x".repeat(MAX_OAUTH_BODY_BYTES + 1)).is_none());
}

#[test]
fn embedded_authorization_payload_matches_cline_suffix_and_uri_encoding() {
    let mut payload_bytes = serde_json::to_vec(&json!({
        "accessToken": "access",
        "refreshToken": "refresh",
        "expiresAt": "2030-01-01T00:00:00.000000000Z"
    }))
    .expect("encode payload");
    payload_bytes.extend_from_slice(b"opaque-signature");
    let payload = general_purpose::STANDARD.encode(payload_bytes);
    let encoded = payload.replace('=', "%3D");

    let decoded = decode_embedded_payload(&encoded).expect("embedded OAuth payload");
    assert_eq!(decoded["accessToken"], "access");
    assert_eq!(decoded["refreshToken"], "refresh");
    assert!(is_embedded_callback_code(&encoded));
}
