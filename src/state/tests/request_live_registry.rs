use super::support::*;
use super::*;

#[tokio::test]
async fn request_live_registry_is_bounded_and_keeps_duplicate_request_ids_separate() {
    let registry = Arc::new(RequestLiveRegistry::new());
    let mut updates = registry.subscribe();
    let mut guards = Vec::with_capacity(super::request_live::MAX_REQUEST_LIVE_ENTRIES);
    for _ in 0..super::request_live::MAX_REQUEST_LIVE_ENTRIES {
        guards.push(registry.start("same-request", "route", "model", "messages", None));
    }
    let first_id = match updates.recv().await.expect("first live event") {
        RequestLiveEvent::Started { row, .. } => row.live_id,
        event => panic!("unexpected event: {event:?}"),
    };
    for _ in 1..super::request_live::MAX_REQUEST_LIVE_ENTRIES {
        let _ = updates.recv().await.expect("live start event");
    }
    let second = registry.start("same-request", "route", "model", "messages", None);
    let snapshot = registry.snapshot();
    assert_eq!(
        snapshot.requests.len(),
        super::request_live::MAX_REQUEST_LIVE_ENTRIES
    );
    assert!(snapshot.truncated);
    assert!(
        snapshot
            .requests
            .iter()
            .any(|request| request.live_id == first_id)
    );
    assert_eq!(
        snapshot
            .requests
            .iter()
            .filter(|request| request.request_id == "same-request")
            .count(),
        super::request_live::MAX_REQUEST_LIVE_ENTRIES
    );
    assert_eq!(
        snapshot.active_count,
        super::request_live::MAX_REQUEST_LIVE_ENTRIES + 1
    );
    assert!(matches!(
        updates.recv().await.expect("capacity event"),
        RequestLiveEvent::Capacity {
            truncated: true,
            ..
        }
    ));

    drop(guards.pop());
    assert!(matches!(
        updates.recv().await.expect("finish event"),
        RequestLiveEvent::Finished { .. }
    ));
    assert!(registry.snapshot().truncated);
    assert_eq!(
        registry.snapshot().active_count,
        super::request_live::MAX_REQUEST_LIVE_ENTRIES
    );
    drop(second);
    assert!(matches!(
        updates.recv().await.expect("capacity recovery event"),
        RequestLiveEvent::Capacity {
            truncated: false,
            ..
        }
    ));
    assert_eq!(
        registry.snapshot().active_count,
        super::request_live::MAX_REQUEST_LIVE_ENTRIES - 1
    );
    drop(guards);
}

#[tokio::test]
async fn request_live_guard_updates_provider_and_finishes_once() {
    let registry = Arc::new(RequestLiveRegistry::new());
    let mut updates = registry.subscribe();
    let guard = registry.start("request", "route", "model", "messages", Some("key"));
    let live_id = match updates.recv().await.expect("start event") {
        RequestLiveEvent::Started { row, .. } => row.live_id,
        event => panic!("unexpected event: {event:?}"),
    };
    guard.update_provider(Some("provider-a"));
    assert!(
        matches!(updates.recv().await.expect("provider event"), RequestLiveEvent::Updated { live_id: id, provider_id: Some(provider), .. } if id == live_id && provider == "provider-a")
    );
    let record = live_test_record("request", Some("provider-a"));
    let record_id = record.id.clone();
    guard.update_log(&record);
    assert!(matches!(
        updates.recv().await.expect("latest log event"),
        RequestLiveEvent::Updated {
            latest_log_id: Some(id),
            input_tokens: Some(12),
            output_tokens: Some(7),
            ..
        } if id == record_id
    ));
    drop(guard);
    assert!(
        matches!(updates.recv().await.expect("finish event"), RequestLiveEvent::Finished { live_id: id, request: Some(record), .. } if id == live_id && record.status == 200)
    );
    assert!(matches!(
        updates.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}
