import assert from 'node:assert/strict';
import test from 'node:test';

test('prototype-polluting keys never throw and echo back', async () => {
  const { t } = await import('../src/lib/i18n');
  for (const key of ['__proto__', 'constructor', 'toString', 'hasOwnProperty', 'valueOf']) {
    assert.equal(t('en', key), key, `Key "${key}" must echo back`);
    assert.equal(t('vi', key), key, `Key "${key}" must echo back for vi`);
  }
});

test('unknown locales fall back to English instead of throwing', async () => {
  const { t, getIntlLocale } = await import('../src/lib/i18n');
  const bogus = 'xx' as never;
  assert.equal(t(bogus, 'Overview'), 'Overview');
  assert.equal(t(bogus, '__proto__'), '__proto__');
  assert.equal(getIntlLocale(bogus), 'en-US');
  assert.equal(getIntlLocale('vi'), 'vi-VN');
});

test('translation interpolation never leaks prototype values', async () => {
  const { t } = await import('../src/lib/i18n');
  assert.equal(t('en', 'Loading {page}…', { page: '__proto__' }), 'Loading __proto__…');
  const poisoned = JSON.parse('{ "toString": "pwned" }');
  assert.equal(t('en', 'Loading {page}…', poisoned), 'Loading {page}…');
});

test('backend-controlled details are never treated as translation keys', async () => {
  const { localizedError } = await import('../src/lib/errors');
  const { ApiError } = await import('../src/lib/api');
  const tr = (key: string) => `TR:${key}`;
  assert.equal(localizedError(new ApiError('__proto__', 500), 'fallback', tr), '__proto__');
  assert.equal(localizedError(new ApiError('Overview', 500), 'fallback', tr), 'TR:Overview');
  assert.equal(
    localizedError(new ApiError('Request failed (418).', 418), 'fallback', tr),
    'Request failed (418).'
  );
});

test('saveLocale ignores values outside the locale allowlist', async () => {
  const { getStoredLocale, saveLocale } = await import('../src/lib/i18n');
  const storage = new Map<string, string>();
  const originalLocalStorage = globalThis.localStorage;
  Object.defineProperty(globalThis, 'localStorage', {
    value: {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
      removeItem: (key: string) => storage.delete(key),
      clear: () => storage.clear(),
    },
    configurable: true,
    writable: true,
  });
  try {
    storage.clear();
    saveLocale('__proto__' as never);
    assert.equal(storage.get('exoroute.locale'), undefined);
    assert.equal(getStoredLocale(), 'en');
    saveLocale('vi');
    saveLocale('xx' as never);
    assert.equal(storage.get('exoroute.locale'), 'vi');
  } finally {
    Object.defineProperty(globalThis, 'localStorage', {
      value: originalLocalStorage,
      configurable: true,
      writable: true,
    });
  }
});
