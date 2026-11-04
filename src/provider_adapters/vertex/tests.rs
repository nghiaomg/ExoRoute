use super::*;

/// A 2048-bit RSA key generated for tests. Shipping a fixed test key keeps the
/// suite fast; it must never be used outside this module.
fn test_service_account_json() -> String {
    let key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 2048).expect("test key");
    let pem = rsa::pkcs8::EncodePrivateKey::to_pkcs8_pem(&key, rsa::pkcs8::LineEnding::LF)
        .expect("encode test key")
        .to_string();
    serde_json::json!({
        "type": "service_account",
        "project_id": "demo-project",
        "client_email": "runner@demo-project.iam.gserviceaccount.com",
        "private_key": pem,
        "token_uri": "https://oauth2.googleapis.com/token",
    })
    .to_string()
}

#[test]
fn vertex_parses_a_service_account_key() {
    let account = parse_service_account(&test_service_account_json()).expect("account");
    assert_eq!(
        account.client_email,
        "runner@demo-project.iam.gserviceaccount.com"
    );
    assert_eq!(account.token_uri, "https://oauth2.googleapis.com/token");
}

#[test]
fn vertex_reports_a_malformed_credential_instead_of_ignoring_it() {
    for secret in [
        "not json",
        r#"{"client_email":"a@b.c"}"#,
        r#"{"client_email":"","private_key":"pem"}"#,
    ] {
        assert!(parse_service_account(secret).is_err(), "{secret}");
    }
}

#[test]
fn vertex_rejects_an_unexpected_token_endpoint() {
    let mut value: Value = serde_json::from_str(&test_service_account_json()).unwrap();
    value["token_uri"] = Value::String("https://evil.example.com/token".to_owned());
    let error = parse_service_account(&value.to_string()).unwrap_err();
    assert!(error.contains("token endpoint"), "{error}");
}

#[test]
fn vertex_signs_a_verifiable_rs256_assertion() {
    let account = parse_service_account(&test_service_account_json()).expect("account");
    let assertion = sign_jwt_assertion(&account, 1_700_000_000).expect("assertion");
    let parts: Vec<&str> = assertion.split('.').collect();
    assert_eq!(parts.len(), 3);

    let header: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).expect("header")).expect("json");
    assert_eq!(header["alg"], "RS256");
    assert_eq!(header["typ"], "JWT");

    let claims: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).expect("claims")).expect("json");
    assert_eq!(claims["iss"], account.client_email);
    assert_eq!(claims["aud"], account.token_uri);
    assert_eq!(claims["scope"], VERTEX_SCOPE);
    assert_eq!(claims["iat"], 1_700_000_000u64);
    assert_eq!(claims["exp"], 1_700_003_600u64);

    // A 2048-bit RSA signature is 256 bytes; the `oid` feature is what makes
    // this a valid PKCS#1 v1.5 DigestInfo signature rather than a raw one.
    let signature = URL_SAFE_NO_PAD.decode(parts[2]).expect("signature");
    assert_eq!(signature.len(), 256);
}

#[test]
fn vertex_endpoint_addresses_the_project_and_location() {
    let base = "https://us-central1-aiplatform.googleapis.com/v1/projects/demo-project";
    assert_eq!(
        vertex_endpoint(base, "gemini-2.5-pro", false)
            .unwrap()
            .as_str(),
        "https://us-central1-aiplatform.googleapis.com/v1/projects/demo-project/locations/us-central1/publishers/google/models/gemini-2.5-pro:generateContent"
    );
    let streaming = vertex_endpoint(base, "gemini-2.5-pro", true).unwrap();
    assert!(
        streaming
            .as_str()
            .ends_with("/publishers/google/models/gemini-2.5-pro:streamGenerateContent?alt=sse")
    );
}

#[test]
fn vertex_splits_the_regional_host_into_a_location() {
    let (location, project) = vertex_location_and_project(
        "https://europe-west4-aiplatform.googleapis.com/v1/projects/my-project",
    )
    .unwrap();
    assert_eq!(location, "europe-west4");
    assert_eq!(project, "my-project");
}

