import assert from 'node:assert/strict';
import test from 'node:test';
import { THEMES, themeById, themeScheme, applyTheme, type ThemeId } from '../src/lib/theme';

test('theme catalog has all 7 themes with distinct design system style tags', () => {
  const expectedThemeIds: ThemeId[] = [
    'light',
    'dark',
    'tokyo-night',
    'catppuccin',
    'linear',
    'cyber-neon',
    'minimal-light',
  ];

  assert.equal(THEMES.length, 7);
  for (const id of expectedThemeIds) {
    const theme = themeById(id);
    assert.ok(theme, `Theme ${id} must exist in catalog`);
    assert.equal(theme.id, id);
    assert.ok(typeof theme.styleTag === 'string' && theme.styleTag.length > 0, `Theme ${id} must have a styleTag`);
    assert.ok(Array.isArray(theme.swatches) && theme.swatches.length === 4, `Theme ${id} must have 4 swatches`);
  }
});

test('theme scheme mapping categorizes light and dark themes correctly', () => {
  assert.equal(themeScheme('light'), 'light');
  assert.equal(themeScheme('minimal-light'), 'light');
  assert.equal(themeScheme('dark'), 'dark');
  assert.equal(themeScheme('tokyo-night'), 'dark');
  assert.equal(themeScheme('catppuccin'), 'dark');
  assert.equal(themeScheme('linear'), 'dark');
  assert.equal(themeScheme('cyber-neon'), 'dark');
});

test('each theme defines a distinct style archetype', () => {
  const light = themeById('light');
  assert.equal(light.styleTag, 'Neo-Brutalism');

  const minimalLight = themeById('minimal-light');
  assert.equal(minimalLight.styleTag, 'Swiss Minimal');

  const dark = themeById('dark');
  assert.equal(dark.styleTag, 'Glassmorphic');

  const tokyoNight = themeById('tokyo-night');
  assert.equal(tokyoNight.styleTag, 'IDE Terminal');

  const catppuccin = themeById('catppuccin');
  assert.equal(catppuccin.styleTag, 'Cozy Pastels');

  const linear = themeById('linear');
  assert.equal(linear.styleTag, 'Precision SaaS');

  const cyberNeon = themeById('cyber-neon');
  assert.equal(cyberNeon.styleTag, 'Cyberpunk HUD');
});

test('applyTheme updates document attributes correctly', () => {
  // Mock documentElement
  const dataset: Record<string, string> = {};
  const style: Record<string, string> = {};
  let metaColor = '';

  const mockDoc = {
    documentElement: {
      dataset,
      style,
    },
    querySelector: (selector: string) => {
      if (selector === 'meta[name="theme-color"]') {
        return {
          setAttribute: (_attr: string, val: string) => {
            metaColor = val;
          },
        };
      }
      return null;
    },
  };

  (globalThis as unknown as { document: typeof mockDoc }).document = mockDoc;
  (globalThis as unknown as { localStorage: Storage }).localStorage = {
    getItem: () => null,
    setItem: () => {},
    removeItem: () => {},
    clear: () => {},
    key: () => null,
    length: 0,
  };

  applyTheme('cyber-neon');
  assert.equal(dataset.theme, 'dark');
  assert.equal(dataset.palette, 'cyber-neon');
  assert.equal(dataset.themeStyle, 'cyber-neon');
  assert.equal(style.colorScheme, 'dark');
  assert.equal(metaColor, '#0a0a0f');

  applyTheme('minimal-light');
  assert.equal(dataset.theme, 'light');
  assert.equal(dataset.palette, 'minimal-light');
  assert.equal(dataset.themeStyle, 'minimal-light');
  assert.equal(style.colorScheme, 'light');
  assert.equal(metaColor, '#f8fafc');
});
