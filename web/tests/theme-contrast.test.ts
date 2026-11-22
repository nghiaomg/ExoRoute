import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { THEMES } from '../src/lib/theme';

const tokensCss = readFileSync(new URL('../src/styles/tokens.css', import.meta.url), 'utf8');
const palettesCss = readFileSync(new URL('../src/styles/palettes.css', import.meta.url), 'utf8');

type Cascade = Record<string, string>;

function declarations(css: string, selector: string): Cascade {
  const index = css.indexOf(selector);
  assert.notEqual(index, -1, `missing CSS rule: ${selector}`);
  const open = css.indexOf('{', index);
  const close = css.indexOf('}', open);
  assert.ok(open > index && close > open, `invalid CSS rule: ${selector}`);
  const result: Cascade = {};
  for (const declaration of css.slice(open + 1, close).split(';')) {
    const separator = declaration.indexOf(':');
    if (separator < 0) continue;
    const name = declaration.slice(0, separator).split(/\r?\n/).at(-1)?.trim();
    if (name?.startsWith('--')) result[name] = declaration.slice(separator + 1).trim();
  }
  return result;
}

function resolveToken(cascade: Cascade, key: string, seen = new Set<string>()): string {
  const value = cascade[key] ?? '';
  const match = /^var\((--[\w-]+)\)$/.exec(value);
  if (match && !seen.has(match[1])) {
    seen.add(match[1]);
    return resolveToken(cascade, match[1], seen);
  }
  return value;
}

function luminance(color: string): number {
  const channels = color.replace('#', '').match(/.{2}/g);
  assert.ok(channels?.length === 3, `expected a six-digit hex color, got '${color}'`);
  const linear = channels
    .map((channel) => parseInt(channel, 16) / 255)
    .map((value) => (value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4));
  return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
}

function contrast(foreground: string, background: string): number {
  const [high, low] = [luminance(foreground), luminance(background)].sort((a, b) => b - a);
  return (high + 0.05) / (low + 0.05);
}

function cascadeFor(themeId: string, scheme: 'light' | 'dark'): Cascade {
  const cascade: Cascade = { ...declarations(tokensCss, ':root') };
  if (scheme === 'dark') {
    Object.assign(cascade, declarations(tokensCss, ":root[data-theme='dark']"));
  }
  if (themeId !== 'light' && themeId !== 'dark') {
    Object.assign(
      cascade,
      declarations(palettesCss, `:root[data-theme='${scheme}'][data-palette='${themeId}']`),
    );
  }
  return cascade;
}

for (const theme of THEMES) {
  test(`${theme.id}: text tokens meet 4.5:1 on canvas and surface`, () => {
    const cascade = cascadeFor(theme.id, theme.scheme);
    const get = (key: string): string => {
      const value = resolveToken(cascade, key);
      assert.match(value, /^#[0-9a-f]{6}$/i, `${theme.id} ${key} must be a hex color, got '${value}'`);
      return value;
    };
    const canvas = get('--canvas');
    const surface = get('--surface');
    for (const [name, background] of [['canvas', canvas], ['surface', surface]] as const) {
      for (const token of ['--ink', '--muted', '--info'] as const) {
        const foreground = get(token);
        const ratio = contrast(foreground, background);
        assert.ok(
          ratio >= 4.5,
          `${theme.id}: ${token} (${foreground}) on ${name} (${background}) = ${ratio.toFixed(2)}:1 < 4.5:1`,
        );
      }
    }
  });

  test(`${theme.id}: accent-contrast meets 4.5:1 on accent`, () => {
    const cascade = cascadeFor(theme.id, theme.scheme);
    const get = (key: string): string => {
      const value = resolveToken(cascade, key);
      assert.match(value, /^#[0-9a-f]{6}$/i, `${theme.id} ${key} must be a hex color, got '${value}'`);
      return value;
    };
    const accent = get('--accent');
    const accentContrast = get('--accent-contrast');
    const ratio = contrast(accentContrast, accent);
    assert.ok(
      ratio >= 4.5,
      `${theme.id}: --accent-contrast (${accentContrast}) on --accent (${accent}) = ${ratio.toFixed(2)}:1 < 4.5:1`,
    );
  });

  test(`${theme.id}: focus indicator meets 3:1 on canvas and surface`, () => {
    const cascade = cascadeFor(theme.id, theme.scheme);
    const get = (key: string): string => {
      const value = resolveToken(cascade, key);
      assert.match(value, /^#[0-9a-f]{6}$/i, `${theme.id} ${key} must be a hex color, got '${value}'`);
      return value;
    };
    for (const [name, background] of [['canvas', get('--canvas')], ['surface', get('--surface')]] as const) {
      const focus = get('--focus-ring');
      const ratio = contrast(focus, background);
      assert.ok(
        ratio >= 3,
        `${theme.id}: --focus-ring (${focus}) on ${name} (${background}) = ${ratio.toFixed(2)}:1 < 3:1`,
      );
    }
  });
}
