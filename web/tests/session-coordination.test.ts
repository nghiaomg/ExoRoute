import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import {
  applyRequestLiveEvent,
  createRequestLiveState,
  mergeFinishedRequests,
} from '../src/features/requests/request.state';
import { mergeProviderUsage } from '../src/features/quota/quota.state';
import { api } from '../src/lib/api';
import type { ProviderKey, ProviderUsageAccount, RequestLiveEvent, RequestLiveRow, RequestLog } from '../src/lib/types';

export function parseRetryAfterSeconds(value: string | null): number {
  if (!value) return 0;
  const numericValue = Number(value);
  if (Number.isFinite(numericValue) && numericValue >= 0) {
    return Math.min(3600, Math.ceil(numericValue));
  }
  const retryAt = Date.parse(value);
  if (!Number.isFinite(retryAt)) return 0;
  return Math.min(3600, Math.max(0, Math.ceil((retryAt - Date.now()) / 1000)));
}

export function clampAccessTtlSeconds(value: number): number {
  return Math.max(1, Math.min(600, value));
}

test('session retry timer honors bounded Retry-After semantics', () => {
  assert.equal(parseRetryAfterSeconds(null), 0);
  assert.equal(parseRetryAfterSeconds('5'), 5);
  assert.equal(parseRetryAfterSeconds('5.2'), 6);
  assert.equal(parseRetryAfterSeconds('99999'), 3600);
  assert.equal(parseRetryAfterSeconds('-1'), 0);
  assert.equal(parseRetryAfterSeconds('not-a-date'), 0);
});

test('access token expiry is clamped to the documented 10-minute window', () => {
  assert.equal(clampAccessTtlSeconds(600), 600);
  assert.equal(clampAccessTtlSeconds(10_000), 600);
  assert.equal(clampAccessTtlSeconds(0), 1);
  assert.equal(clampAccessTtlSeconds(-30), 1);
});

test('legacy admin tokens are only cleared, never read for authentication', () => {
  const storageKeys = ['exoroute.admin-session', 'exoroute.admin-key'];
  assert.deepEqual(storageKeys, ['exoroute.admin-session', 'exoroute.admin-key']);
});

test('request live SSE ignores malformed events and parses lifecycle events without REST refresh', async () => {
  const originalFetch = globalThis.fetch;
  const controller = new AbortController();
  const events: RequestLiveEvent[] = [];
  let fetchCalls = 0;
  const payload = [
    'event: request-snapshot\ndata: {"requests":[],"truncated":false,"limit":64,"active_count":0}\n\n',
    'event: request-started\ndata: {"live":true,"live_id":"live-a","request_id":"client-a","route_alias":"route","model":"model","started_at_ms":1700000000000,"api_key_id":"key-a","active_count":1}\n\n',
    'event: request-started\ndata: {"live":true,"live_id":"live-b","started_at_ms":"invalid","model":"model"}\n\n',
    'event: request-updated\ndata: {"live_id":"live-a","provider_id":"provider-a","latest_log_id":"log-a"}\n\n',
    'event: request-finished\ndata: {"live_id":"live-a","finished_at_ms":1700000001000,"request":{"id":"log-a","request_id":"client-a","model":"model","status":200,"created_at":null},"active_count":0}\n\n',
  ].join('');
  globalThis.fetch = async () => {
    fetchCalls += 1;
    return new Response(payload, { headers: { 'content-type': 'text/event-stream' } });
  };
  try {
    await api.streamRequestLive((event) => {
      events.push(event);
      if (event.type === 'finished') controller.abort();
    }, controller.signal);
  } finally {
    globalThis.fetch = originalFetch;
  }

  assert.deepEqual(events.map((event) => event.type), ['snapshot', 'started', 'updated', 'finished']);
  assert.equal(fetchCalls, 1);
  assert.equal(events[0].type === 'snapshot' ? events[0].active_count : -1, 0);
  assert.equal(events[1].type === 'started' ? events[1].active_count : -1, 1);
  assert.equal(events[1].type === 'started' ? events[1].request.live_id : '', 'live-a');
  assert.equal(events[2].type === 'updated' ? events[2].latest_log_id : '', 'log-a');
  assert.equal(events[3].type === 'finished' ? events[3].active_count : -1, 0);
  assert.equal(events[3].type === 'finished' ? events[3].request?.status : 0, 200);
});

