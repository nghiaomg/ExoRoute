use super::*;

pub(super) struct TestState {
    state: AppState,
    _database: TestDatabase,
}

impl Deref for TestState {
    type Target = AppState;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

pub(super) async fn new_test_state() -> TestState {
    let database = TestDatabase::open().await;
    let state = AppState::new(database.config(), database.db.clone());
    TestState {
        state,
        _database: database,
    }
}

pub(super) fn live_test_record(request_id: &str, provider_id: Option<&str>) -> RequestLogRecord {
    RequestLogRecord {
        id: uuid::Uuid::new_v4().to_string(),
        request_id: request_id.to_owned(),
        route_alias: "route".to_owned(),
        provider_id: provider_id.map(str::to_owned),
        provider_credential_id: None,
        api_key_id: Some("key".to_owned()),
        model: "model".to_owned(),
        client_protocol: "chat_completions".to_owned(),
        upstream_protocol: Some("chat_completions".to_owned()),
        status: 200,
        duration_ms: 12,
        input_tokens: Some(12),
        output_tokens: Some(7),
        cached_tokens: None,
        cache_input_tokens: None,
        cost_micro_usd: None,
        error: None,
    }
}
