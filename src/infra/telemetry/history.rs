//! Bounded request-history analytics.
//!
//! Every view here comes from a single reverse pass over the retained
//! request-log index, so the dashboard can render a time series, provider and
//! status rankings, latency percentiles and spend without adding a dimension to
//! the per-minute rollups and without writing extra records on the request
//! path.
//!
//! Request history is bounded three ways: the retention window, the retained
//! row count, and the scan limit below. The rolling rollups are not bounded the
//! same way, so every view reports how many log rows it covered and the caller
//! surfaces that next to the rollup totals instead of implying the two describe
//! the same traffic.

use super::{cache_ratio_percent, unix_seconds};
use crate::infra::{
    db,
    storage::{ReadTxn, Record, StorageError, Table},
};
use std::collections::BTreeMap;

/// Log rows one history scan may visit. Bounds the work a single dashboard
/// request can do no matter how much traffic the installation has served.
pub(crate) const MAX_HISTORY_SCAN_ROWS: usize = 100_000;
/// Model rows kept after grouping; the tail is dropped and flagged.
pub(crate) const MAX_HISTORY_MODEL_ROWS: usize = 256;
/// Provider rows kept after grouping.
pub(crate) const MAX_HISTORY_PROVIDER_ROWS: usize = 64;
/// Distinct status codes kept after grouping.
pub(crate) const MAX_HISTORY_STATUS_ROWS: usize = 32;

/// Exclusive upper bounds, in milliseconds, of the latency histogram. The final
/// bucket is open-ended, so there is one more bucket than there are bounds.
const LATENCY_BUCKET_BOUNDS_MS: [i64; 7] = [100, 250, 500, 1_000, 2_000, 5_000, 15_000];

/// How the caller wants the time series sliced. Buckets are contiguous and all
/// the same width, the last one ends at `end_seconds`, and a timestamp at or
/// after `end_seconds` still lands in that last bucket so clock skew between the
/// stored row and the query cannot push a row out of the series.
#[derive(Clone, Copy)]
pub(crate) struct SeriesSpec {
    end_seconds: i64,
    pub(crate) bucket_seconds: i64,
    bucket_count: usize,
}

/// Oldest second the range covers. Compared against the seconds parsed from a
/// log index key, so an out-of-range row stops the scan without a second parse.
pub(crate) fn history_cutoff(window_minutes: i64) -> i64 {
    unix_seconds().saturating_sub(window_minutes.max(0).saturating_mul(60))
}

/// Picks a bucket width that keeps the chart readable at every supported range:
/// hourly for a day, six-hourly for a week, daily for a month.
pub(crate) fn series_spec(window_minutes: i64) -> SeriesSpec {
    let (bucket_seconds, bucket_count) = if window_minutes <= 24 * 60 {
        (3_600, 24)
    } else if window_minutes <= 7 * 24 * 60 {
        (21_600, 28)
    } else {
        (86_400, 30)
    };
    SeriesSpec {
        end_seconds: unix_seconds(),
        bucket_seconds,
        bucket_count,
    }
}

#[derive(Default, Clone, Copy)]
struct GroupCounts {
    requests: i64,
    successes: i64,
    input_tokens: i64,
    output_tokens: i64,
    cached_tokens: i64,
    cache_input_tokens: i64,
    duration_ms: i64,
    cost_micro_usd: i64,
    cost_events: i64,
}

