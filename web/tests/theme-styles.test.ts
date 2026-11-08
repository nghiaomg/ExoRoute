import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import test from 'node:test';
import {
  THEMES,
  THEME_STORAGE_KEY,
  themeById,
  themeScheme,
  applyTheme,
  applyThemePreference,
  getStoredThemePreference,
  resolveTheme,
  setStoredThemePreference,
  type ThemeId,
} from '../src/lib/theme';
import { themeBootstrapCatalogIds } from '../src/lib/theme-bootstrap';

const expectedThemeIds: ThemeId[] = [
  'light', 'dark', 'tokyo-night', 'catppuccin', 'linear', 'cyber-neon', 'minimal-light',
];

test('theme catalog has 7 visual themes and matches first-paint bootstrap IDs', () => {
  assert.equal(THEMES.length, 7);
  assert.deepEqual(THEMES.map(({ id }) => id), expectedThemeIds);
  assert.deepEqual(themeBootstrapCatalogIds(), expectedThemeIds);
  for (const theme of THEMES) {
    assert.ok(theme.styleTag.length > 0, `${theme.id} has a style tag`);
    assert.equal(theme.swatches.length, 4);
    assert.match(theme.metaColor, /^#[0-9a-f]{6}$/i);
  }
});

test('theme scheme mapping categorizes all light and dark palettes correctly', () => {
  assert.equal(themeScheme('light'), 'light');
  assert.equal(themeScheme('minimal-light'), 'light');
  for (const id of ['dark', 'tokyo-night', 'catppuccin', 'linear', 'cyber-neon'] as const) {
    assert.equal(themeScheme(id), 'dark');
  }
});

test('each theme has its documented design archetype', () => {
  const archetypes = [
    'Neo-Brutalism', 'Glassmorphic', 'IDE Terminal', 'Cozy Pastels',
    'Precision SaaS', 'Cyberpunk HUD', 'Swiss Minimal',
  ];
  assert.deepEqual(THEMES.map(({ styleTag }) => styleTag), archetypes);
});

test('system preference resolves dynamically and explicit themes remain fixed', () => {
  assert.equal(resolveTheme('system', 'light'), 'light');
  assert.equal(resolveTheme('system', 'dark'), 'dark');
  assert.equal(resolveTheme('tokyo-night', 'light'), 'tokyo-night');
  assert.equal(resolveTheme('minimal-light', 'dark'), 'minimal-light');
});

test('storage distinguishes System from an explicit classic theme', () => {
  const values = new Map<string, string>();
  (globalThis as unknown as { localStorage: Storage }).localStorage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
    removeItem: (key: string) => { values.delete(key); },
    clear: () => values.clear(),
    key: (index: number) => [...values.keys()][index] ?? null,
    get length() { return values.size; },
  };

  values.clear();
  assert.equal(getStoredThemePreference(), 'system');
  setStoredThemePreference('system');
  assert.equal(values.has(THEME_STORAGE_KEY), false);
  assert.equal(getStoredThemePreference(), 'system');
  setStoredThemePreference('tokyo-night');
  assert.equal(values.get(THEME_STORAGE_KEY), 'tokyo-night');
  assert.equal(getStoredThemePreference(), 'tokyo-night');
  values.set(THEME_STORAGE_KEY, 'not-a-theme');
  assert.equal(getStoredThemePreference(), 'system');
  values.clear();
});

test('storage failures fall back to System and do not prevent applying the theme', () => {
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: {
      getItem: () => { throw new Error('storage denied'); },
      setItem: () => { throw new Error('storage denied'); },
      removeItem: () => { throw new Error('storage denied'); },
    },
  });
  assert.equal(getStoredThemePreference(), 'system');
  assert.doesNotThrow(() => setStoredThemePreference('system'));
});

test('applyTheme sets the requested visual theme and exact browser theme color', () => {
  const dataset: Record<string, string> = {};
  const style: Record<string, string> = {};
  let metaColor = '';
  const mockDoc = {
    documentElement: { dataset, style },
    querySelector: (selector: string) => selector === 'meta[name="theme-color"]'
      ? { setAttribute: (_attribute: string, value: string) => { metaColor = value; } }
      : null,
  };
  (globalThis as unknown as { document: typeof mockDoc }).document = mockDoc;

  for (const theme of THEMES) {
    applyTheme(theme.id);
    assert.equal(dataset.theme, theme.scheme);
    assert.equal(dataset.palette, theme.id);
    assert.equal(dataset.themeStyle, theme.id);
    assert.equal(style.colorScheme, theme.scheme);
    assert.equal(metaColor, theme.metaColor);
  }
});

test('applying System resolves to current OS scheme without persisting the resolved ID', () => {
  const values = new Map<string, string>();
  (globalThis as unknown as { localStorage: Storage }).localStorage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
    removeItem: (key: string) => { values.delete(key); },
    clear: () => values.clear(), key: () => null, length: 0,
  };
  const dataset: Record<string, string> = {};
  const style: Record<string, string> = {};
  let metaColor = '';
  (globalThis as unknown as { document: unknown }).document = {
    documentElement: { dataset, style },
    querySelector: () => ({ setAttribute: (_: string, value: string) => { metaColor = value; } }),
  };

  assert.equal(applyThemePreference('system', 'dark'), 'dark');
  assert.equal(dataset.theme, 'dark');
  assert.equal(values.has(THEME_STORAGE_KEY), false);
  assert.equal(metaColor, themeById('dark').metaColor);
  setStoredThemePreference('system');
  assert.equal(applyThemePreference('system', 'light'), 'light');
  assert.equal(values.has(THEME_STORAGE_KEY), false);
});