test('request live state replaces stale snapshots and applies provider updates', () => {
  const filters = { api_key_id: '', model: '', provider_id: '', status: '' as const };
  const row = (liveId: string): RequestLiveRow => ({
    live: true,
    live_id: liveId,
    request_id: `request-${liveId}`,
    route_alias: 'default',
    provider_id: undefined,
    provider_credential_id: undefined,
    api_key_id: 'key-a',
    model: 'model-a',
    client_protocol: 'chat_completions',
    upstream_protocol: undefined,
    started_at_ms: 1_700_000_000_000,
    created_at: new Date(1_700_000_000_000).toISOString(),
  });

  let state = createRequestLiveState();
  state = applyRequestLiveEvent(state, { type: 'started', request: row('live-a') }, filters).state;
  state = applyRequestLiveEvent(state, {
    type: 'updated',
    live_id: 'live-a',
    provider_id: 'provider-a',
    latest_log_id: 'log-a',
  }, filters).state;
  assert.equal(state.requests.get('live-a')?.provider_id, 'provider-a');
  assert.equal(state.requests.get('live-a')?.id, 'log-a');

  state = applyRequestLiveEvent(state, {
    type: 'snapshot',
    requests: [row('live-b')],
    truncated: true,
    limit: 64,
    active_count: 65,
  }, filters).state;
  assert.deepEqual([...state.requests.keys()], ['live-b']);
  assert.equal(state.truncated, true);
  assert.equal(state.activeCount, 65);
});

test('request live finish is de-duplicated when REST catches up', () => {
  const filters = { api_key_id: '', model: '', provider_id: '', status: '' as const };
  const finished: RequestLog = {
    id: 'log-a',
    request_id: 'request-a',
    model: 'model-a',
    status: 200,
    created_at: new Date(1_700_000_000_100).toISOString(),
  };
  const started: RequestLiveRow = {
    ...finished,
    id: undefined,
    created_at: new Date(1_700_000_000_000).toISOString(),
    live: true,
    live_id: 'live-a',
    request_id: 'request-a',
    route_alias: 'default',
    started_at_ms: 1_700_000_000_000,
    client_protocol: 'chat_completions',
  };
  let state = applyRequestLiveEvent(createRequestLiveState(), { type: 'started', request: started }, filters).state;
  const result = applyRequestLiveEvent(state, {
    type: 'finished',
    live_id: 'live-a',
    request: finished,
    finished_at_ms: 1_700_000_000_100,
  }, filters);
  state = result.state;
  assert.equal(result.finished?.id, 'log-a');
  assert.equal(state.requests.size, 0);

  const merged = mergeFinishedRequests([finished], null, state.pendingFinished, filters);
  assert.deepEqual(merged.requests.map((request) => request.id), ['log-a']);
  assert.equal(merged.pendingFinished.size, 0);
});

test('quota usage merge keeps real keys and adds each synthetic account once', () => {
  const existingKey: ProviderKey = {
    id: 'key-a',
    name: 'Primary',
    enabled: true,
    invalid: false,
    created_at: '2026-01-01T00:00:00.000Z',
  };
  const accounts: ProviderUsageAccount[] = [
    { key_id: 'key-a', name: 'Primary account', status: 'fresh' },
    { key_id: '', name: '', status: 'reauth_required' },
    { key_id: '', name: 'OAuth account', status: 'disabled' },
  ];
  const merged = mergeProviderUsage([existingKey], accounts, (id) => `Account ${id}`, '2026-02-01T00:00:00.000Z');

  assert.deepEqual(merged.keys.map((key) => key.id), ['key-a', 'account_1', 'account_2']);
  assert.equal(merged.keys[1].credential_type, 'oauth');
  assert.equal(merged.keys[1].invalid, true);
  assert.equal(merged.keys[2].enabled, false);
  assert.equal(merged.usageByKey.account_2?.name, 'OAuth account');
});
