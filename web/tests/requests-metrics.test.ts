import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import {
  appliedRequestFilters,
  clearedRequestFilter,
  durationBarPercent,
  durationTone,
  modelAccent,
  modelMonogram,
  requestOutcome,
  requestOutcomeCounts,
  requestPageMetrics,
  requestRowKey,
  sameRequestFilters,
  tokenRatio,
  SLOW_DURATION_MS,
  VERY_SLOW_DURATION_MS,
} from '../src/features/requests/request.metrics';
import type { RequestLog, RequestLiveRow } from '../src/lib/types';
import type { Translate } from '../src/lib/format';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

function finished(overrides: Partial<RequestLog> = {}): RequestLog {
  return {
    id: 'log-1',
    model: 'ocg/minimax-m3',
    created_at: '2026-10-05T10:00:00.000Z',
    status: 200,
    duration_ms: 120,
    input_tokens: 100,
    output_tokens: 50,
    ...overrides,
  };
}

function liveRow(overrides: Partial<RequestLiveRow> = {}): RequestLiveRow {
  return {
    live: true,
    live_id: 'live-1',
    started_at_ms: 1_000,
    model: 'ocg/minimax-m3',
    created_at: '2026-10-05T10:00:00.000Z',
    ...overrides,
  };
}

test('an in-flight row is active, 2xx is success, and anything else is a failure', () => {
  assert.equal(requestOutcome(liveRow()), 'active');
  assert.equal(requestOutcome(finished({ status: 200 })), 'success');
  assert.equal(requestOutcome(finished({ status: 299 })), 'success');
  assert.equal(requestOutcome(finished({ status: 302 })), 'failure');
  assert.equal(requestOutcome(finished({ status: 404 })), 'failure');
  assert.equal(requestOutcome(finished({ status: 500 })), 'failure');
  // A finished row without a recorded status cannot be reported as a success.
  assert.equal(requestOutcome(finished({ status: undefined })), 'failure');
});

test('page metrics count outcomes and derive the success rate from finished rows', () => {
  const rows = [
    finished({ id: 'a', status: 200, duration_ms: 100 }),
    finished({ id: 'b', status: 500, duration_ms: 50 }),
    liveRow({ live_id: 'c' }),
  ];
  const counts = requestOutcomeCounts(rows);
  assert.deepEqual(counts, { active: 1, success: 1, failure: 1 });

  const metrics = requestPageMetrics(rows, 2_000);
  assert.equal(metrics.total, 3);
  assert.equal(metrics.successRate, 50);
  // Only finished rows carry a measured duration; the live row reports elapsed
  // time instead, so the percentile stays over [50, 100].
  assert.equal(metrics.p95DurationMs, 100);
  assert.equal(metrics.slowestDurationMs, 100);
});

test('the success rate is unknown while nothing has finished', () => {
  const metrics = requestPageMetrics([liveRow()], 3_000);
  assert.equal(metrics.successRate, null);
  assert.equal(metrics.p95DurationMs, null);
  assert.equal(metrics.slowestDurationMs, null);
});

test('the 95th percentile uses the nearest rank of the sorted samples', () => {
  const rows = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100].map((duration, index) =>
    finished({ id: `row-${index}`, duration_ms: duration }),
  );
  assert.equal(requestPageMetrics(rows, 0).p95DurationMs, 100);
});

test('duration tiers mark slow and very slow rows', () => {
  assert.equal(durationTone(null), 'fast');
  assert.equal(durationTone(SLOW_DURATION_MS - 1), 'fast');
  assert.equal(durationTone(SLOW_DURATION_MS), 'slow');
  assert.equal(durationTone(VERY_SLOW_DURATION_MS - 1), 'slow');
  assert.equal(durationTone(VERY_SLOW_DURATION_MS), 'verySlow');
});

