use super::*;

pub(super) use crate::support::mock_upstream::{
    MockResponse, spawn_mock_http_server, spawn_silent_upstream,
};

pub(super) async fn test_state() -> (TestDatabase, AppState) {
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some([47_u8; 32]);
    config.allow_private_provider_urls = true;
    let state = AppState::new(config, database.db.clone());
    (database, state)
}

pub(super) fn codex_account_for_match(
    account_id: Option<&str>,
    user_id: Option<&str>,
    email: Option<&str>,
) -> CodexAccount {
    CodexAccount {
        access_token: "access-token".to_owned(),
        refresh_token: None,
        id_token: None,
        expires_at: 0,
        account_id: account_id.map(str::to_owned),
        chatgpt_user_id: user_id.map(str::to_owned),
        email: email.map(str::to_owned),
        plan: None,
    }
}
