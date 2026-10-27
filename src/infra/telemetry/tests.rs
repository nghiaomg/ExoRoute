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
    telemetry
        .shutdown(Duration::from_secs(2))
        .await
        .expect("bounded shutdown drain");

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
async fn shutdown_reports_a_stuck_writer_within_its_budget() {
    let (database, path) = test_db().await;
    let telemetry = Telemetry::new(database.clone());

    // Hold the single LMDB writer so the telemetry writer cannot answer the
    // flush. This is exactly the shutdown case the budget exists for: the
    // database is still committing while the process wants to exit.
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let (entered_sender, mut entered_receiver) = tokio::sync::mpsc::channel(1);
    let blocker = tokio::spawn({
        let database = database.clone();
        let barrier = barrier.clone();
        async move {
            database
                .write(move |_| {
                    entered_sender.blocking_send(()).map_err(|_| {
                        StorageError::Task("test entry signal was closed".to_owned())
                    })?;
                    barrier.wait();
                    Ok(())
                })
                .await
        }
    });
    entered_receiver
        .recv()
        .await
        .expect("blocking write entered");

    telemetry.record_api_key_request("key-stuck".to_owned());
    let error = telemetry
        .shutdown(Duration::from_millis(700))
        .await
        .expect_err("a stuck writer cannot drain inside the budget");
    assert!(
        error.contains("bounded drain"),
        "shutdown reported an unexpected error: {error}"
    );

    let release = barrier.clone();
    tokio::task::spawn_blocking(move || release.wait())
        .await
        .expect("release the held LMDB writer");
    blocker
        .await
        .expect("join blocker")
        .expect("write completes");
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
    assert_eq!(snapshot["logged_requests"], 1);
    assert_eq!(
        snapshot["model_breakdown_incomplete"], false,
        "a request that is in history keeps the breakdown complete"
    );
    assert_eq!(snapshot["input_tokens"], 10);
    assert_eq!(snapshot["cached_tokens"], 6);
    assert_eq!(snapshot["cache_ratio"], 60.0);
    assert_eq!(snapshot["api_keys"]["top"][0]["label"], "Key One");
    assert_eq!(snapshot["model_breakdown"][0]["model"], "provider/model");
    assert_eq!(snapshot["model_breakdown"][0]["provider"], "provider");
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
    assert_eq!(key_snapshot["model_breakdown"][0]["provider"], "provider");
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
async fn model_breakdown_splits_one_model_by_serving_provider() {
    let (database, path) = test_db().await;
    let telemetry = Telemetry::new(database.clone());
    // Every row calls the same model through the same route alias; only the
    // provider that actually served the request differs. The breakdown has to
    // attribute the usage to that provider instead of collapsing the rows under
    // the alias.
    for (id, provider_id, status, input_tokens) in [
        ("log-provider-a", Some("provider-a"), 200, 10),
        ("log-provider-b", Some("provider-b"), 500, 4),
        ("log-without-provider", None, 500, 2),
    ] {
        telemetry.enqueue_request_log(RequestLogRecord {
            id: id.to_owned(),
            request_id: format!("request-{id}"),
            route_alias: "shared-combo".to_owned(),
            provider_id: provider_id.map(str::to_owned),
            provider_credential_id: None,
            api_key_id: None,
            model: "shared/model".to_owned(),
            client_protocol: "chat_completions".to_owned(),
            upstream_protocol: Some("chat_completions".to_owned()),
            status,
            duration_ms: 12,
            input_tokens: Some(input_tokens),
            output_tokens: Some(1),
            cached_tokens: None,
            cache_input_tokens: None,
            cost_micro_usd: None,
            error: None,
        });
    }
    telemetry.flush().await.expect("flush telemetry");

    let snapshot = statistics_snapshot(&database, "1d", 1440, &telemetry)
        .await
        .expect("statistics snapshot");
    let rows = snapshot["model_breakdown"]
        .as_array()
        .expect("model breakdown rows");
    assert_eq!(rows.len(), 3, "each serving provider keeps its own row");
    assert!(
        rows.iter().all(|row| row["model"] == "shared/model"),
        "the rows stay grouped by model"
    );
    let provider_a = rows
        .iter()
        .find(|row| row["provider"] == "provider-a")
        .expect("row for provider A");
    assert_eq!(provider_a["requests"], 1);
    assert_eq!(provider_a["input_tokens"], 10);
    assert_eq!(provider_a["success_rate"], 100.0);
    let provider_b = rows
        .iter()
        .find(|row| row["provider"] == "provider-b")
        .expect("row for provider B");
    assert_eq!(provider_b["requests"], 1);
    assert_eq!(provider_b["input_tokens"], 4);
    assert_eq!(provider_b["success_rate"], 0.0);
    let unknown = rows
        .iter()
        .find(|row| row["provider"].is_null())
        .expect("row for a request that never reached a provider");
    assert_eq!(unknown["requests"], 1);
    assert_eq!(unknown["input_tokens"], 2);
    drop(telemetry);
    drop(database);
    let _ = fs::remove_dir_all(&path);
}

#[tokio::test]
async fn statistics_flag_history_that_covers_fewer_requests_than_the_totals() {
    let (database, path) = test_db().await;
    let telemetry = Telemetry::new(database.clone());
    // The rollup counts three requests while only one request-log row is
    // retained, which is what a bounded request history looks like once the
    // retained rows or retention days cover less traffic than the selected
    // statistics range. The totals stay authoritative; the model breakdown
    // reports that it covers less.
    for _ in 0..3 {
        let analytics = telemetry.start_request("key-1".to_owned());
        analytics.set_requested_model(Some("provider/model"));
        analytics.set_token_usage(Some(10), Some(4));
        analytics.finish(true);
    }
    telemetry.enqueue_request_log(RequestLogRecord {
        id: "log-retained".to_owned(),
        request_id: "request-retained".to_owned(),
        route_alias: "default".to_owned(),
        provider_id: Some("provider".to_owned()),
        provider_credential_id: None,
        api_key_id: Some("key-1".to_owned()),
        model: "provider/model".to_owned(),
        client_protocol: "chat_completions".to_owned(),
        upstream_protocol: Some("chat_completions".to_owned()),
        status: 200,
        duration_ms: 12,
        input_tokens: Some(10),
        output_tokens: Some(4),
        cached_tokens: None,
        cache_input_tokens: None,
        cost_micro_usd: None,
        error: None,
    });
    telemetry.flush().await.expect("flush telemetry");

    let snapshot = statistics_snapshot(&database, "30d", 30 * 24 * 60, &telemetry)
        .await
        .expect("statistics snapshot");
    assert_eq!(snapshot["total_requests"], 3);
    assert_eq!(snapshot["logged_requests"], 1);
    assert_eq!(snapshot["model_breakdown_incomplete"], true);
    assert_eq!(snapshot["model_breakdown_truncated"], false);
    assert_eq!(
        snapshot["model_breakdown"][0]["requests"], 1,
        "the retained history is reported as-is"
    );
    drop(telemetry);
    drop(database);
    let _ = fs::remove_dir_all(&path);
}

/// Builds a retained request-log row from the fields the history views read, so
/// each test only spells out the values it asserts on.
fn history_row(
    id: &str,
    provider_id: Option<&str>,
    api_key_id: Option<&str>,
    status: i64,
    duration_ms: i64,
    cost_micro_usd: Option<i64>,
) -> RequestLogRecord {
    RequestLogRecord {
        id: id.to_owned(),
        request_id: format!("request-{id}"),
        route_alias: "shared-combo".to_owned(),
        provider_id: provider_id.map(str::to_owned),
        provider_credential_id: None,
        api_key_id: api_key_id.map(str::to_owned),
        model: "shared/model".to_owned(),
        client_protocol: "chat_completions".to_owned(),
        upstream_protocol: Some("chat_completions".to_owned()),
        status,
        duration_ms,
        input_tokens: Some(10),
        output_tokens: Some(4),
        cached_tokens: None,
        cache_input_tokens: None,
        cost_micro_usd,
        error: None,
    }
}

#[tokio::test]
async fn history_views_group_retained_rows_by_provider_status_cost_and_latency() {
    let (database, path) = test_db().await;
    let telemetry = Telemetry::new(database.clone());
    // Three retained rows: two served by provider-a (one fast, one mid), and one
    // that failed at provider-b after a slow response and reported no cost.
    for (id, provider, key, status, duration_ms, cost) in [
        (
            "log-a-fast",
            Some("provider-a"),
            Some("key-1"),
            200,
            80,
            Some(1_000),
        ),
        (
            "log-a-slow",
            Some("provider-a"),
            Some("key-1"),
            200,
            300,
            Some(2_000),
        ),
        (
            "log-b-failed",
            Some("provider-b"),
            Some("key-2"),
            502,
            20_000,
            None,
        ),
    ] {
        telemetry.enqueue_request_log(history_row(id, provider, key, status, duration_ms, cost));
    }
    telemetry.flush().await.expect("flush telemetry");

    let snapshot = statistics_snapshot(&database, "1d", 1440, &telemetry)
        .await
        .expect("statistics snapshot");
    assert_eq!(snapshot["logged_requests"], 3);
    assert_eq!(snapshot["history_truncated"], false);
    assert_eq!(
        snapshot["model_breakdown_incomplete"], false,
        "the flag only fires when history covers fewer rows than the totals; here no rollup \
         aggregate was seeded, so it is the totals that cover less"
    );

    // Providers are ranked by request count; a lower-ranked provider keeps its
    // own row instead of being merged into the leader.
    assert_eq!(snapshot["providers"]["truncated"], false);
    let providers = snapshot["providers"]["rows"]
        .as_array()
        .expect("provider rows");
    assert_eq!(providers.len(), 2);
    let provider_a = &providers[0];
    assert_eq!(provider_a["provider"], "provider-a");
    assert_eq!(provider_a["requests"], 2);
    assert_eq!(provider_a["successes"], 2);
    assert_eq!(provider_a["success_rate"], 100.0);
    assert_eq!(provider_a["average_latency_ms"], 190.0);
    assert_eq!(provider_a["input_tokens"], 20);
    assert_eq!(provider_a["cost_micro_usd"], 3_000);
    assert_eq!(provider_a["cost_reported_micro_usd"], 3_000);
    assert_eq!(provider_a["cost_events"], 2);
    assert_eq!(provider_a["missing_cost_events"], 0);
    let provider_b = &providers[1];
    assert_eq!(provider_b["provider"], "provider-b");
    assert_eq!(provider_b["requests"], 1);
    assert_eq!(provider_b["successes"], 0);
    assert_eq!(provider_b["success_rate"], 0.0);
    assert_eq!(provider_b["average_latency_ms"], 20_000.0);
    // A provider whose requests never reported a cost has no total to show: the
    // exact total stays null while the reported sum records what is known.
    assert!(
        provider_b["cost_micro_usd"].is_null(),
        "a partial spend total must not read as a complete one"
    );
    assert_eq!(provider_b["cost_reported_micro_usd"], 0);
    assert_eq!(provider_b["cost_events"], 0);
    assert_eq!(provider_b["missing_cost_events"], 1);

    assert_eq!(snapshot["statuses"]["truncated"], false);
    let statuses = snapshot["statuses"]["rows"]
        .as_array()
        .expect("status rows");
    assert_eq!(statuses.len(), 2);
    assert_eq!(statuses[0]["status"], 200);
    assert_eq!(statuses[0]["requests"], 2);
    assert_eq!(statuses[0]["successes"], 2);
    assert_eq!(statuses[1]["status"], 502);
    assert_eq!(statuses[1]["requests"], 1);
    assert_eq!(statuses[1]["successes"], 0);

    // The 1d range is one bucket per hour, and rows written by this test land in
    // the newest one.
    assert_eq!(snapshot["series"]["truncated"], false);
    assert_eq!(snapshot["series"]["bucket_seconds"], 3_600);
    let points = snapshot["series"]["points"]
        .as_array()
        .expect("series points");
    assert_eq!(points.len(), 24);
    let bucketed = points
        .iter()
        .map(|point| point["requests"].as_i64().unwrap_or_default())
        .sum::<i64>();
    assert_eq!(
        bucketed, 3,
        "every retained row lands in exactly one bucket"
    );
    let newest = &points[23];
    assert_eq!(newest["requests"], 3);
    assert_eq!(newest["successes"], 2);
    assert_eq!(newest["failures"], 1);
    assert_eq!(newest["input_tokens"], 30);
    assert_eq!(newest["output_tokens"], 12);
    assert!(newest["cost_micro_usd"].is_null());
    assert_eq!(newest["cost_reported_micro_usd"], 3_000);
    assert_eq!(newest["missing_cost_events"], 1);

    // Latency is estimated from a fixed histogram, so the reported percentiles
    // are bucket tops and the slowest bucket makes them a floor.
    let latency = &snapshot["latency"];
    assert_eq!(latency["samples"], 3);
    assert_eq!(latency["open_ended"], true);
    let average = latency["average_ms"].as_f64().expect("average latency");
    assert!(
        (average - 6_793.333).abs() < 0.01,
        "average latency was {average}"
    );
    let buckets = latency["buckets"]
        .as_array()
        .expect("latency buckets")
        .iter()
        .map(|bucket| bucket.as_i64().expect("bucket count"))
        .collect::<Vec<_>>();
    assert_eq!(buckets, vec![1, 0, 1, 0, 0, 0, 0, 1]);
    assert_eq!(latency["p50_ms"], 500.0);
    // One slow request exists, but it decides only the percentiles whose own
    // sample lands in the open bucket, not the median.
    assert_eq!(latency["p50_is_floor"], false);
    assert_eq!(latency["p95_ms"], 15_000.0);
    assert_eq!(latency["p95_is_floor"], true);
    assert_eq!(latency["p99_ms"], 15_000.0);
    assert_eq!(latency["p99_is_floor"], true);

    let cost = &snapshot["cost"];
    assert_eq!(cost["requests"], 3);
    assert!(cost["cost_micro_usd"].is_null());
    assert_eq!(cost["cost_reported_micro_usd"], 3_000);
    assert_eq!(cost["cost_events"], 2);
    assert_eq!(cost["missing_cost_events"], 1);
    drop(telemetry);
    drop(database);
    let _ = fs::remove_dir_all(&path);
}

#[tokio::test]
async fn time_to_first_token_average_covers_only_streamed_requests() {
    let (database, path) = test_db().await;
    let current = unix_minute();
    let recent = current.saturating_sub(5);
    let base = StatisticsEvent {
        api_key_id: "key".to_owned(),
        requested_model: "model".to_owned(),
        minute: recent,
        succeeded: true,
        duration_ms: 8,
        input_tokens: 3,
        output_tokens: 2,
        cached_tokens: 0,
        cache_input_tokens: 3,
        first_output_ms: Some(120),
    };
    let events = [
        StatisticsEvent {
            first_output_ms: Some(120),
            ..base.clone()
        },
        StatisticsEvent {
            first_output_ms: Some(180),
            ..base.clone()
        },
        // A request that never produced output contributes no sample, or the
        // average would describe every request instead of the streamed ones.
        StatisticsEvent {
            first_output_ms: None,
            ..base.clone()
        },
    ];
    let mut aggregates = HashMap::new();
    for event in &events {
        add_aggregate(&mut aggregates, event.minute, 0, "", event);
    }
    database
        .write(move |transaction| {
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

    let telemetry = Telemetry::new(database.clone());
    let snapshot = statistics_snapshot(&database, "1d", 1440, &telemetry)
        .await
        .expect("statistics snapshot");
    assert_eq!(snapshot["total_requests"], 3);
    assert_eq!(snapshot["time_to_first_token_samples"], 2);
    assert_eq!(snapshot["average_time_to_first_token_ms"], 150.0);

    // Dropping the counters behind the average clears both the value and its
    // sample count, so a period without streamed responses reports no estimate
    // instead of a stale one.
    telemetry
        .clear_request_history()
        .await
        .expect("clear request history");
    let cleared = statistics_snapshot(&database, "1d", 1440, &telemetry)
        .await
        .expect("cleared statistics snapshot");
    assert_eq!(cleared["total_requests"], 0);
    assert_eq!(cleared["time_to_first_token_samples"], 0);
    assert!(cleared["average_time_to_first_token_ms"].is_null());
    drop(telemetry);
    drop(database);
    let _ = fs::remove_dir_all(&path);
}

#[tokio::test]
async fn traffic_time_series_reflects_usage_minutes_when_request_logs_are_pruned() {
    let (database, path) = test_db().await;
    let telemetry = Telemetry::new(database.clone());
    let current_minute = unix_minute();

    let base = Counts {
        requests: 5,
        succeeded: 4,
        failed: 1,
        duration_ms: 1_000,
        input_tokens: 50_000,
        output_tokens: 2_500,
        cached_tokens: 10_000,
        cache_input_tokens: 50_000,
        first_output_ms: 0,
        first_output_events: 0,
    };
    let minutes_and_counts = [
        (current_minute, base),
        (current_minute - 120, base),
        (current_minute - 300, base),
    ];

    database
        .write(move |transaction| {
            for (minute, counts) in minutes_and_counts {
                let key = minute_key(minute, 0, "")?;
                transaction.put(Table::UsageMinutes, &key, &counts_record(counts))?;
                for (window, _) in STATISTICS_RANGES {
                    let aggregate_key = window_key(window, 0, "")?;
                    update_window_counts(transaction, &aggregate_key, window, 0, "", counts)?;
                }
            }
            Ok(())
        })
        .await
        .expect("seed usage minutes");

    telemetry.enqueue_request_log(RequestLogRecord {
        id: "log-only-one".to_owned(),
        request_id: "request-only-one".to_owned(),
        route_alias: "default".to_owned(),
        provider_id: Some("provider".to_owned()),
        provider_credential_id: None,
        api_key_id: None,
        model: "provider/model".to_owned(),
        client_protocol: "chat_completions".to_owned(),
        upstream_protocol: Some("chat_completions".to_owned()),
        status: 200,
        duration_ms: 200,
        input_tokens: Some(10_000),
        output_tokens: Some(500),
        cached_tokens: None,
        cache_input_tokens: None,
        cost_micro_usd: None,
        error: None,
    });
    telemetry.flush().await.expect("flush telemetry");

    let snapshot = statistics_snapshot(&database, "1d", 1440, &telemetry)
        .await
        .expect("statistics snapshot");

    assert_eq!(snapshot["total_requests"], 15);
    assert_eq!(snapshot["input_tokens"], 150_000);
    assert_eq!(snapshot["output_tokens"], 7_500);
    assert_eq!(snapshot["logged_requests"], 1);

    let points = snapshot["series"]["points"]
        .as_array()
        .expect("series points");
    assert_eq!(points.len(), 24);

    let total_series_requests: i64 = points
        .iter()
        .map(|p| p["requests"].as_i64().unwrap_or(0))
        .sum();
    let total_series_input_tokens: i64 = points
        .iter()
        .map(|p| p["input_tokens"].as_i64().unwrap_or(0))
        .sum();
    let total_series_output_tokens: i64 = points
        .iter()
        .map(|p| p["output_tokens"].as_i64().unwrap_or(0))
        .sum();

    assert_eq!(
        total_series_requests, 15,
        "all requests across all hours are represented in series"
    );
    assert_eq!(
        total_series_input_tokens, 150_000,
        "all input tokens are represented in series"
    );
    assert_eq!(
        total_series_output_tokens, 7_500,
        "all output tokens are represented in series"
    );

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
        .shutdown(Duration::from_secs(2))
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
        first_output_ms: Some(40),
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
    // Expiring the old bucket has to remove its time-to-first-output sample
    // too, or the rolling average would keep weighing a request that is no
    // longer in the window.
    assert_eq!(counts.first_output_ms, 40);
    assert_eq!(counts.first_output_events, 1);
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

/// Enqueues one request worth of telemetry the way the gateway's request path
/// does: an API key usage event, a statistics event, a request log, and a
/// provider cost event.
fn enqueue_request(telemetry: &Telemetry, index: usize) {
    let api_key_id = if index.is_multiple_of(2) {
        "key-1"
    } else {
        "key-2"
    };
    telemetry.record_api_key_request(api_key_id.to_owned());
    let analytics = telemetry.start_request(api_key_id.to_owned());
    analytics.set_requested_model(Some("provider/model"));
    analytics.set_token_usage(Some(10), Some(4));
    analytics.finish(true);
    telemetry.enqueue_request_log(RequestLogRecord {
        id: format!("log-{index}"),
        request_id: format!("request-{index}"),
        route_alias: "default".to_owned(),
        provider_id: Some("provider".to_owned()),
        provider_credential_id: Some("cred-1".to_owned()),
        api_key_id: Some(api_key_id.to_owned()),
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
    telemetry.record_provider_usage("cred-1".to_owned(), Some(125_000), Some(10), Some(4));
}

/// Request rates this harness paces through the telemetry writer.
///
/// The 10 ms pacing step caps the delivered rate at roughly 1.2k requests per
/// second on Windows, so a higher target would only repeat the same load.
const SWEEP_RATES: [usize; 5] = [125, 250, 500, 1_000, 2_000];
const SWEEP_SECONDS: usize = 3;

/// Sustained request traffic must reach the writer without the bounded queue
/// overflowing.
///
/// This sweeps the rates a single process is expected to serve and prints what
/// each one measured. The loss budget stays loose so a slower machine does not
/// turn a timing measurement into a flake, while still catching the regression
/// this replaced: when every dropped event woke the writer for a counters
/// transaction, the same sweep lost 13% of its events at the top rates.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn paced_request_traffic_does_not_overflow_the_telemetry_queue() {
    let (database, path) = test_db().await;
    database
        .write(|transaction| {
            transaction.put(Table::ApiKeys, "key-1", &api_key_record("Key One"))?;
            transaction.put(Table::ApiKeys, "key-2", &api_key_record("Key Two"))?;
            transaction.put(
                Table::ProviderApiKeys,
                "cred-1",
                &Record::new().with("id", Field::Text("cred-1".to_owned())),
            )
        })
        .await
        .expect("seed records");
    let telemetry = Telemetry::new(database.clone());
    let mut index = 0;
    for rate in SWEEP_RATES {
        let before = telemetry.drop_counters().total();
        let requests = rate * SWEEP_SECONDS;
        let chunk = (rate / 100).max(1);
        let started = std::time::Instant::now();
        let mut sent = 0;
        while sent < requests {
            let this_chunk = chunk.min(requests - sent);
            for _ in 0..this_chunk {
                enqueue_request(&telemetry, index);
                index += 1;
            }
            sent += this_chunk;
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        telemetry.flush().await.expect("flush telemetry");
        let dropped = telemetry.drop_counters().total() - before;
        let events = requests * 4;
        let drop_pct = dropped as f64 * 100.0 / events as f64;
        println!(
            "SWEEP rate={rate}/s events={events} dropped={dropped} drop_pct={drop_pct:.2} paced_ms={}",
            started.elapsed().as_millis(),
        );
        if rate <= 500 {
            assert_eq!(
                dropped, 0,
                "request logging at {rate}/s lost {dropped} of {events} events"
            );
        }
        assert!(
            drop_pct <= 1.0,
            "request logging at {rate}/s lost {drop_pct:.2}% of its events"
        );
    }
    drop(telemetry);
    drop(database);
    let _ = fs::remove_dir_all(&path);
}

#[tokio::test]
async fn time_series_matches_usage_minutes_even_when_request_logs_are_pruned() {
    let (database, path) = test_db().await;
    let now_minute = unix_minute();
    let minute_5h_ago = now_minute - 300;
    let minute_2h_ago = now_minute - 120;
    let minute_now = now_minute;

    let counts_5h = Counts {
        requests: 50,
        succeeded: 48,
        failed: 2,
        duration_ms: 100_000,
        input_tokens: 1_000_000,
        output_tokens: 20_000,
        cached_tokens: 500_000,
        cache_input_tokens: 1_000_000,
        first_output_ms: 5_000,
        first_output_events: 48,
    };
    let counts_2h = Counts {
        requests: 100,
        succeeded: 99,
        failed: 1,
        duration_ms: 250_000,
        input_tokens: 2_500_000,
        output_tokens: 50_000,
        cached_tokens: 1_200_000,
        cache_input_tokens: 2_500_000,
        first_output_ms: 10_000,
        first_output_events: 99,
    };
    let counts_now = Counts {
        requests: 1,
        succeeded: 1,
        failed: 0,
        duration_ms: 4_683,
        input_tokens: 91_884,
        output_tokens: 54,
        cached_tokens: 0,
        cache_input_tokens: 91_884,
        first_output_ms: 500,
        first_output_events: 1,
    };

    let total = counts_5h.add(counts_2h).add(counts_now);

    database
        .write(move |transaction| {
            let k_5h = minute_key(minute_5h_ago, 0, "")?;
            let k_2h = minute_key(minute_2h_ago, 0, "")?;
            let k_now = minute_key(minute_now, 0, "")?;
            transaction.put(Table::UsageMinutes, &k_5h, &counts_record(counts_5h))?;
            transaction.put(Table::UsageMinutes, &k_2h, &counts_record(counts_2h))?;
            transaction.put(Table::UsageMinutes, &k_now, &counts_record(counts_now))?;

            for (window, _) in STATISTICS_RANGES {
                let w_key = window_key(window, 0, "")?;
                update_window_counts(transaction, &w_key, window, 0, "", total)?;
            }
            Ok(())
        })
        .await
        .expect("seed usage minutes");

    let telemetry = Telemetry::new(database.clone());
    telemetry.enqueue_request_log(RequestLogRecord {
        id: "log-retained-recent".to_owned(),
        request_id: "req-retained".to_owned(),
        route_alias: "default".to_owned(),
        provider_id: Some("provider".to_owned()),
        provider_credential_id: None,
        api_key_id: None,
        model: "provider/model".to_owned(),
        client_protocol: "chat_completions".to_owned(),
        upstream_protocol: Some("chat_completions".to_owned()),
        status: 200,
        duration_ms: 4_683,
        input_tokens: Some(91_884),
        output_tokens: Some(54),
        cached_tokens: None,
        cache_input_tokens: None,
        cost_micro_usd: None,
        error: None,
    });
    telemetry.flush().await.expect("flush telemetry");

    let snapshot = statistics_snapshot(&database, "1d", 1440, &telemetry)
        .await
        .expect("statistics snapshot");

    assert_eq!(snapshot["total_requests"], 151);
    assert_eq!(snapshot["input_tokens"], 3_591_884);
    assert_eq!(snapshot["output_tokens"], 70_054);
    assert_eq!(snapshot["logged_requests"], 1);
    assert_eq!(snapshot["model_breakdown_incomplete"], true);

    let points = snapshot["series"]["points"]
        .as_array()
        .expect("points array");
    assert_eq!(points.len(), 24);

    let sum_requests: i64 = points
        .iter()
        .map(|p| p["requests"].as_i64().unwrap_or(0))
        .sum();
    let sum_input_tokens: i64 = points
        .iter()
        .map(|p| p["input_tokens"].as_i64().unwrap_or(0))
        .sum();
    let sum_output_tokens: i64 = points
        .iter()
        .map(|p| p["output_tokens"].as_i64().unwrap_or(0))
        .sum();
    let sum_successes: i64 = points
        .iter()
        .map(|p| p["successes"].as_i64().unwrap_or(0))
        .sum();
    let sum_failures: i64 = points
        .iter()
        .map(|p| p["failures"].as_i64().unwrap_or(0))
        .sum();

    assert_eq!(sum_requests, snapshot["total_requests"].as_i64().unwrap());
    assert_eq!(sum_input_tokens, snapshot["input_tokens"].as_i64().unwrap());
    assert_eq!(
        sum_output_tokens,
        snapshot["output_tokens"].as_i64().unwrap()
    );
    assert_eq!(sum_successes, snapshot["successes"].as_i64().unwrap());
    assert_eq!(sum_failures, snapshot["failures"].as_i64().unwrap());

    let non_zero_buckets = points
        .iter()
        .filter(|p| p["requests"].as_i64().unwrap_or(0) > 0)
        .count();
    assert_eq!(
        non_zero_buckets, 3,
        "all 3 distinct hourly buckets must have data"
    );

    drop(telemetry);
    drop(database);
    let _ = fs::remove_dir_all(&path);
}

#[tokio::test]
async fn api_key_time_series_matches_usage_minutes_for_api_key() {
    let (database, path) = test_db().await;
    let now_minute = unix_minute();
    let minute_3h_ago = now_minute - 180;

    let counts = Counts {
        requests: 25,
        succeeded: 25,
        failed: 0,
        duration_ms: 50_000,
        input_tokens: 500_000,
        output_tokens: 10_000,
        cached_tokens: 200_000,
        cache_input_tokens: 500_000,
        first_output_ms: 2_000,
        first_output_events: 25,
    };

    database
        .write(move |transaction| {
            transaction.put(Table::ApiKeys, "key-ts", &api_key_record("Key TimeSeries"))?;
            let k = minute_key(minute_3h_ago, 1, "key-ts")?;
            transaction.put(Table::UsageMinutes, &k, &counts_record(counts))?;
            for (window, _) in STATISTICS_RANGES {
                let w_key = window_key(window, 1, "key-ts")?;
                update_window_counts(transaction, &w_key, window, 1, "key-ts", counts)?;
            }
            Ok(())
        })
        .await
        .expect("seed API key usage minutes");

    let telemetry = Telemetry::new(database.clone());
    let snapshot = api_key_statistics_snapshot(&database, "key-ts", "1d", 1440, &telemetry)
        .await
        .expect("API key statistics snapshot");

    assert_eq!(snapshot["total_requests"], 25);
    assert_eq!(snapshot["input_tokens"], 500_000);
    assert_eq!(snapshot["output_tokens"], 10_000);

    let points = snapshot["series"]["points"]
        .as_array()
        .expect("series points");
    assert_eq!(points.len(), 24);
    let sum_tokens: i64 = points
        .iter()
        .map(|p| p["input_tokens"].as_i64().unwrap_or(0))
        .sum();
    assert_eq!(sum_tokens, 500_000);

    drop(telemetry);
    drop(database);
    let _ = fs::remove_dir_all(&path);
}
