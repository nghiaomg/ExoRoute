use super::*;

#[tokio::test]
async fn analytics_marks_disconnected_response_as_failed() {
    use axum::body::Body;
    let database = TestDatabase::open().await;
    let state = AppState::new(database.config(), database.db.clone());
    let analytics = state.telemetry.start_request("test-key".to_owned());
    let body = Body::from_stream(async_stream::stream! {
        yield Ok::<Bytes, std::io::Error>(Bytes::from_static(b"partial"));
        std::future::pending::<()>().await;
    });
    let response = track_analytics_response(Response::new(body), analytics);
    let mut chunks = response.into_body().into_data_stream();
    assert!(chunks.next().await.is_some());
    drop(chunks);
    state
        .telemetry
        .flush()
        .await
        .expect("flush disconnected request");
    let stats =
        crate::infra::telemetry::statistics_snapshot(&database.db, "24h", 1440, &state.telemetry)
            .await;
    let stats = stats.expect("statistics reflect the disconnected body");
    assert_eq!(stats["total_requests"], 1);
    assert_eq!(stats["failures"], 1);
}
