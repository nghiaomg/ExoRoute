import type { RequestStatistics } from '../../../src/lib/types';

/**
 * A complete, healthy statistics snapshot for components that render the
 * traffic summary. Individual tests override the fields they assert on;
 * unspecified fields mirror the neutral values used by the node-suite
 * statistics fixtures.
 */
export function statisticsFixture(overrides: Partial<RequestStatistics> = {}): RequestStatistics {
  return {
    range: '1d',
    total_requests: 100,
    successes: 95,
    failures: 5,
    success_rate: 0.95,
    average_latency_ms: 300,
    input_tokens: 0,
    output_tokens: 0,
    cached_tokens: 0,
    cache_ratio: 0,
    as_of: '2026-11-07 12:00:00',
    collection_started_at: null,
    complete: true,
    dropped_events: 0,
    aggregation_current: true,
    api_keys: { top: [], others: 0 },
    models: { top: [], others: 0 },
    model_breakdown: [],
    model_breakdown_truncated: false,
    logged_requests: 100,
    model_breakdown_incomplete: false,
    providers: { rows: [], truncated: false },
    statuses: { rows: [], truncated: false },
    series: { points: [], granularity: 'hour' },
    latency: { p50: 0, p95: 0, p99: 0, max: 0, samples: 0 },
    cost: { total: 0, currency: 'usd' } as unknown as RequestStatistics['cost'],
    history_truncated: false,
    average_time_to_first_token_ms: null,
    time_to_first_token_samples: 0,
    ...overrides,
  };
}
