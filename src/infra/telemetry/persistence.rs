use super::maintenance::{clear_history_in_database, persist_drop_delta};
use super::*;

pub(super) async fn telemetry_writer(
    db: Database,
    mut receiver: mpsc::Receiver<TelemetryMessage>,
    mut dropped_receiver: watch::Receiver<u64>,
    dropped_notify: watch::Sender<u64>,
    counters: Arc<DropCounters>,
    drop_state_lock: Arc<AsyncMutex<()>>,
) {
    let mut batch = Vec::with_capacity(TELEMETRY_BATCH_SIZE);
    let mut flush = tokio::time::interval(TELEMETRY_FLUSH_INTERVAL);
    flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    flush.tick().await;
    let mut maintenance = tokio::time::interval(STATISTICS_MAINTENANCE_INTERVAL);
    maintenance.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    maintenance.tick().await;
    let mut persisted_local_drops = 0_u64;
    let mut receiver_closed = false;

    loop {
        tokio::select! {
            message = receiver.recv(), if !receiver_closed && batch.len() < TELEMETRY_BATCH_SIZE => match message {
                Some(TelemetryMessage::ClearHistory(reply)) => {
                    if !batch.is_empty() {
                        persist_batch_with_retry(&db, &batch, &counters).await;
                        batch.clear();
                    }
                    let _drop_state_guard = drop_state_lock.lock().await;
                    let result = clear_history_in_database(&db).await;
                    match result {
                        Ok(deleted) => {
                            counters.request_logs.store(0, Ordering::Relaxed);
                            counters.api_key_usage.store(0, Ordering::Relaxed);
                            counters.statistics.store(0, Ordering::Relaxed);
                            counters.provider_usage.store(0, Ordering::Relaxed);
                            counters.total.store(0, Ordering::Relaxed);
                            counters.persisted_this_process.store(0, Ordering::Relaxed);
                            counters
                                .persisted_provider_usage_this_process
                                .store(0, Ordering::Relaxed);
                            persisted_local_drops = 0;
                            dropped_notify.send_replace(0);
                            let _ = reply.send(Ok(deleted));
                        }
                        Err(error) => {
                            let _ = reply.send(Err(error));
                        }
                    }
                }
                Some(TelemetryMessage::FlushHistory(reply)) => {
                    let batch_saved = if batch.is_empty() {
                        true
                    } else {
                        let saved = persist_batch_with_retry(&db, &batch, &counters).await;
                        batch.clear();
                        saved
                    };
                    let drops_saved = persist_drop_delta(
                        &db,
                        &counters,
                        &drop_state_lock,
                        &mut persisted_local_drops,
                    )
                    .await;
                    let result = if batch_saved && drops_saved {
                        Ok(())
                    } else {
                        Err("Some queued request history could not be saved.".to_owned())
                    };
                    let _ = reply.send(result);
                }
                Some(message) => batch.push(message),
                None => receiver_closed = true,
            },
            _ = flush.tick() => {
                if !batch.is_empty() {
                    persist_batch_with_retry(&db, &batch, &counters).await;
                    batch.clear();
                }
                persist_drop_delta(&db, &counters, &drop_state_lock, &mut persisted_local_drops).await;
            }
            result = dropped_receiver.changed() => {
                if result.is_ok() {
                    persist_drop_delta(&db, &counters, &drop_state_lock, &mut persisted_local_drops).await;
                }
            }
            _ = maintenance.tick() => {
                if !batch.is_empty() {
                    persist_batch_with_retry(&db, &batch, &counters).await;
                    batch.clear();
                }
                persist_drop_delta(&db, &counters, &drop_state_lock, &mut persisted_local_drops).await;
                if let Err(error) = expire_statistics(&db, unix_minute()).await {
                    tracing::warn!(%error, "could not expire old request statistics buckets");
                }
                if let Err(error) = expire_provider_usage(&db, unix_minute()).await {
                    tracing::warn!(%error, "could not expire old provider usage meter buckets");
                }
            }
        }

        if batch.len() >= TELEMETRY_BATCH_SIZE {
            persist_batch_with_retry(&db, &batch, &counters).await;
            batch.clear();
        }
        if receiver_closed {
            if !batch.is_empty() {
                persist_batch_with_retry(&db, &batch, &counters).await;
                batch.clear();
            }
            persist_drop_delta(&db, &counters, &drop_state_lock, &mut persisted_local_drops).await;
            break;
        }
    }
}

async fn persist_batch_with_retry(
    db: &Database,
    batch: &[TelemetryMessage],
    counters: &Arc<DropCounters>,
) -> bool {
    for attempt in 0..3 {
        match super::persist_batch(db, batch).await {
            Ok(()) => return true,
            Err(error) if attempt < 2 => {
                tracing::warn!(%error, attempt = attempt + 1, records = batch.len(), "could not persist telemetry batch; retrying");
                tokio::time::sleep(Duration::from_millis(50 * (attempt + 1) as u64)).await;
            }
            Err(error) => {
                tracing::error!(%error, records = batch.len(), "telemetry batch was dropped after database write failures");
                for message in batch {
                    let kind = match message {
                        TelemetryMessage::RequestLog(_) => DropKind::RequestLog,
                        TelemetryMessage::ApiKeyUsage(_) => DropKind::ApiKeyUsage,
                        TelemetryMessage::ProviderUsage(_) => DropKind::ProviderUsage,
                        TelemetryMessage::Statistics(_) => DropKind::Statistics,
                        TelemetryMessage::FlushHistory(_) | TelemetryMessage::ClearHistory(_) => {
                            continue;
                        }
                    };
                    increment_for_writer(counters, kind);
                }
                return false;
            }
        }
    }
    false
}

fn increment_for_writer(counters: &DropCounters, kind: DropKind) {
    match kind {
        DropKind::RequestLog => increment_saturating(&counters.request_logs),
        DropKind::ApiKeyUsage => increment_saturating(&counters.api_key_usage),
        DropKind::ProviderUsage => increment_saturating(&counters.provider_usage),
        DropKind::Statistics => increment_saturating(&counters.statistics),
    };
    increment_saturating(&counters.total);
}
