use super::device::{
    API_VERSION, DevicePoll, MUSE_USER_AGENT, mint_api_key, mint_request_body,
    parse_device_code_response, parse_mint_response, parse_token_response,
};
use super::*;
use crate::support::mock_upstream::{MockResponse, spawn_mock_http_server};
use crate::support::test_support::{ProviderSeed, TestDatabase, seed_provider, seed_provider_key};
use serde_json::json;

const TEST_MASTER_KEY: [u8; 32] = [47_u8; 32];
const OK: http::StatusCode = http::StatusCode::OK;

#[test]
fn endpoints_route_responses_and_chat_completions() {
    let adapter = MuseAdapter;
    assert_eq!(
        adapter
            .endpoint("https://api.meta.ai/v1", Protocol::Responses, None)
            .expect("responses endpoint")
            .as_str(),
        "https://api.meta.ai/v1/responses"
    );
    assert_eq!(
        adapter
            .endpoint("https://api.meta.ai/v1", Protocol::ChatCompletions, None)
            .expect("chat endpoint")
            .as_str(),
        "https://api.meta.ai/v1/chat/completions"
    );
    assert_eq!(
        adapter
            .model_list_endpoint("https://api.meta.ai/v1")
            .expect("catalog endpoint")
            .as_str(),
        "https://api.meta.ai/v1/models"
    );
    // A row that already stores a full endpoint still resolves, and the query
    // and fragment are dropped so a stored endpoint cannot smuggle a parameter.
    for (stored, suffix, expected) in [
        (
            "https://api.meta.ai/v1",
            "responses",
            "https://api.meta.ai/v1/responses",
        ),
        (
            "https://api.meta.ai/v1/responses",
            "responses",
            "https://api.meta.ai/v1/responses",
        ),
        (
            "https://api.meta.ai/v1/chat/completions?x=1",
            "chat/completions",
            "https://api.meta.ai/v1/chat/completions",
        ),
        (
            "https://api.meta.ai",
            "chat/completions",
            "https://api.meta.ai/chat/completions",
        ),
    ] {
        assert_eq!(
            muse_url(stored, suffix).expect("resolved url").as_str(),
            expected,
            "{stored} + {suffix} must resolve to {expected}"
        );
    }
    assert!(
        adapter
            .endpoint("https://api.meta.ai/v1", Protocol::Messages, None)
            .is_err(),
        "Muse has no Messages upstream"
    );
    assert!(muse_url("not a url", "models").is_err());
    assert!(muse_url("mailto:ops@example.com", "models").is_err());
}

#[test]
fn config_validation_matches_the_preset() {
    let adapter = MuseAdapter;
    let preset = preset_for_adapter(MUSE_ADAPTER_ID).expect("muse preset");
    let protocols = ["responses".to_owned(), "chat_completions".to_owned()];
    assert!(
        adapter
            .validate_config(
                preset,
                "bearer",
                "https://api.meta.ai/v1",
                "responses",
                &protocols,
                true
            )
            .is_ok()
    );
    assert!(
        adapter
            .validate_config(
                preset,
                "bearer",
                "https://api.meta.ai/v1",
                "chat_completions",
                &protocols,
                true
            )
            .is_ok()
    );
    for (auth_type, protocol, supported) in [
        ("none", "responses", protocols.to_vec()),
        ("bearer", "messages", protocols.to_vec()),
        ("bearer", "responses", vec!["messages".to_owned()]),
    ] {
        assert!(
            adapter
                .validate_config(
                    preset,
                    auth_type,
                    "https://api.meta.ai/v1",
                    protocol,
                    &supported,
                    true
                )
                .is_err(),
            "{auth_type}/{protocol}/{supported:?} must not validate"
        );
    }
}