test('duration bars stay visible for instant rows and are capped at the slowest row', () => {
  assert.equal(durationBarPercent(null, 100), 0);
  assert.equal(durationBarPercent(100, null), 0);
  assert.equal(durationBarPercent(1, 100), 6);
  assert.equal(durationBarPercent(50, 100), 50);
  assert.equal(durationBarPercent(100, 100), 100);
  // A row slower than the scaling value never overflows its track.
  assert.equal(durationBarPercent(400, 100), 100);
});

test('token ratios split recorded usage and stay unknown without any', () => {
  assert.deepEqual(tokenRatio(75, 25), { input: 75, output: 25 });
  assert.deepEqual(tokenRatio(0, 10), { input: 0, output: 100 });
  assert.deepEqual(tokenRatio(1, 2), { input: 33, output: 67 });
  assert.equal(tokenRatio(0, 0), null);
  assert.equal(tokenRatio(undefined, undefined), null);
  assert.equal(tokenRatio(Number.NaN, undefined), null);
  assert.equal(tokenRatio(-5, null), null);
});

test('row keys prefer the stable identifiers and fall back to the timestamp', () => {
  assert.equal(requestRowKey(liveRow({ live_id: 'live-9' })), 'live-9');
  assert.equal(requestRowKey(finished({ id: 'log-9' })), 'log-9');
  assert.equal(requestRowKey(finished({ id: undefined, request_id: 'req-9' })), 'req-9');
  assert.equal(
    requestRowKey(finished({ id: undefined, request_id: undefined, created_at: '2026-10-05T10:00:00.000Z' })),
    '2026-10-05T10:00:00.000Z',
  );
});

test('model monograms read from the model segment and survive empty ids', () => {
  assert.equal(modelMonogram('ocg/minimax-m3'), 'MI');
  assert.equal(modelMonogram('gpt-4o'), 'GP');
  assert.equal(modelMonogram('x'), 'X');
  assert.equal(modelMonogram('---'), '—');
});

test('accent buckets are stable per id and stay inside the palette', () => {
  for (const id of ['provider-a', 'provider-b', 'ocg/minimax-m3', '']) {
    const bucket = modelAccent(id);
    assert.ok(bucket >= 0 && bucket < 6, `${id} produced ${bucket}`);
    assert.equal(modelAccent(id), bucket);
  }
  assert.equal(modelAccent('provider-a', 3), modelAccent('provider-a') % 3);
});

test('applied filters describe only the filters that are set, in form order', () => {
  assert.deepEqual(appliedRequestFilters({}, tr), []);

  const applied = appliedRequestFilters(
    { api_key_id: 'unknown', model: ' minimax ', provider_id: 'p1', status: 'failure' },
    tr,
  );
  assert.deepEqual(applied, [
    { key: 'api_key_id', label: 'API key ID', value: 'Unknown key' },
    { key: 'model', label: 'Requested model', value: 'minimax' },
    { key: 'provider_id', label: 'Provider ID', value: 'p1' },
    { key: 'status', label: 'Outcome', value: 'Failed' },
  ]);

  assert.deepEqual(appliedRequestFilters({ status: 'success' }, tr), [
    { key: 'status', label: 'Outcome', value: 'Successful' },
  ]);
});

test('removing one filter clears exactly that key', () => {
  const filters = { api_key_id: 'k1', model: 'm1', provider_id: 'p1', status: 'success' as const };
  assert.deepEqual(clearedRequestFilter(filters, 'model'), { ...filters, model: '' });
  assert.deepEqual(clearedRequestFilter(filters, 'status'), { ...filters, status: '' });
});

test('the filter panel compares drafts ignoring surrounding whitespace', () => {
  assert.equal(sameRequestFilters({ model: 'm1' }, { model: ' m1 ' }), true);
  assert.equal(sameRequestFilters({ model: 'm1' }, { model: 'm2' }), false);
  assert.equal(sameRequestFilters({ status: 'success' }, {}), false);
  assert.equal(sameRequestFilters({}, {}), true);
});
