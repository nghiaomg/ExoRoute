//! Tests for the telemetry aggregation pipeline.

use super::*;

use crate::infra::storage::Field;
use crate::infra::telemetry::store::provider_usage_key;
use std::fs;

async fn test_db() -> (Database, std::path::PathBuf) {
    let path =
        std::env::temp_dir().join(format!("exoroute-telemetry-lmdb-{}", uuid::Uuid::new_v4()));
    let database = db::connect(&path)
        .await
        .expect("open LMDB test environment");
    (database, path)
}

fn api_key_record(id: &str) -> Record {
    Record::new()
        .with("id", Field::Text(id.to_owned()))
        .with("name", Field::Text(id.to_owned()))
        .with("request_count", Field::I64(0))
        .with("request_count_generation", Field::I64(0))
}

#[tokio::test]
async fn shutdown_flush_still_applies_queued_events() {
    let (database, path) = test_db().await;
    database
        .write(|transaction| {
            transaction.put(
                Table::ApiKeys,
                "key-shutdown",
                &api_key_record("Key Shutdown"),
            )
        })
        .await
        .expect("seed API key");
    let telemetry = Telemetry::new(database.clone());
    telemetry.record_api_key_request("key-shutdown".to_owned());
    telemetry.shutdown().await.expect("bounded shutdown drain");

    let api_key = database
        .read(|transaction| transaction.get::<Record>(Table::ApiKeys, "key-shutdown"))
        .await
        .expect("read API key")
        .expect("stored API key");
    assert_eq!(api_key.integer("request_count").unwrap(), 1);
    drop(telemetry);
    drop(database);
    let _ = fs::remove_dir_all(&path);
}

