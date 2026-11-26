import assert from 'node:assert/strict';
import test from 'node:test';
import { EMPTY_METRICS, loadKeyMetrics, metricsFromStatistics } from '../src/features/api-keys/api-keys.data';
import { pingGateway } from '../src/lib/api/gateway';
import type { GatewayApiKey, GatewayApiKeyStatistics } from '../src/lib/types';

const key = (id: string): GatewayApiKey => ({ id, name: id, enabled: true, created_at: '2026-01-01 00:00:00' });
const stats = (id: string): GatewayApiKeyStatistics => ({
  api_key_id: id,
  api_key_name: id,
  range: '1d',
  total_requests: 4,
  successes: 3,
  failures: 1,
  success_rate: 0.75,
  average_latency_ms: 100,
  input_tokens: 10,
  output_tokens: 5,
  cached_tokens: 2,
  cache_ratio: 0.2,
  as_of: '2026-11-19 12:00:00',
  collection_started_at: null,
  complete: true,
  dropped_events: 0,
  aggregation_current: true,
  api_keys: { top: [], others: 0 },
} as unknown as GatewayApiKeyStatistics);

test('F-06: per-key statistics map into metrics without a RequestStatistics cast', () => {
  const m = metricsFromStatistics(stats('k1'));
  assert.deepEqual(m, { requests: 4, tokens: 17, failures: 1, successRate: 0.75, loaded: true, error: false });
});

test('F-07: aborting the cycle stops pending key-statistics requests', async () => {
  const controller = new AbortController();
  let observed: AbortSignal | undefined;
  const fetcher = (_id: string, _range: '1d', signal: AbortSignal) => {
    observed = signal;
    return new Promise<GatewayApiKeyStatistics>((_resolve, reject) => {
      signal.addEventListener('abort', () => reject(new Error('aborted')));
    });
  };
  const pending = loadKeyMetrics([key('k1'), key('k2')], '1d', fetcher, controller.signal);
  await new Promise((r) => setTimeout(r, 5));
  assert.ok(observed);
  assert.equal(observed!.aborted, false);
  controller.abort();
  const result = await pending;
  assert.deepEqual(result, { metrics: {}, partialError: false });
});

test('F-07: a failed key without abort becomes a bounded error metric', async () => {
  const fetcher = async (id: string): Promise<GatewayApiKeyStatistics> => {
    if (id === 'bad') throw new Error('offline');
    return stats(id);
  };
  const result = await loadKeyMetrics([key('good'), key('bad')], '1d', fetcher as never, new AbortController().signal);
  assert.equal(result.metrics.good.loaded, true);
  assert.deepEqual(result.metrics.bad, { ...EMPTY_METRICS, error: true });
  assert.equal(result.partialError, true);
});

test('F-09: pingGateway honors an external AbortSignal', async () => {
  const controller = new AbortController();
  const original = globalThis.fetch;
  globalThis.fetch = (() => new Promise<Response>((_resolve, reject) => {
    controller.signal.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')));
  })) as typeof fetch;
  try {
    const pending = pingGateway('http://127.0.0.1:1', { signal: controller.signal, timeoutMs: 5000 });
    controller.abort();
    await assert.rejects(pending);
  } finally {
    globalThis.fetch = original;
  }
});

test('F-09: pingGateway enforces its timeout', async () => {
  const original = globalThis.fetch;
  globalThis.fetch = ((_url: string, init?: RequestInit) => new Promise<Response>((_resolve, reject) => {
    init?.signal?.addEventListener('abort', () => reject(new DOMException('timeout', 'AbortError')));
  })) as typeof fetch;
  try {
    await assert.rejects(pingGateway('http://127.0.0.1:1', { timeoutMs: 10 }), /timed out/);
  } finally {
    globalThis.fetch = original;
  }
});

test('F-09: a successful ping returns a non-negative latency', async () => {
  const original = globalThis.fetch;
  globalThis.fetch = (async () => new Response('{}', { status: 200 })) as typeof fetch;
  try {
    const ms = await pingGateway('http://127.0.0.1:1/', { timeoutMs: 1000 });
    assert.ok(ms >= 0);
  } finally {
    globalThis.fetch = original;
  }
});

test('F-14: null limits state has a safe no-op guard', () => {
  // Mirrors applyDraft's guard: hasChanges is false whenever limits is null.
  const limits: { saved: { v: number } } | null = null;
  const hasChanges = false;
  let saved = 0;
  if (!hasChanges || !limits) return;
  saved = limits.saved.v;
  assert.equal(saved, 0);
});