impl GroupCounts {
    fn add(&mut self, row: &LogRow) {
        self.requests = self.requests.saturating_add(1);
        if row.succeeded {
            self.successes = self.successes.saturating_add(1);
        }
        self.input_tokens = self.input_tokens.saturating_add(row.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(row.output_tokens);
        self.cached_tokens = self.cached_tokens.saturating_add(row.cached_tokens);
        self.cache_input_tokens = self
            .cache_input_tokens
            .saturating_add(row.cache_input_tokens);
        self.duration_ms = self.duration_ms.saturating_add(row.duration_ms);
        if let Some(cost) = row.cost_micro_usd {
            self.cost_micro_usd = self.cost_micro_usd.saturating_add(cost);
            self.cost_events = self.cost_events.saturating_add(1);
        }
    }

    fn missing_cost_events(&self) -> i64 {
        self.requests.saturating_sub(self.cost_events)
    }

    /// Cost is only meaningful as a total when every grouped request reported
    /// one; a partial sum would understate spend while looking complete.
    fn complete_cost(&self) -> Option<i64> {
        (self.requests > 0 && self.cost_events == self.requests).then_some(self.cost_micro_usd)
    }

    fn success_rate(&self) -> f64 {
        if self.requests > 0 {
            self.successes as f64 / self.requests as f64 * 100.0
        } else {
            0.0
        }
    }

    fn average_latency_ms(&self) -> f64 {
        if self.requests > 0 {
            self.duration_ms as f64 / self.requests as f64
        } else {
            0.0
        }
    }

    /// Cost fields shared by every view that reports spend.
    fn cost_fields(&self) -> [(&'static str, serde_json::Value); 4] {
        [
            ("cost_micro_usd", serde_json::json!(self.complete_cost())),
            (
                "cost_reported_micro_usd",
                serde_json::json!(self.cost_micro_usd),
            ),
            ("cost_events", serde_json::json!(self.cost_events)),
            (
                "missing_cost_events",
                serde_json::json!(self.missing_cost_events()),
            ),
        ]
    }
}

/// The fields one retained log row contributes to every view.
struct LogRow {
    model: String,
    provider: Option<String>,
    status: i64,
    succeeded: bool,
    duration_ms: i64,
    input_tokens: i64,
    output_tokens: i64,
    cached_tokens: i64,
    cache_input_tokens: i64,
    cost_micro_usd: Option<i64>,
}

pub(crate) struct HistoryScan {
    pub(crate) models: Vec<serde_json::Value>,
    pub(crate) models_truncated: bool,
    pub(crate) providers: Vec<serde_json::Value>,
    pub(crate) providers_truncated: bool,
    pub(crate) statuses: Vec<serde_json::Value>,
    pub(crate) statuses_truncated: bool,
    pub(crate) series: Vec<serde_json::Value>,
    pub(crate) latency: serde_json::Value,
    pub(crate) cost: serde_json::Value,
    /// Retained log rows this scan covered, after the API-key filter.
    pub(crate) logged_requests: i64,
    /// True when the scan stopped at its row limit before reaching the cutoff,
    /// so every view here understates the range.
    pub(crate) scan_truncated: bool,
}

/// Groups retained log rows inside the range into every view the dashboard
/// needs. `cutoff` is the oldest `created_at` the range covers; the scan stops
/// as soon as it walks past it.
pub(crate) fn scan_history(
    transaction: &ReadTxn<'_, '_>,
    cutoff: i64,
    api_key_id: Option<&str>,
    series: SeriesSpec,
) -> Result<HistoryScan, StorageError> {
    let mut models = BTreeMap::<(String, Option<String>), GroupCounts>::new();
    let mut providers = BTreeMap::<Option<String>, GroupCounts>::new();
    let mut statuses = BTreeMap::<i64, GroupCounts>::new();
    let mut buckets = vec![GroupCounts::default(); series.bucket_count];
    let mut latency_buckets = [0_i64; LATENCY_BUCKET_BOUNDS_MS.len() + 1];
    let mut totals = GroupCounts::default();
    let mut logged_requests = 0_i64;
    let mut after = None::<String>;
    let mut scanned = 0usize;
    let mut reached_cutoff = false;

    while scanned < MAX_HISTORY_SCAN_ROWS && !reached_cutoff {
        let batch_limit = (MAX_HISTORY_SCAN_ROWS - scanned).min(256);
        let page = transaction.scan_prefix_reverse_after::<String>(
            Table::RequestLogIndex,
            db::REQUEST_LOG_INDEX_PREFIX,
            after.as_deref(),
            batch_limit,
        )?;
        if page.is_empty() {
            break;
        }
        for (index_key, id) in page {
            scanned = scanned.saturating_add(1);
            let created_at = request_log_created_at(&index_key)?;
            if created_at < cutoff {
                reached_cutoff = true;
                break;
            }
            after = Some(index_key);
            let record = transaction
                .get::<Record>(Table::RequestLogs, &id)?
                .ok_or_else(|| {
                    StorageError::Invalid("request log index points to a missing record".to_owned())
                })?;
            if let Some(expected) = api_key_id
                && record.optional_text("api_key_id")? != Some(expected)
            {
                continue;
            }
            logged_requests = logged_requests.saturating_add(1);
            let row = decode_row(&record)?;
            totals.add(&row);
            statuses.entry(row.status).or_default().add(&row);
            providers.entry(row.provider.clone()).or_default().add(&row);
            if !row.model.is_empty() {
                models
                    .entry((row.model.clone(), row.provider.clone()))
                    .or_default()
                    .add(&row);
            }
            if let Some(index) = series_index(created_at, series) {
                buckets[index].add(&row);
            }
            let bucket = latency_bucket(row.duration_ms);
            latency_buckets[bucket] = latency_buckets[bucket].saturating_add(1);
        }
    }

    let scan_truncated = !reached_cutoff && scanned >= MAX_HISTORY_SCAN_ROWS;
    let (models, models_cut) =
        group_rows(models, MAX_HISTORY_MODEL_ROWS, |(model, provider), c| {
            serde_json::json!({
                "model": model,
                "provider": provider,
                "input_tokens": c.input_tokens,
                "output_tokens": c.output_tokens,
                "cache_hit_rate": cache_ratio_percent(c.cached_tokens, c.cache_input_tokens),
                "requests": c.requests,
                "successes": c.successes,
                "success_rate": c.success_rate(),
            })
        });
    let (providers, providers_cut) =
        group_rows(providers, MAX_HISTORY_PROVIDER_ROWS, |provider, c| {
            let mut value = serde_json::json!({
                "provider": provider,
                "requests": c.requests,
                "successes": c.successes,
                "success_rate": c.success_rate(),
                "average_latency_ms": c.average_latency_ms(),
                "input_tokens": c.input_tokens,
                "output_tokens": c.output_tokens,
            });
            for (name, field) in c.cost_fields() {
                value[name] = field;
            }
            value
        });
    let (statuses, statuses_cut) = group_rows(statuses, MAX_HISTORY_STATUS_ROWS, |status, c| {
        serde_json::json!({
            "status": status,
            "requests": c.requests,
            "successes": c.successes,
            "average_latency_ms": c.average_latency_ms(),
        })
    });
    let series_rows = buckets
        .iter()
        .enumerate()
        .map(|(index, counts)| {
            let offset = series
                .bucket_seconds
                .saturating_mul((series.bucket_count - index) as i64);
            let start = series.end_seconds.saturating_sub(offset).max(0);
            let mut value = serde_json::json!({
                "start": db::format_utc_timestamp(start as u64),
                "requests": counts.requests,
                "successes": counts.successes,
                "failures": counts.requests.saturating_sub(counts.successes),
                "input_tokens": counts.input_tokens,
                "output_tokens": counts.output_tokens,
                "average_latency_ms": counts.average_latency_ms(),
            });
            for (name, field) in counts.cost_fields() {
                value[name] = field;
            }
            value
        })
        .collect::<Vec<_>>();

    let mut cost = serde_json::json!({ "requests": totals.requests });
    for (name, field) in totals.cost_fields() {
        cost[name] = field;
    }

    Ok(HistoryScan {
        models,
        models_truncated: models_cut || scan_truncated,
        providers,
        providers_truncated: providers_cut || scan_truncated,
        statuses,
        statuses_truncated: statuses_cut || scan_truncated,
        series: series_rows,
        latency: latency_summary(&latency_buckets, &totals),
        cost,
        logged_requests,
        scan_truncated,
    })
}

/// Reads the fields every view needs, rejecting negative counters instead of
/// folding them into the totals.
fn decode_row(record: &Record) -> Result<LogRow, StorageError> {
    let input_tokens = non_negative_log_value(record, "input_tokens")?;
    let cache_input_tokens =
        non_negative_log_value(record, "cache_input_tokens")?.max(input_tokens);
    let status = record.integer("status")?;
    Ok(LogRow {
        model: record.text("model")?.to_owned(),
        // The split is by the provider that actually served the request, not by
        // the combo/route alias the client called. A request that failed before
        // a provider was selected has no provider and is grouped on its own.
        provider: record
            .optional_text("provider_id")?
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        status,
        succeeded: (200..300).contains(&status),
        duration_ms: non_negative_log_value(record, "duration_ms")?,
        input_tokens,
        output_tokens: non_negative_log_value(record, "output_tokens")?,
        cached_tokens: non_negative_log_value(record, "cached_tokens")?.min(cache_input_tokens),
        cache_input_tokens,
        cost_micro_usd: record
            .optional_integer("cost_micro_usd")?
            .filter(|value| *value >= 0),
    })
}

/// Ranks groups by request count and keeps the head, reporting whether anything
/// was dropped.
fn group_rows<K: Ord, F: FnMut(K, &GroupCounts) -> serde_json::Value>(
    groups: BTreeMap<K, GroupCounts>,
    limit: usize,
    mut build: F,
) -> (Vec<serde_json::Value>, bool) {
    let mut ranked = groups
        .into_iter()
        .map(|(key, counts)| (counts.requests, key, counts))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let truncated = ranked.len() > limit;
    ranked.truncate(limit);
    (
        ranked
            .into_iter()
            .map(|(_, key, counts)| build(key, &counts))
            .collect(),
        truncated,
    )
}

fn latency_bucket(duration_ms: i64) -> usize {
    LATENCY_BUCKET_BOUNDS_MS
        .iter()
        .position(|bound| duration_ms <= *bound)
        .unwrap_or(LATENCY_BUCKET_BOUNDS_MS.len())
}

/// Percentile estimate from the fixed histogram. The reported value is the
/// bucket's upper bound, so it can only overstate; the open tail reports its
/// lower bound and carries its own `*_is_floor` flag so the caller renders "or
/// more" on that value alone instead of on every percentile in the range.
fn latency_summary(buckets: &[i64], totals: &GroupCounts) -> serde_json::Value {
    let samples = buckets.iter().copied().sum::<i64>();
    let open_floor = LATENCY_BUCKET_BOUNDS_MS[LATENCY_BUCKET_BOUNDS_MS.len() - 1];
    // The rank the percentile has to reach, counting from the fastest sample.
    let rank = |percent: f64| (percent / 100.0 * samples as f64).ceil().max(1.0);
    let percentile = |percent: f64| -> Option<f64> {
        if samples <= 0 {
            return None;
        }
        let target = rank(percent);
        let mut cumulative = 0_i64;
        for (index, count) in buckets.iter().enumerate() {
            cumulative = cumulative.saturating_add(*count);
            if cumulative as f64 >= target {
                return Some(
                    LATENCY_BUCKET_BOUNDS_MS
                        .get(index)
                        .copied()
                        .unwrap_or(open_floor) as f64,
                );
            }
        }
        None
    };
    // A percentile is a floor only when its own sample falls in the open bucket.
    // Marking every percentile a floor because one slow request exists would
    // turn a measured median into "or more".
    let is_floor = |percent: f64| -> bool {
        if samples <= 0 {
            return false;
        }
        let bounded = buckets
            .iter()
            .take(LATENCY_BUCKET_BOUNDS_MS.len())
            .copied()
            .sum::<i64>();
        (bounded as f64) < rank(percent)
    };
    serde_json::json!({
        "average_ms": totals.average_latency_ms(),
        "p50_ms": percentile(50.0),
        "p50_is_floor": is_floor(50.0),
        "p95_ms": percentile(95.0),
        "p95_is_floor": is_floor(95.0),
        "p99_ms": percentile(99.0),
        "p99_is_floor": is_floor(99.0),
        // True when the slowest bucket holds samples, so a reported percentile
        // that lands there is a floor rather than a measurement.
        "open_ended": buckets.last().is_some_and(|count| *count > 0),
        "bucket_bounds_ms": LATENCY_BUCKET_BOUNDS_MS,
        "buckets": buckets,
        "samples": samples,
    })
}

fn non_negative_log_value(record: &Record, name: &str) -> Result<i64, StorageError> {
    match record.optional_integer(name)? {
        Some(value) if value >= 0 => Ok(value),
        Some(_) => Err(StorageError::Invalid(format!(
            "request log field '{name}' is negative"
        ))),
        None => Ok(0),
    }
}

fn request_log_created_at(index_key: &str) -> Result<i64, StorageError> {
    let malformed = || StorageError::Invalid("request log time index is malformed".to_owned());
    let created_at = index_key
        .strip_prefix(db::REQUEST_LOG_INDEX_PREFIX)
        .and_then(|suffix| suffix.split_once('/'))
        .map(|(created_at, _)| created_at)
        .ok_or_else(malformed)?;
    db::parse_utc_timestamp(created_at)
        .map(|seconds| i64::try_from(seconds).unwrap_or(i64::MAX))
        .ok_or_else(malformed)
}

/// Bucket index for one timestamp, or `None` when it falls before the series.
fn series_index(seconds: i64, spec: SeriesSpec) -> Option<usize> {
    if spec.bucket_count == 0 || spec.bucket_seconds <= 0 {
        return None;
    }
    if seconds >= spec.end_seconds {
        return Some(spec.bucket_count - 1);
    }
    // `max(1)` keeps a row written in the current second inside the last bucket
    // instead of one slot past the end of the series.
    let age = (spec.end_seconds - seconds).max(1);
    let back = age.saturating_add(spec.bucket_seconds - 1) / spec.bucket_seconds;
    let index = spec.bucket_count as i64 - back;
    (index >= 0).then_some(index as usize)
}