#[tokio::test]
async fn telemetry_persists_log_usage_and_statistics_in_one_backend() {
    let (database, path) = test_db().await;
    database
        .write(|transaction| transaction.put(Table::ApiKeys, "key-1", &api_key_record("Key One")))
        .await
        .expect("seed API key");
    let telemetry = Telemetry::new(database.clone());
    let analytics = telemetry.start_request("key-1".to_owned());
    analytics.set_requested_model(Some("provider/model"));
    analytics.set_token_usage(Some(10), Some(4));
    analytics.set_cached_token_usage(Some(6), Some(10));
    analytics.finish(true);
    telemetry.record_api_key_request("key-1".to_owned());
    telemetry.enqueue_request_log(RequestLogRecord {
        id: "log-1".to_owned(),
        request_id: "request-1".to_owned(),
        route_alias: "default".to_owned(),
        provider_id: Some("provider".to_owned()),
        provider_credential_id: Some("credential-1".to_owned()),
        api_key_id: Some("key-1".to_owned()),
        model: "provider/model".to_owned(),
        client_protocol: "chat_completions".to_owned(),
        upstream_protocol: Some("chat_completions".to_owned()),
        status: 200,
        duration_ms: 12,
        input_tokens: Some(10),
        output_tokens: Some(4),
        cached_tokens: Some(6),
        cache_input_tokens: Some(10),
        cost_micro_usd: Some(125_000),
        error: None,
    });
    telemetry.flush().await.expect("flush telemetry");

    let log = database
        .read(|transaction| transaction.get::<Record>(Table::RequestLogs, "log-1"))
        .await
        .expect("read request log")
        .expect("stored request log");
    assert_eq!(log.text("id").unwrap(), "log-1");
    assert_eq!(log.text("request_id").unwrap(), "request-1");
    let api_key = database
        .read(|transaction| transaction.get::<Record>(Table::ApiKeys, "key-1"))
        .await
        .expect("read API key")
        .expect("stored API key");
    assert_eq!(api_key.integer("request_count").unwrap(), 1);
    let snapshot = statistics_snapshot(&database, "1d", 1440, &telemetry)
        .await
        .expect("statistics snapshot");
    assert_eq!(snapshot["total_requests"], 1);
    assert_eq!(snapshot["input_tokens"], 10);
    assert_eq!(snapshot["cached_tokens"], 6);
    assert_eq!(snapshot["cache_ratio"], 60.0);
    assert_eq!(snapshot["api_keys"]["top"][0]["label"], "Key One");
    assert_eq!(snapshot["model_breakdown"][0]["model"], "provider/model");
    assert_eq!(snapshot["model_breakdown"][0]["combo"], "default");
    assert_eq!(snapshot["model_breakdown"][0]["input_tokens"], 10);
    assert_eq!(snapshot["model_breakdown"][0]["output_tokens"], 4);
    assert_eq!(snapshot["model_breakdown"][0]["cache_hit_rate"], 60.0);
    assert_eq!(snapshot["model_breakdown"][0]["successes"], 1);
    assert_eq!(snapshot["model_breakdown"][0]["success_rate"], 100.0);
    let key_snapshot = api_key_statistics_snapshot(&database, "key-1", "1d", 1440, &telemetry)
        .await
        .expect("API key statistics snapshot");
    assert_eq!(key_snapshot["api_key_id"], "key-1");
    assert_eq!(key_snapshot["api_key_name"], "Key One");
    assert_eq!(key_snapshot["total_requests"], 1);
    assert_eq!(key_snapshot["input_tokens"], 10);
    assert_eq!(key_snapshot["cached_tokens"], 6);
    assert_eq!(key_snapshot["models"]["top"][0]["id"], "provider/model");
    assert_eq!(key_snapshot["successes"], 1);
    assert_eq!(key_snapshot["success_rate"], 100.0);
    assert_eq!(key_snapshot["output_tokens"], 4);
    assert_eq!(key_snapshot["model_breakdown"][0]["combo"], "default");
    assert_eq!(key_snapshot["model_breakdown"][0]["requests"], 1);
    database
        .write(|transaction| transaction.put(Table::ApiKeys, "key-2", &api_key_record("Key Two")))
        .await
        .expect("seed second API key");
    let empty_snapshot = api_key_statistics_snapshot(&database, "key-2", "1d", 1440, &telemetry)
        .await
        .expect("unused API key statistics snapshot");
    assert_eq!(empty_snapshot["total_requests"], 0);
    assert!(
        empty_snapshot["models"]["top"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    assert_eq!(empty_snapshot["models"]["others"], 0);
    drop(telemetry);
    drop(database);
    let _ = fs::remove_dir_all(&path);
}

#[tokio::test]
async fn request_logs_survive_telemetry_and_database_restart() {
    let (database, path) = test_db().await;
    let telemetry = Telemetry::new(database.clone());
    telemetry.enqueue_request_log(RequestLogRecord {
        id: "log-restart".to_owned(),
        request_id: "request-restart".to_owned(),
        route_alias: "default".to_owned(),
        provider_id: Some("provider".to_owned()),
        provider_credential_id: None,
        api_key_id: None,
        model: "provider/model".to_owned(),
        client_protocol: "chat_completions".to_owned(),
        upstream_protocol: Some("chat_completions".to_owned()),
        status: 200,
        duration_ms: 12,
        input_tokens: None,
        output_tokens: None,
        cached_tokens: None,
        cache_input_tokens: None,
        cost_micro_usd: None,
        error: None,
    });
    telemetry
        .shutdown()
        .await
        .expect("shutdown flushes request log");
    drop(telemetry);
    drop(database);

    let reopened = db::connect(&path).await.expect("reopen LMDB environment");
    let log = reopened
        .read(|transaction| {
            let record = transaction.get::<Record>(Table::RequestLogs, "log-restart")?;
            let indexes = transaction.scan_prefix::<String>(
                Table::RequestLogIndex,
                db::REQUEST_LOG_INDEX_PREFIX,
                10,
            )?;
            Ok((record, indexes))
        })
        .await
        .expect("read request log after restart");
    assert_eq!(
        log.0.as_ref().and_then(|record| record.field("id")),
        Some(&Field::Text("log-restart".to_owned()))
    );
    assert!(log.1.iter().any(|(_, id)| id == "log-restart"));

    drop(reopened);
    let _ = fs::remove_dir_all(&path);
}

#[tokio::test]
async fn provider_usage_meter_persists_cost_and_marks_missing_cost_as_partial() {
    let (database, path) = test_db().await;
    database
        .write(|transaction| {
            transaction.put(Table::ProviderApiKeys, "credential-cost", &Record::new())?;
            transaction.put(Table::ProviderApiKeys, "credential-partial", &Record::new())
        })
        .await
        .expect("seed provider credentials");
    let telemetry = Telemetry::new(database.clone());
    telemetry.record_provider_usage(
        "credential-cost".to_owned(),
        Some(125_000),
        Some(10),
        Some(4),
    );
    telemetry.record_provider_usage("credential-partial".to_owned(), None, Some(7), Some(3));
    telemetry.flush().await.expect("flush provider usage");

    let cost_snapshot = provider_usage_meter_snapshot(&database, "credential-cost")
        .await
        .expect("cost meter snapshot");
    assert_eq!(cost_snapshot["status"], "fresh");
    assert_eq!(cost_snapshot["windows"]["5h"]["cost_micro_usd"], 125_000);
    assert_eq!(cost_snapshot["windows"]["5h"]["input_tokens"], 10);
    assert_eq!(cost_snapshot["windows"]["5h"]["output_tokens"], 4);

    let partial_snapshot = provider_usage_meter_snapshot(&database, "credential-partial")
        .await
        .expect("partial meter snapshot");
    assert_eq!(partial_snapshot["status"], "partial");
    assert!(partial_snapshot["windows"]["5h"]["cost_micro_usd"].is_null());
    assert_eq!(partial_snapshot["windows"]["5h"]["missing_cost_events"], 1);

    drop(telemetry);
    drop(database);
    let _ = fs::remove_dir_all(&path);
}

#[tokio::test]
async fn provider_usage_meter_excludes_rows_outside_the_rolling_window() {
    let (database, path) = test_db().await;
    let current = unix_minute();
    persist_batch(
        &database,
        &[TelemetryMessage::ProviderUsage(ProviderUsageEvent {
            credential_id: "credential-old".to_owned(),
            minute: current.saturating_sub(5 * 60),
            cost_micro_usd: Some(90_000),
            input_tokens: Some(1),
            output_tokens: Some(1),
        })],
    )
    .await
    .expect("persist old provider usage row");
    let snapshot = provider_usage_meter_snapshot(&database, "credential-old")
        .await
        .expect("old meter snapshot");
    assert_eq!(snapshot["windows"]["5h"]["requests"], 0);
    assert!(snapshot["windows"]["5h"]["cost_micro_usd"].is_null());

    drop(database);
    let _ = fs::remove_dir_all(&path);
}

#[tokio::test]
async fn provider_usage_meter_does_not_reappear_after_credential_deletion() {
    let (database, path) = test_db().await;
    let credential_id = "credential-deleted".to_owned();
    database
        .write({
            let credential_id = credential_id.clone();
            move |transaction| {
                transaction.put(
                    Table::ProviderApiKeys,
                    &credential_id,
                    &Record::new().with("id", Field::Text(credential_id.clone())),
                )
            }
        })
        .await
        .expect("seed provider credential");

    let minute = unix_minute();
    let event = TelemetryMessage::ProviderUsage(ProviderUsageEvent {
        credential_id: credential_id.clone(),
        minute,
        cost_micro_usd: Some(10_000),
        input_tokens: Some(1),
        output_tokens: Some(1),
    });
    persist_batch(&database, &[event])
        .await
        .expect("persist live credential meter");
    let meter_key = provider_usage_key(&credential_id, minute).expect("meter key");
    assert!(
        database
            .read({
                let meter_key = meter_key.clone();
                move |transaction| transaction.get::<Record>(Table::ProviderUsageMeters, &meter_key)
            })
            .await
            .expect("read live meter")
            .is_some()
    );

    database
        .write({
            let credential_id = credential_id.clone();
            let meter_key = meter_key.clone();
            move |transaction| {
                transaction.delete(Table::ProviderUsageMeters, &meter_key)?;
                transaction.delete(Table::ProviderApiKeys, &credential_id)
            }
        })
        .await
        .expect("delete provider credential and meter");

    persist_batch(
        &database,
        &[TelemetryMessage::ProviderUsage(ProviderUsageEvent {
            credential_id: credential_id.clone(),
            minute,
            cost_micro_usd: Some(20_000),
            input_tokens: Some(2),
            output_tokens: Some(2),
        })],
    )
    .await
    .expect("persist late meter event");
    assert!(
        database
            .read(move |transaction| {
                transaction.get::<Record>(Table::ProviderUsageMeters, &meter_key)
            })
            .await
            .expect("read deleted meter")
            .is_none()
    );

    drop(database);
    let _ = fs::remove_dir_all(&path);
}

#[test]
fn provider_meter_credential_ids_cannot_escape_the_bounded_key_namespace() {
    assert!(valid_provider_credential_id("credential-123"));
    assert!(!valid_provider_credential_id(""));
    assert!(!valid_provider_credential_id("credential/other"));
    assert!(!valid_provider_credential_id("credential\nother"));
    assert!(!valid_provider_credential_id(&"x".repeat(257)));
}

#[test]
fn legacy_statistics_records_default_cache_metrics_without_changing_input_totals() {
    let record = Record::new()
        .with("request_count", Field::I64(1))
        .with("success_count", Field::I64(1))
        .with("failure_count", Field::I64(0))
        .with("duration_ms_total", Field::I64(12))
        .with("input_tokens_total", Field::I64(10))
        .with("output_tokens_total", Field::I64(4));
    let counts = record_counts(&record).expect("legacy statistics record");
    assert_eq!(counts.input_tokens, 10);
    assert_eq!(counts.cached_tokens, 0);
    assert_eq!(counts.cache_input_tokens, 10);
}

#[tokio::test]
async fn clearing_history_uses_generation_to_reset_unbounded_api_key_population() {
    let (database, path) = test_db().await;
    database
        .write(|transaction| transaction.put(Table::ApiKeys, "key-1", &api_key_record("key-1")))
        .await
        .expect("seed API key");
    let telemetry = Telemetry::new(database.clone());
    let analytics = telemetry.start_request("key-1".to_owned());
    analytics.set_requested_model(Some("model"));
    analytics.finish(true);
    telemetry.flush().await.expect("save event");
    telemetry
        .clear_request_history()
        .await
        .expect("clear history");
    let snapshot = statistics_snapshot(&database, "1d", 1440, &telemetry)
        .await
        .expect("empty snapshot");
    assert_eq!(snapshot["total_requests"], 0);
    let generation = database
        .read(|transaction| transaction.get::<i64>(Table::Meta, "statistics_generation"))
        .await
        .expect("read generation");
    assert_eq!(generation, Some(1));
    drop(telemetry);
    drop(database);
    let _ = fs::remove_dir_all(&path);
}

#[tokio::test]
async fn statistics_expiry_subtracts_only_the_expired_bucket() {
    let (database, path) = test_db().await;
    let current = unix_minute();
    let old = current.saturating_sub(30 * 24 * 60 + 2_000);
    let recent = current.saturating_sub(10);
    let old_event = StatisticsEvent {
        api_key_id: "key".to_owned(),
        requested_model: "model".to_owned(),
        minute: old,
        succeeded: true,
        duration_ms: 8,
        input_tokens: 3,
        output_tokens: 2,
        cached_tokens: 0,
        cache_input_tokens: 3,
    };
    let recent_event = StatisticsEvent {
        minute: recent,
        ..old_event.clone()
    };
    let mut aggregates = HashMap::new();
    for event in [&old_event, &recent_event] {
        add_aggregate(&mut aggregates, event.minute, 0, "", event);
        add_aggregate(&mut aggregates, event.minute, 1, &event.api_key_id, event);
        if let Some(model_dimension) =
            api_key_model_dimension_id(&event.api_key_id, &event.requested_model)
        {
            add_aggregate(&mut aggregates, event.minute, 3, &model_dimension, event);
        }
        add_aggregate(
            &mut aggregates,
            event.minute,
            2,
            &event.requested_model,
            event,
        );
    }
    database
        .write(move |transaction| {
            for (window, _) in STATISTICS_RANGES {
                transaction.put(
                    Table::UsageExpiryCursor,
                    &window.to_string(),
                    &cursor_record(old.saturating_sub(1), 0, ""),
                )?;
            }
            for (key, counts) in &aggregates {
                let minute = minute_key(key.minute, key.dimension_kind, &key.dimension_id)?;
                transaction.put(Table::UsageMinutes, &minute, &counts_record(*counts))?;
                for (window, _) in STATISTICS_RANGES {
                    let aggregate = window_key(window, key.dimension_kind, &key.dimension_id)?;
                    update_window_counts(
                        transaction,
                        &aggregate,
                        window,
                        key.dimension_kind,
                        &key.dimension_id,
                        *counts,
                    )?;
                }
            }
            Ok(())
        })
        .await
        .expect("seed statistic buckets");
    expire_statistics(&database, current)
        .await
        .expect("expire old buckets");
    let month_summary = window_key(30 * 24 * 60, 0, "").expect("month summary key");
    let counts = database
        .read(move |transaction| {
            transaction
                .get::<Record>(Table::UsageWindows, &month_summary)
                .map(|record| record.map(|record| record_counts(&record)).transpose())
        })
        .await
        .expect("read month summary")
        .expect("summary exists")
        .expect("valid counts");
    assert_eq!(counts.requests, 1);
    let month_key_model =
        window_key(30 * 24 * 60, 3, "key/model").expect("month API key model key");
    let model_counts = database
        .read(move |transaction| {
            transaction
                .get::<Record>(Table::UsageWindows, &month_key_model)
                .map(|record| record.map(|record| record_counts(&record)).transpose())
        })
        .await
        .expect("read month API key model")
        .expect("API key model exists")
        .expect("valid API key model counts");
    assert_eq!(model_counts.requests, 1);
    drop(database);
    let _ = fs::remove_dir_all(&path);
}