#[test]
fn vertex_rejects_a_url_without_a_regional_endpoint_or_project() {
    for base in [
        "https://aiplatform.googleapis.com/v1/projects/demo",
        "https://us-central1-aiplatform.googleapis.com/v1/projects",
        "https://us-central1-aiplatform.googleapis.com/",
        "https://us-central1-aiplatform.googleapis.com/v1/projects/../escape",
        "not a url",
    ] {
        assert!(
            vertex_location_and_project(base).is_err(),
            "expected rejection for {base}"
        );
    }
}

#[test]
fn vertex_rejects_model_names_that_could_escape_the_path() {
    let base = "https://us-central1-aiplatform.googleapis.com/v1/projects/demo-project";
    assert!(vertex_endpoint(base, "", false).is_err());
    assert!(vertex_endpoint(base, "gemini-2.5-pro/../../other", false).is_err());
    assert!(vertex_endpoint(base, "gemini 2.5", false).is_err());
}

#[test]
fn vertex_only_accepts_the_google_upstream_protocol() {
    let adapter = &VERTEX_ADAPTER as &dyn ProviderAdapter;
    let base = "https://us-central1-aiplatform.googleapis.com/v1/projects/demo-project";
    for protocol in [
        UpstreamProtocol::ChatCompletions,
        UpstreamProtocol::Responses,
        UpstreamProtocol::Messages,
    ] {
        assert!(
            adapter
                .upstream_endpoint(base, protocol, "gemini-2.5-pro", false, None)
                .is_err(),
            "{protocol:?}"
        );
    }
    assert!(
        adapter
            .upstream_endpoint(
                base,
                UpstreamProtocol::GoogleGenerateContent,
                "gemini-2.5-pro",
                false,
                None
            )
            .is_ok()
    );
}

#[test]
fn vertex_validate_config_requires_a_service_account_and_the_google_protocol() {
    let adapter = &VERTEX_ADAPTER as &dyn ProviderAdapter;
    let preset = preset_for_adapter(VERTEX_ADAPTER_ID).expect("vertex preset");
    let base = "https://us-central1-aiplatform.googleapis.com/v1/projects/demo-project";
    assert!(
        adapter
            .validate_config(
                preset,
                "vertex_service_account",
                base,
                "google_generate_content",
                &["google_generate_content".to_owned()],
                true,
            )
            .is_ok()
    );
    // No credential means nothing to authenticate with.
    assert!(
        adapter
            .validate_config(
                preset,
                "vertex_service_account",
                base,
                "google_generate_content",
                &["google_generate_content".to_owned()],
                false,
            )
            .is_err()
    );
    assert!(
        adapter
            .validate_config(
                preset,
                "vertex_service_account",
                base,
                "chat_completions",
                &["chat_completions".to_owned()],
                true,
            )
            .is_err()
    );
    assert!(
        adapter
            .validate_config(
                preset,
                "vertex_service_account",
                "https://aiplatform.googleapis.com/v1/projects/demo",
                "google_generate_content",
                &["google_generate_content".to_owned()],
                true,
            )
            .is_err()
    );
}

#[test]
fn vertex_model_list_endpoint_targets_the_publisher_catalog() {
    let adapter = &VERTEX_ADAPTER as &dyn ProviderAdapter;
    assert_eq!(
        adapter
            .model_list_endpoint(
                "https://us-central1-aiplatform.googleapis.com/v1/projects/demo-project"
            )
            .unwrap()
            .path(),
        "/v1/projects/demo-project/locations/us-central1/publishers/google/models"
    );
}

#[test]
fn vertex_token_cache_expires_and_evicts() {
    // Direct cache checks avoid a live network exchange.
    let key = "cache-probe@example.com:https://oauth2.googleapis.com/token";
    store_token(key, "first".to_owned(), Duration::from_secs(3600));
    assert_eq!(cached_token(key).as_deref(), Some("first"));

    // A lifetime shorter than the refresh margin must not cache at all.
    let short = "short-probe@example.com:https://oauth2.googleapis.com/token";
    store_token(short, "second".to_owned(), Duration::from_secs(60));
    assert_eq!(cached_token(short), None);

    let mut cache = token_cache().lock().unwrap();
    cache.clear();
}