#[test]
fn a_device_grant_is_validated_and_bounded() {
    let authorization = parse_device_code_response(&json!({
        "device_code": "device-1",
        "user_code": "ABCD-EFGH",
        "verification_uri": "https://www.meta.ai/device",
        "expires_in": 300,
        "interval": 5,
    }))
    .expect("usable device grant");
    assert_eq!(authorization.device_code, "device-1");
    assert_eq!(authorization.user_code, "ABCD-EFGH");
    assert_eq!(authorization.verification_uri, "https://www.meta.ai/device");
    assert_eq!(authorization.expires_in.as_secs(), 300);
    assert_eq!(authorization.interval.as_secs(), 5);

    // Meta publishes the cadence; a missing or absurd one is clamped instead of
    // being trusted.
    let fallback = parse_device_code_response(&json!({
        "device_code": "device-1",
        "user_code": "ABCD-EFGH",
        "verification_uri": "https://www.meta.ai/device",
        "expires_in": 0,
        "interval": 9999,
    }))
    .expect("usable device grant");
    assert_eq!(fallback.interval.as_secs(), 60);
    assert_eq!(fallback.expires_in.as_secs(), 900);

    for invalid in [
        json!({"user_code": "A", "verification_uri": "https://www.meta.ai/d"}),
        json!({"device_code": "d", "verification_uri": "https://www.meta.ai/d"}),
        json!({"device_code": "d", "user_code": "A"}),
        json!({"device_code": "d", "user_code": "A", "verification_uri": "http://www.meta.ai/d"}),
        json!({"device_code": "d", "user_code": "A", "verification_uri": "https://u:p@www.meta.ai/d"}),
        json!({"device_code": "d", "user_code": "A", "verification_uri": "https://www.meta.ai/d#f"}),
    ] {
        assert!(
            parse_device_code_response(&invalid).is_err(),
            "expected rejection: {invalid}"
        );
    }
}

