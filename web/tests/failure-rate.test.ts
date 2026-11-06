/**
 * Failure-rate alert thresholds and dismissal (src/lib/failure-rate.ts).
 *
 * The banner is deliberate-noise only for a confirmed incident: healthy
 * rates, small samples, dismissed windows and failed fetches must all stay
 * silent. These tests pin the threshold predicate and the dismissal
 * contract; checkFailureRate is exercised with a stubbed statistics source.
 */
import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import {
  checkFailureRate,
  dismissFailureAlert,
  shouldAlertFailureRate,
} from '../src/lib/failure-rate';
import type { RequestStatistics } from '../src/lib/types';

const STORAGE_KEY = 'exoroute.failure-alert-dismissed';

function installStorage(): Map<string, string> {
  const storage = new Map<string, string>();
  Object.defineProperty(globalThis, 'window', {
    value: {},
    configurable: true,
  });
  Object.defineProperty(globalThis, 'localStorage', {
    value: {
      getItem: (key: string) => (storage.has(key) ? storage.get(key)! : null),
      setItem: (key: string, value: string) => { storage.set(key, value); },
      removeItem: (key: string) => { storage.delete(key); },
      clear: () => storage.clear(),
      key: () => null,
      length: 0,
    } as Storage,
    configurable: true,
    writable: true,
  });
  return storage;
}

function statistics(overrides: Partial<RequestStatistics> = {}): RequestStatistics {
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
    as_of: '2026-11-06 12:00:00',
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

test('shouldAlertFailureRate fires only below the floor with enough volume', () => {
  assert.equal(shouldAlertFailureRate({ success_rate: 0.95, total_requests: 100 }), false, 'healthy');
  assert.equal(shouldAlertFailureRate({ success_rate: 0.9, total_requests: 100 }), false, 'exactly at the floor');
  assert.equal(shouldAlertFailureRate({ success_rate: 0.89, total_requests: 100 }), true, 'below floor');
  assert.equal(shouldAlertFailureRate({ success_rate: 0.5, total_requests: 19 }), false, 'too little volume');
  assert.equal(shouldAlertFailureRate({ success_rate: 0.5, total_requests: 20 }), true, 'minimum volume');
  assert.equal(shouldAlertFailureRate({ success_rate: Number.NaN, total_requests: 100 }), false, 'NaN never alerts');
  assert.equal(shouldAlertFailureRate({ success_rate: 0.5, total_requests: Number.POSITIVE_INFINITY }), false, 'non-finite volume never alerts');
});

test('checkFailureRate returns the incident for a bad rate and null for a healthy one', async () => {
  installStorage();
  const bad = await checkFailureRate({
    stats: async () => statistics({ success_rate: 0.62, failures: 38, total_requests: 100 }),
  });
  assert.deepEqual(bad, {
    incidentId: '2026-11-06 12:00:00',
    successRate: 0.62,
    failures: 38,
    total: 100,
  });

  const healthy = await checkFailureRate({ stats: async () => statistics({ success_rate: 0.99 }) });
  assert.equal(healthy, null);
});

test('checkFailureRate stays silent when the statistics fetch fails', async () => {
  installStorage();
  const failed = await checkFailureRate({
    stats: async () => { throw new Error('backend down'); },
  });
  assert.equal(failed, null, 'failures never surface the banner');
});

test('a dismissed incident stays dismissed; a new snapshot re-notifies', async () => {
  const storage = installStorage();
  const bad = () => statistics({ success_rate: 0.62, failures: 38, total_requests: 100 });
  dismissFailureAlert('2026-11-06 12:00:00');

  const suppressed = await checkFailureRate({ stats: async () => bad() });
  assert.equal(suppressed, null, 'the dismissed window stays silent');

  const fresh = await checkFailureRate({
    stats: async () => statistics({ success_rate: 0.62, failures: 38, total_requests: 100, as_of: '2026-11-06 13:00:00' }),
  });
  assert.equal(fresh?.incidentId, '2026-11-06 13:00:00', 'a newer snapshot re-notifies');

  assert.equal(
    JSON.parse(storage.get(STORAGE_KEY)!) as unknown,
    '2026-11-06 12:00:00',
  );
});
