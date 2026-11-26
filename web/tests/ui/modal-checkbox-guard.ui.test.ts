import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, test } from 'vitest';

const css = readFileSync(
  resolve(process.cwd(), 'src/styles/modals-and-quota.css'),
  'utf8',
);

const guardedSelectors = [
  '.modal-form input:not([type="checkbox"]):not([type="radio"]):not([type="range"])',
  '.provider-key-input-row input:not([type="checkbox"]):not([type="radio"])',
  '.provider-custom-header-row input:not([type="checkbox"]):not([type="radio"])',
  '.provider-key-rename input:not([type="checkbox"]):not([type="radio"])',
  '.provider-budget-editor input:not([type="checkbox"]):not([type="radio"])',
  '.quota-target-form input:not([type="checkbox"]):not([type="radio"])',
];

function guardRule(): string | undefined {
  return css.match(
    /\.modal-form input\[type="checkbox"\],\s*\.modal-form input\[type="radio"\]\s*\{[^}]*\}/,
  )?.[0];
}

describe('modal checkbox and radio width guard', () => {
  test('text-field skins exclude checkbox, radio and range controls', () => {
    for (const selector of guardedSelectors) {
      expect(css, selector).toContain(selector);
    }
    expect(css).not.toMatch(/\.modal-form input,\s*\.modal-form select/);
    expect(css).not.toMatch(/\.modal-form input\s*\{/);
  });

  test('checkbox and radio inputs keep intrinsic width and height without importance overrides', () => {
    const rule = guardRule();
    expect(rule).toBeDefined();
    expect(rule).toContain('width: auto');
    expect(rule).toContain('height: auto');
    expect(rule).toContain('flex: 0 0 auto');
    expect(rule).not.toContain('!important');
  });
});