#[test]
fn a_device_poll_classifies_rfc8628_outcomes() {
    let approved = parse_token_response(OK, br#"{"access_token":"dca:one","token_type":"bearer"}"#)
        .expect("approved poll");
    let DevicePoll::Approved(payload) = approved else {
        panic!("a successful token response must be an approval");
    };
    assert_eq!(payload["access_token"], "dca:one");

    let pending = http::StatusCode::BAD_REQUEST;
    for (body, expected) in [
        (r#"{"error":"authorization_pending"}"#, "pending"),
        (r#"{"error":"slow_down"}"#, "slow_down"),
        (r#"{"error":"access_denied"}"#, "denied"),
        (r#"{"error":"expired_token"}"#, "expired"),
    ] {
        let poll = parse_token_response(pending, body.as_bytes()).expect("classified poll");
        let classified = match poll {
            DevicePoll::Pending => "pending",
            DevicePoll::SlowDown => "slow_down",
            DevicePoll::Denied => "denied",
            DevicePoll::Expired => "expired",
            DevicePoll::Approved(_) => "approved",
        };
        assert_eq!(classified, expected, "{body} must classify as {expected}");
    }
    // A rate limit without a recognizable body still asks for a slower cadence,
    // and an unrecognized rejection is a real failure rather than an endless
    // pending poll.
    assert!(matches!(
        parse_token_response(http::StatusCode::TOO_MANY_REQUESTS, b"slow down").expect("throttled"),
        DevicePoll::SlowDown
    ));
    assert!(
        parse_token_response(http::StatusCode::FORBIDDEN, br#"{"error":"invalid_grant"}"#).is_err()
    );
    assert!(parse_token_response(OK, b"not json").is_err());
}

#[test]
fn minting_requires_an_active_subscription_and_a_key() {
    let minted = parse_mint_response(
        OK,
        br#"{"api_key":"inference-one","user_email":"operator@example.com","subs_tier_name":"Pro"}"#,
    )
    .expect("usable mint");
    assert_eq!(minted.api_key, "inference-one");
    // The camelCase spelling is accepted because the reference implementations
    // disagree on which one the service returns.
    assert_eq!(
        parse_mint_response(OK, br#"{"apiKey":"inference-two"}"#)
            .expect("camelCase mint")
            .api_key,
        "inference-two"
    );

    let Err(inactive) = parse_mint_response(OK, br#"{"is_subs_active":false}"#) else {
        panic!("an inactive subscription must not yield a key");
    };
    assert!(
        inactive.contains("subscription is inactive"),
        "unexpected message: {inactive}"
    );
    let Err(rejected) = parse_mint_response(http::StatusCode::UNAUTHORIZED, br#"{}"#) else {
        panic!("a rejected device token must not yield a key");
    };
    assert!(
        rejected.contains("reconnect"),
        "unexpected message: {rejected}"
    );
    assert!(parse_mint_response(OK, br#"{"api_key":"  "}"#).is_err());
    assert!(parse_mint_response(OK, b"not json").is_err());
    let Err(server_error) = parse_mint_response(http::StatusCode::BAD_GATEWAY, b"{}") else {
        panic!("a server error must not yield a key");
    };
    assert!(
        server_error.contains("502"),
        "unexpected message: {server_error}"
    );
}

#[test]
fn request_auth_uses_the_minted_key_and_identifies_as_the_muse_client() {
    let adapter = MuseAdapter;
    let account = OAuthRequestAuth {
        access_token: "inference-one".to_owned(),
        account_id: None,
        project_id: None,
    };
    let request = built_request(&adapter, "bearer", None, Some(&account));
    assert_eq!(
        request.headers().get(http::header::AUTHORIZATION).unwrap(),
        "Bearer inference-one"
    );
    assert_eq!(
        request
            .headers()
            .get(device::API_VERSION_HEADER)
            .expect("api version header"),
        device::API_VERSION
    );
    assert_eq!(
        request.headers().get(http::header::USER_AGENT).unwrap(),
        INFERENCE_USER_AGENT
    );

    // A pasted API key authenticates the same way, and the durable device token
    // is never sent as a bearer credential.
    let pasted = built_request(&adapter, "bearer", Some("pasted-key"), None);
    assert_eq!(
        pasted.headers().get(http::header::AUTHORIZATION).unwrap(),
        "Bearer pasted-key"
    );
    assert!(!format!("{:?}", pasted.headers()).contains("dca:"));

    let anonymous = built_request(&adapter, "none", None, None);
    assert!(
        anonymous
            .headers()
            .get(http::header::AUTHORIZATION)
            .is_none()
    );
}

/// A credential the adapter cannot repair must keep the standard invalidation
/// path, so a rejection is never silently swallowed.
#[tokio::test]
async fn only_a_muse_account_is_offered_for_repair() {
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some(TEST_MASTER_KEY);
    let state = AppState::new(config, database.db.clone());
    seed_provider(
        &database.db,
        ProviderSeed {
            id: "muse-provider",
            name: "Meta Muse Code",
            base_url: "https://api.meta.ai/v1",
            adapter_id: MUSE_ADAPTER_ID,
            auth_type: "bearer",
            model_prefix: "muse",
            preferred_protocol: "responses",
            supported_protocols: &["responses", "chat_completions"],
        },
    )
    .await
    .expect("seed provider");
    seed_provider_key(
        &database.db,
        "muse-provider",
        "pasted-key",
        b"a-pasted-muse-api-key",
        false,
    )
    .await
    .expect("seed pasted key");

    // A status that is not a rejection is never repaired, so the adapter does
    // no work on the common failure paths.
    assert!(
        !MUSE_ADAPTER
            .repair_rejected_credential(
                &state,
                "muse-provider",
                "pasted-key",
                StatusCode::TOO_MANY_REQUESTS
            )
            .await
    );
    // A pasted API key has no durable device token to mint from.
    assert!(
        !MUSE_ADAPTER
            .repair_rejected_credential(
                &state,
                "muse-provider",
                "pasted-key",
                StatusCode::UNAUTHORIZED
            )
            .await
    );
    assert!(
        !MUSE_ADAPTER
            .repair_rejected_credential(
                &state,
                "muse-provider",
                "missing-credential",
                StatusCode::UNAUTHORIZED
            )
            .await
    );
}

/// The mint body is the one part of this integration that had to be inferred:
/// Meta publishes no third-party Muse contract, and the reference
/// implementations disagree about whether the body carries `onboard` or the
/// device token. Pinning it here means switching to the other form is a
/// deliberate edit with a failing test, not a silent drift.
#[test]
fn the_mint_body_is_pinned_to_the_onboard_form() {
    assert_eq!(mint_request_body(), json!({ "onboard": true }));
}

/// Drives the real HTTP path against a scripted upstream, so the request the
/// adapter actually sends — method, path, bearer token, version header, and
/// user agent — is asserted instead of assumed. This is the call whose contract
/// could not be verified against the live service.
#[tokio::test]
async fn the_mint_call_sends_the_pinned_request_shape() {
    let (address, server) = spawn_mock_http_server(vec![MockResponse::json(
        200,
        br#"{"api_key":"muse-inference-key","is_subs_active":true}"#.to_vec(),
    )])
    .await;
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some(TEST_MASTER_KEY);
    config.allow_private_provider_urls = true;
    let state = AppState::new(config, database.db.clone());
    let operational = state.operational_settings().settings;

    let minted = mint_api_key(
        &format!("http://{address}/muse-code/key"),
        "dca-token-1",
        true,
        operational.connect_timeout,
        operational.request_timeout,
        operational.upstream,
    )
    .await
    .expect("mint");
    assert_eq!(minted.api_key, "muse-inference-key");

    let requests = server.await.expect("mock upstream");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].request_line, "POST /muse-code/key HTTP/1.1");
    assert_eq!(
        requests[0].authorization.as_deref(),
        Some("Bearer dca-token-1")
    );
    assert_eq!(requests[0].header("x-api-version"), Some(API_VERSION));
    assert_eq!(requests[0].header("user-agent"), Some(MUSE_USER_AGENT));
    assert_eq!(requests[0].header("accept"), Some("application/json"));
    assert_eq!(requests[0].header("content-type"), Some("application/json"));
}

/// Muse Spark spends its budget on hidden reasoning, so a request with too small
/// a budget can come back as HTTP 200 with no content. The floor is applied to
/// Muse Spark only, and a client that asked for more is never lowered.
#[test]
fn the_muse_spark_output_floor_only_raises_a_missing_or_small_budget() {
    let responses = UpstreamProtocol::Responses;
    let chat = UpstreamProtocol::ChatCompletions;

    // Omitted on Responses: the floor is added.
    let mut absent = json!({"model": "muse-spark-1.3"});
    apply_muse_spark_output_floor("muse-spark-1.3", responses, &mut absent);
    assert_eq!(
        absent["max_output_tokens"],
        json!(MUSE_SPARK_MIN_OUTPUT_TOKENS)
    );

    // Below the floor: raised.
    let mut small = json!({"model": "muse-spark-1.3", "max_output_tokens": 16});
    apply_muse_spark_output_floor("muse-spark-1.3", responses, &mut small);
    assert_eq!(
        small["max_output_tokens"],
        json!(MUSE_SPARK_MIN_OUTPUT_TOKENS)
    );

    // Above the floor: untouched, so a long answer is never truncated.
    let mut large = json!({"model": "muse-spark-1.3", "max_output_tokens": 8192});
    apply_muse_spark_output_floor("muse-spark-1.3", responses, &mut large);
    assert_eq!(large["max_output_tokens"], json!(8192));

    // Chat Completions carries the budget under its own field name.
    let mut chat_body = json!({"model": "muse-spark-1.2", "max_tokens": 4});
    apply_muse_spark_output_floor("muse-spark-1.2", chat, &mut chat_body);
    assert_eq!(chat_body["max_tokens"], json!(MUSE_SPARK_MIN_OUTPUT_TOKENS));
    assert!(chat_body.get("max_output_tokens").is_none());

    // A non-Spark model is left exactly as the client sent it.
    let mut llama = json!({"model": "llama-3.1-8b"});
    apply_muse_spark_output_floor("llama-3.1-8b", responses, &mut llama);
    assert!(llama.get("max_output_tokens").is_none());
    assert!(!is_muse_spark_model("llama-3.1-8b"));
    assert!(is_muse_spark_model("muse-spark-1.3-contributor"));

    // A protocol the adapter never sends is not reshaped, and the body's own
    // model name is honoured when the caller only has the translated body.
    let mut messages = json!({"model": "muse-spark-1.3"});
    apply_muse_spark_output_floor("muse-spark-1.3", UpstreamProtocol::Messages, &mut messages);
    assert!(messages.get("max_tokens").is_none());
    let mut body_only = json!({"model": "muse-spark-1.1"});
    apply_muse_spark_output_floor("some-alias", responses, &mut body_only);
    assert_eq!(
        body_only["max_output_tokens"],
        json!(MUSE_SPARK_MIN_OUTPUT_TOKENS)
    );
}

fn built_request(
    adapter: &MuseAdapter,
    auth_type: &str,
    secret: Option<&str>,
    oauth: Option<&OAuthRequestAuth>,
) -> reqwest::Request {
    let client = reqwest::Client::new();
    let url = adapter
        .endpoint("https://api.meta.ai/v1", Protocol::Responses, None)
        .expect("responses endpoint");
    let builder = client.post(url).json(&json!({"model": "muse-spark-1.3"}));
    let builder = adapter.apply_client_headers(builder, &HeaderMap::new());
    adapter
        .apply_request_auth(builder, auth_type, None, secret, oauth, "session")
        .expect("request auth")
        .build()
        .expect("built request")
}
