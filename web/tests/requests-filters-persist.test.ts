/**
 * Persistence for the Requests page filter set (request.filters.persist.ts).
 *
 * Only the four public filter fields round-trip through storage; the tests
 * pin the validation (status allowlist, length caps, corrupt JSON fall-back)
 * and that nothing else — keys, credentials, extra fields — is ever stored.
 */
import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import {
  loadSavedRequestFilters,
  saveSavedRequestFilters,
} from '../src/features/requests/request.filters.persist';
import type { RequestLogFilters } from '../src/lib/types';

const STORAGE_KEY = 'exoroute:requests:active_filters';
const EMPTY: RequestLogFilters = { api_key_id: '', model: '', provider_id: '', status: '' };

function withStorage<T>(fn: (store: Map<string, string>) => T): T {
  const store = new Map<string, string>();
  const originalWindow = globalThis.window;
  const originalLocalStorage = globalThis.localStorage;
  try {
    globalThis.localStorage = {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => { store.set(key, value); },
      removeItem: (key: string) => { store.delete(key); },
      clear: () => store.clear(),
      key: () => null,
      length: 0,
    } as Storage;
    (globalThis as { window: unknown }).window = {};
    return fn(store);
  } finally {
    globalThis.window = originalWindow;
    globalThis.localStorage = originalLocalStorage;
  }
}

test('loadSavedRequestFilters returns the empty set when nothing is stored', () => {
  withStorage((store) => {
    assert.equal(store.size, 0);
    assert.deepEqual(loadSavedRequestFilters(), EMPTY);
  });
});

test('saved filters round-trip through storage', () => {
  withStorage(() => {
    const filters: RequestLogFilters = {
      api_key_id: 'key-1',
      model: 'gpt-4o',
      provider_id: 'prov-9',
      status: 'failure',
    };
    saveSavedRequestFilters(filters);
    assert.deepEqual(loadSavedRequestFilters(), filters);
  });
});

test('corrupt or foreign storage falls back to the empty set', () => {
  withStorage((store) => {
    store.set(STORAGE_KEY, '{not json');
    assert.deepEqual(loadSavedRequestFilters(), EMPTY);
    store.set(STORAGE_KEY, '["not", "an", "object"]');
    assert.deepEqual(loadSavedRequestFilters(), EMPTY);
    store.set(STORAGE_KEY, '{"status":"sometimes"}');
    assert.deepEqual(loadSavedRequestFilters(), EMPTY, 'unknown statuses are dropped');
  });
});

test('only the four public filter fields are ever stored', () => {
  withStorage((store) => {
    saveSavedRequestFilters({
      api_key_id: 'key-1',
      model: 'gpt-4o',
      provider_id: 'prov-9',
      status: 'success',
    });
    const stored = JSON.parse(store.get(STORAGE_KEY)!) as Record<string, unknown>;
    assert.deepEqual(
      Object.keys(stored).sort(),
      ['api_key_id', 'model', 'provider_id', 'status'],
    );
  });
});

test('overlong filter strings are truncated, not stored whole', () => {
  withStorage(() => {
    saveSavedRequestFilters({
      api_key_id: 'k'.repeat(400),
      model: 'm'.repeat(400),
      provider_id: 'p'.repeat(400),
      status: '',
    });
    const loaded = loadSavedRequestFilters();
    assert.equal(loaded.api_key_id.length, 256);
    assert.equal(loaded.model.length, 256);
    assert.equal(loaded.provider_id.length, 256);
  });
});
