import { readFileSync } from 'node:fs';
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { describe, expect, test, vi } from 'vitest';
import AppearancePanel from '../../src/features/settings/AppearancePanel.svelte';
import type { Translate } from '../../src/lib/format';

const tr: Translate = (key, vars) => key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

function renderAppearance(themePreference: 'system' | 'light' | 'dark' | 'tokyo-night' = 'system') {
  const onThemeChange = vi.fn();
  const onThemePreferenceChange = vi.fn();
  const result = render(AppearancePanel, {
    props: {
      tr,
      theme: themePreference === 'system' ? 'dark' : themePreference,
      themePreference,
      onThemeChange,
      onThemePreferenceChange,
    },
  });
  return { ...result, onThemeChange, onThemePreferenceChange };
}

describe('appearance theme radio group', () => {
  test('exposes the selected System preference and only one tab stop', () => {
    renderAppearance();

    const group = screen.getByRole('radiogroup', { name: 'Theme' });
    const radios = within(group).getAllByRole('radio');
    const selected = screen.getByRole('radio', { name: 'System theme' });

    expect(radios).toHaveLength(8);
    expect(selected.getAttribute('aria-checked')).toBe('true');
    expect(selected.getAttribute('tabindex')).toBe('0');
    expect(radios.filter((radio) => radio.getAttribute('tabindex') === '0')).toEqual([selected]);
    expect(screen.getByText('Currently dark')).toBeTruthy();
    expect(screen.getByText('Follow your operating system appearance setting.')).toBeTruthy();
  });

  test('keyboard arrows, Home, and End move focus and update the roving tab stop', async () => {
    const { onThemeChange, onThemePreferenceChange } = renderAppearance('light');
    const light = screen.getByRole('radio', { name: 'Classic Light' });
    light.focus();

    await fireEvent.keyDown(light, { key: 'ArrowRight' });
    const dark = screen.getByRole('radio', { name: 'Classic Dark' });
    expect(document.activeElement).toBe(dark);
    expect(dark.getAttribute('aria-checked')).toBe('false');
    expect(dark.getAttribute('tabindex')).toBe('0');
    expect(onThemeChange).toHaveBeenLastCalledWith('dark');

    await fireEvent.keyDown(dark, { key: 'Home' });
    const system = screen.getByRole('radio', { name: 'System theme' });
    expect(document.activeElement).toBe(system);
    expect(system.getAttribute('aria-checked')).toBe('false');
    expect(system.getAttribute('tabindex')).toBe('0');
    expect(onThemePreferenceChange).toHaveBeenLastCalledWith('system');

    await fireEvent.keyDown(system, { key: 'End' });
    const minimalLight = screen.getByRole('radio', { name: /Minimal Light/ });
    expect(document.activeElement).toBe(minimalLight);
    expect(minimalLight.getAttribute('aria-checked')).toBe('false');
    expect(minimalLight.getAttribute('tabindex')).toBe('0');
    expect(onThemeChange).toHaveBeenLastCalledWith('minimal-light');
  });

  test('pointer selection routes System and explicit themes through their respective callbacks', async () => {
    const { onThemeChange, onThemePreferenceChange } = renderAppearance('dark');

    await fireEvent.click(screen.getByRole('radio', { name: 'System theme' }));
    expect(onThemePreferenceChange).toHaveBeenCalledWith('system');

    await fireEvent.click(screen.getByRole('radio', { name: 'Tokyo Night' }));
    expect(onThemeChange).toHaveBeenCalledWith('tokyo-night');
  });

  test('reduced-motion preference disables theme-card transitions and hover transform', () => {
    const css = readFileSync('src/features/settings/AppearancePanel.svelte', 'utf8');
    expect(css).toContain('@media (prefers-reduced-motion: reduce)');
    expect(css).toContain('.appearance-theme-card { transition: none; }');
    expect(css).toContain('.appearance-theme-card:hover { transform: none; }');
  });
});
