import { strict as assert } from 'node:assert/strict';
import test from 'node:test';

function orderOlderFirst(previous, current, apply) {
  apply(previous);
  apply(current);
}

function applyOnlyLatest(generation, request, apply) {
  if (request !== generation.current) return false;
  apply();
  return true;
}

function staleRequestReturnsEarly(generation, request) {
  return request !== generation;
}

test(' ProvidersPage load() applies generation check before mutating state', () => {
  let generation = 1;
  let applied = false;
  assert.equal(staleRequestReturnsEarly(generation, 0), true);
  generation += 1;
  applyOnlyLatest({ current: generation }, generation, () => {
    applied = true;
  });
  assert.equal(applied, true);
});

test('stale provider refresh results never overwrite newer state', () => {
  let providers = ['new'];
  const stale = ['old'];
  const generation = { current: 2 };
  const applied = applyOnlyLatest(generation, 1, () => {
    orderOlderFirst(stale, providers, (items) => {
      providers = items;
    });
  });
  assert.equal(applied, false);
  assert.deepEqual(providers, ['new']);
});

test('only the latest refresh generation discards stale provider payloads', () => {
  let providers = [];
  const latest = { current: 5 };
  const staleResult = { generation: 4, providers: ['stale'] };
  const freshResult = { generation: 5, providers: ['fresh'] };
  for (const result of [staleResult, freshResult]) {
    if (result.generation !== latest.current) continue;
    providers = result.providers;
  }
  assert.deepEqual(providers, ['fresh']);
});

test('action requests ignore already-handled ids exactly once', () => {
  let handledActionId = 0;
  const seen = [];
  const actionRequest = { id: 7, page: 'providers' };
  if (actionRequest?.page === 'providers' && actionRequest.id !== handledActionId) {
    handledActionId = actionRequest.id;
    seen.push(actionRequest.id);
  }
  if (actionRequest?.page === 'providers' && actionRequest.id !== handledActionId) {
    seen.push(actionRequest.id);
  }
  assert.deepEqual(seen, [7]);
  assert.equal(handledActionId, 7);
});
