import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import {
  countMaskedColumns,
  DEFAULT_MASKED_COLUMNS,
  loadMaskedColumns,
  MASKABLE_COLUMNS,
  maskTextValue,
  saveMaskedColumns,
  type MaskedColumnsState,
} from '../src/features/requests/request.mask';

test('maskTextValue masks text up to a maximum of 3 asterisks', () => {
  // Empty, null, undefined, or dash returns dash
  assert.equal(maskTextValue(null), '—');
  assert.equal(maskTextValue(undefined), '—');
  assert.equal(maskTextValue(''), '—');
  assert.equal(maskTextValue('   '), '—');
  assert.equal(maskTextValue('—'), '—');

  // Length 1 -> 1 asterisk
  assert.equal(maskTextValue('a'), '*');

  // Length 2 -> 2 asterisks
  assert.equal(maskTextValue('ab'), '**');

  // Length 3 -> 3 asterisks
  assert.equal(maskTextValue('abc'), '***');

  // Length > 3 -> maximum of 3 asterisks
  assert.equal(maskTextValue('abcd'), '***');
  assert.equal(maskTextValue('combo-123'), '***');
  assert.equal(maskTextValue('my-super-long-custom-combo-name'), '***');
});

test('countMaskedColumns counts active masked columns correctly', () => {
  assert.equal(countMaskedColumns(DEFAULT_MASKED_COLUMNS), 0);

  const oneMasked: MaskedColumnsState = {
    alias: true,
    key: false,
    model: false,
    provider: false,
  };
  assert.equal(countMaskedColumns(oneMasked), 1);

  const allMasked: MaskedColumnsState = {
    alias: true,
    key: true,
    model: true,
    provider: true,
  };
  assert.equal(countMaskedColumns(allMasked), 4);
});

test('MASKABLE_COLUMNS exposes expected columns', () => {
  const columnIds = MASKABLE_COLUMNS.map((col) => col.id);
  assert.deepEqual(columnIds, ['key', 'model', 'alias', 'provider']);
});

test('loadMaskedColumns and saveMaskedColumns persist state safely', () => {
  const store = new Map<string, string>();
  const originalWindow = globalThis.window;
  const originalLocalStorage = globalThis.localStorage;

  try {
    // Mock minimal localStorage
    globalThis.localStorage = {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => { store.set(key, value); },
      removeItem: (key: string) => { store.delete(key); },
      clear: () => store.clear(),
      key: () => null,
      length: 0,
    };
    (globalThis as any).window = {};

    // 1. Initial state when empty
    assert.deepEqual(loadMaskedColumns(), DEFAULT_MASKED_COLUMNS);

    // 2. Save and reload
    const state: MaskedColumnsState = {
      alias: true,
      key: false,
      model: true,
      provider: false,
    };
    saveMaskedColumns(state);
    assert.deepEqual(loadMaskedColumns(), state);

    // 3. Fallback on corrupt JSON
    store.set('exoroute:requests:masked_columns', '{ corrupt json');
    assert.deepEqual(loadMaskedColumns(), DEFAULT_MASKED_COLUMNS);
  } finally {
    globalThis.window = originalWindow;
    globalThis.localStorage = originalLocalStorage;
  }
});
