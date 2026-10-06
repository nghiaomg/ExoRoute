use super::support::*;
use super::*;

#[tokio::test]
async fn expired_admin_access_token_is_removed_from_memory_cache() {
    let state = new_test_state().await;
    let token = state
        .issue_admin_access_token("test-family".to_owned(), false)
        .await
        .expect("access-token entropy");
    assert!(state.admin_session(&token).await.is_some());

    let digest = admin_token_digest(&token);
    if let Some(session) = state.admin.admin_sessions.write().await.get_mut(&digest) {
        session.expires_at = Instant::now() - Duration::from_secs(1);
    }
    assert!(state.admin_session(&token).await.is_none());
    assert!(state.admin.admin_sessions.read().await.is_empty());
}
