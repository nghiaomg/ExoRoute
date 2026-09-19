import assert from 'node:assert/strict';
import test from 'node:test';

test('all supported locales have exact 100% key parity with en.ts and no missing or extra keys', async () => {
  const { catalogs, en } = await import('../src/lib/i18n');

  const enKeys = Object.keys(en);
  const enKeySet = new Set(enKeys);

  const locales = Object.fromEntries(
    Object.entries(catalogs).filter(([localeName]) => localeName !== 'en'),
  ) as Record<string, Record<string, string>>;

  assert.ok(enKeys.length > 1400, `Expected en.ts to have over 1400 keys, found ${enKeys.length}`);

  for (const [localeName, catalog] of Object.entries(locales)) {
    const catalogKeys = Object.keys(catalog);
    const catalogKeySet = new Set(catalogKeys);

    // 1. Check for missing keys
    const missingKeys = enKeys.filter((key) => !catalogKeySet.has(key));
    assert.deepEqual(
      missingKeys,
      [],
      `Locale "${localeName}" is missing ${missingKeys.length} keys from en.ts: ${missingKeys.slice(0, 5).join(', ')}…`
    );

    // 2. Check for extra keys
    const extraKeys = catalogKeys.filter((key) => !enKeySet.has(key));
    assert.deepEqual(
      extraKeys,
      [],
      `Locale "${localeName}" has ${extraKeys.length} extra keys not in en.ts: ${extraKeys.slice(0, 5).join(', ')}…`
    );

    // 3. Check for non-empty translations
    for (const key of enKeys) {
      const val = catalog[key];
      assert.ok(
        typeof val === 'string' && val.trim().length > 0,
        `Locale "${localeName}" has an empty translation for key "${key}"`
      );
    }
  }
});

test('placeholders match between en.ts and all other locales across all keys', async () => {
  const { catalogs, en } = await import('../src/lib/i18n');

  const locales = Object.fromEntries(
    Object.entries(catalogs).filter(([localeName]) => localeName !== 'en'),
  ) as Record<string, Record<string, string>>;

  const extractPlaceholders = (str: string): string[] => {
    const matches = str.match(/\{([a-zA-Z0-9_]+)\}/g) || [];
    return matches.sort();
  };

  for (const [key, enVal] of Object.entries(en as Record<string, string>)) {
    const enPlaceholders = extractPlaceholders(enVal);
    if (enPlaceholders.length === 0) continue;

    for (const [localeName, catalog] of Object.entries(locales)) {
      const locVal = catalog[key];
      const locPlaceholders = extractPlaceholders(locVal);
      assert.deepEqual(
        locPlaceholders,
        enPlaceholders,
        `Placeholder mismatch in "${localeName}" for key "${key}": expected [${enPlaceholders.join(', ')}], got [${locPlaceholders.join(', ')}]`
      );
    }
  }
});

test('newly introduced token usage and state keys exist and are translated', async () => {
  const { catalogs } = await import('../src/lib/i18n');

  const newKeys = [
    'Token usage',
    'TOKENS',
    '{input} in / {output} out',
    'Invalid',
    'Connected',
    'Ready',
    'Not tested',
    'Disabled',
    'Enabled',
    'OAuth',
  ];

  for (const [lang, cat] of Object.entries(catalogs)) {
    for (const key of newKeys) {
      assert.ok(key in cat, `Key "${key}" missing in ${lang}`);
      const val = (cat as Record<string, string>)[key];
      assert.ok(typeof val === 'string' && val.length > 0, `Key "${key}" in ${lang} is empty`);
    }
  }
});

test('default locale is English and only changes when explicitly stored', async () => {
  const { getStoredLocale, saveLocale } = await import('../src/lib/i18n');

  const storage = new Map<string, string>();
  const mockLocalStorage = {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
    removeItem: (key: string) => storage.delete(key),
    clear: () => storage.clear(),
  };

  const originalLocalStorage = globalThis.localStorage;
  Object.defineProperty(globalThis, 'localStorage', {
    value: mockLocalStorage,
    configurable: true,
    writable: true,
  });

  try {
    mockLocalStorage.clear();
    assert.equal(getStoredLocale(), 'en', 'Default locale must be English when nothing is stored');

    mockLocalStorage.setItem('exoroute.locale', 'invalid-locale');
    assert.equal(getStoredLocale(), 'en', 'Invalid locale in storage must fall back to English');

    saveLocale('vi');
    assert.equal(getStoredLocale(), 'vi', 'Explicitly saved locale must be returned');

    saveLocale('ja');
    assert.equal(getStoredLocale(), 'ja', 'Explicitly saved locale must be returned');
  } finally {
    Object.defineProperty(globalThis, 'localStorage', {
      value: originalLocalStorage,
      configurable: true,
      writable: true,
    });
  }
});