test('first-paint bootstrap resolves System, explicit palettes, and blocked storage', () => {
  const html = readFileSync(new URL('../index.html', import.meta.url), 'utf8');
  const themeIds = [...html.matchAll(/const themeIds = \[([^\]]+)\]/g)][0]?.[1]
    .match(/'([^']+)'/g)?.map((id) => id.slice(1, -1));
  assert.deepEqual(themeIds, THEMES.map(({ id }) => id), 'first-paint bootstrap ids match the theme catalog');
  const script = html.match(/<script>([\s\S]*?)<\/script>/)?.[1];
  assert.ok(script, 'inline synchronous bootstrap exists before styles and app bundle');
  const runBootstrap = (savedTheme: string | null, dark: boolean, storageBlocked = false) => {
    const root = { dataset: {} as Record<string, string>, style: {} as Record<string, string>, lang: '', dir: '' };
    const meta = { content: '', setAttribute(_name: string, value: string) { this.content = value; } };
    const localStorage = {
      getItem(key: string): string | null {
        if (storageBlocked) throw new Error('storage denied');
        if (key === 'exoroute.theme') return savedTheme;
        return null;
      },
    };
    runInNewContext(script, {
      localStorage,
      window: { matchMedia: () => ({ matches: dark }) },
      document: { documentElement: root, querySelector: () => meta },
    });
    return { root, meta };
  };

  const systemDark = runBootstrap(null, true);
  assert.equal(systemDark.root.dataset.theme, 'dark');
  assert.equal(systemDark.root.dataset.palette, 'dark');
  assert.equal(systemDark.meta.content, themeById('dark').metaColor);

  const systemLightStorageBlocked = runBootstrap(null, false, true);
  assert.equal(systemLightStorageBlocked.root.dataset.theme, 'light');
  assert.equal(systemLightStorageBlocked.root.dataset.palette, 'light');
  assert.equal(systemLightStorageBlocked.meta.content, themeById('light').metaColor);

  for (const theme of THEMES) {
    const explicit = runBootstrap(theme.id, !theme.scheme || theme.scheme === 'light');
    assert.equal(explicit.root.dataset.theme, theme.scheme, `${theme.id} first-paint scheme`);
    assert.equal(explicit.root.dataset.palette, theme.id, `${theme.id} first-paint palette`);
    assert.equal(explicit.meta.content, theme.metaColor, `${theme.id} first-paint theme color`);
  }
});

test('palette text and focus tokens meet contrast requirements using the actual CSS cascade', () => {
  const tokensCss = readFileSync(new URL('../src/styles/tokens.css', import.meta.url), 'utf8');
  const palettesCss = readFileSync(new URL('../src/styles/palettes.css', import.meta.url), 'utf8');
  const declarations = (css: string, selector: string): Record<string, string> => {
    const selectorIndex = css.indexOf(selector);
    assert.notEqual(selectorIndex, -1, `missing CSS rule: ${selector}`);
    const openBrace = css.indexOf('{', selectorIndex);
    const closeBrace = css.indexOf('}', openBrace);
    assert.ok(openBrace > selectorIndex && closeBrace > openBrace, `invalid CSS rule: ${selector}`);
    return Object.fromEntries(css.slice(openBrace + 1, closeBrace).split(';').flatMap((declaration) => {
      const separator = declaration.indexOf(':');
      if (separator < 0) return [];
      const name = declaration.slice(0, separator).split(/\r?\n/).at(-1)?.trim();
      const value = declaration.slice(separator + 1).trim();
      return name?.startsWith('--') ? [[name, value]] : [];
    }));
  };
  const contrast = (foreground: string, background: string): number => {
    const luminance = (hex: string): number => {
      const channels = hex.replace('#', '').match(/.{2}/g);
      assert.ok(channels?.length === 3, `expected a six-digit color, got ${hex}`);
      const [red, green, blue] = channels.map((channel) => parseInt(channel, 16) / 255)
        .map((value) => value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4);
      return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
    };
    const [high, low] = [luminance(foreground), luminance(background)].sort((a, b) => b - a);
    return (high + 0.05) / (low + 0.05);
  };

  for (const theme of THEMES) {
    const cascade = { ...declarations(tokensCss, ':root') };
    if (theme.scheme === 'dark') Object.assign(cascade, declarations(tokensCss, ":root[data-theme='dark']"));
    if (theme.id !== 'light' && theme.id !== 'dark') {
      Object.assign(cascade, declarations(palettesCss, `:root[data-theme='${theme.scheme}'][data-palette='${theme.id}']`));
    }
    const canvas = cascade['--canvas'];
    const ink = cascade['--ink'];
    const muted = cascade['--muted'];
    const dim = cascade['--text-dim'];
    assert.ok(canvas && ink && muted && dim, `${theme.id} defines semantic text tokens`);
    assert.ok(contrast(ink, canvas) >= 4.5, `${theme.id} ink contrast`);
    assert.ok(contrast(muted, canvas) >= 4.5, `${theme.id} muted contrast`);
    assert.ok(contrast(dim, canvas) >= 4.5, `${theme.id} text-dim contrast`);
  }
});
