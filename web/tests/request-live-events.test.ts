import { strict as assert } from 'node:assert/strict';
import test from 'node:test';

import { parseRequestLiveEvent } from '../src/lib/requestParsers';
import { applyRequestLiveEvent, createRequestLiveState } from '../src/features/requests/request.state';
import type { RequestLiveRow, RequestLogFilters } from '../src/lib/types';

test('parseRequestLiveEvent preserves cached_tokens on request-finished event', () => {
  const payload = JSON.stringify({
    live_id: 'live-123',
    finished_at_ms: 1_700_000_000_000,
    active_count: 0,
    request: {
      id: 'log-123',
      request_id: 'req-123',
      route_alias: 'default',
      provider_id: 'openai',
      model: 'gpt-4o',
      status: 200,
      duration_ms: 2500,
      input_tokens: 385925,
      output_tokens: 323,
      cached_tokens: 385792,
      cache_input_tokens: 385792,
      created_at: '2026-10-30T11:26:00.000Z',
    },
  });

  const event = parseRequestLiveEvent('request-finished', payload);
  assert.ok(event && event.type === 'finished');
  assert.equal(event.live_id, 'live-123');
  assert.ok(event.request);
  assert.equal(event.request.input_tokens, 385925);
  assert.equal(event.request.output_tokens, 323);
  assert.equal(event.request.cached_tokens, 385792);
  assert.equal(event.request.cache_input_tokens, 385792);
});

test('parseRequestLiveEvent preserves cached_tokens on request-updated event', () => {
  const payload = JSON.stringify({
    live_id: 'live-456',
    provider_id: 'openai',
    latest_log_id: 'log-456',
    input_tokens: 1000,
    output_tokens: 50,
    cached_tokens: 800,
    cache_input_tokens: 800,
  });

  const event = parseRequestLiveEvent('request-updated', payload);
  assert.ok(event && event.type === 'updated');
  assert.equal(event.live_id, 'live-456');
  assert.equal(event.input_tokens, 1000);
  assert.equal(event.output_tokens, 50);
  assert.equal(event.cached_tokens, 800);
  assert.equal(event.cache_input_tokens, 800);
});

test('applyRequestLiveEvent updates cached_tokens on live row', () => {
  const filters: RequestLogFilters = { api_key_id: '', model: '', provider_id: '', status: '' };
  const initialRow: RequestLiveRow = {
    live: true,
    live_id: 'live-789',
    request_id: 'req-789',
    model: 'gpt-4o',
    created_at: new Date(1_700_000_000_000).toISOString(),
    started_at_ms: 1_700_000_000_000,
  };

  let state = applyRequestLiveEvent(createRequestLiveState(), { type: 'started', request: initialRow }, filters).state;
  assert.equal(state.requests.get('live-789')?.cached_tokens, undefined);

  state = applyRequestLiveEvent(state, {
    type: 'updated',
    live_id: 'live-789',
    provider_id: 'openai',
    latest_log_id: 'log-789',
    input_tokens: 5000,
    output_tokens: 200,
    cached_tokens: 4000,
    cache_input_tokens: 4000,
  }, filters).state;

  const updated = state.requests.get('live-789');
  assert.ok(updated);
  assert.equal(updated.input_tokens, 5000);
  assert.equal(updated.output_tokens, 200);
  assert.equal(updated.cached_tokens, 4000);
  assert.equal(updated.cache_input_tokens, 4000);
});
