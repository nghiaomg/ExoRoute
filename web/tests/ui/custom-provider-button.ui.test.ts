import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardProvidersPage from '../../src/features/providers/DashboardProvidersPage.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
const tr: Translate = (key, vars) => key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));
function setup() {
  vi.spyOn(api, 'providers').mockResolvedValue([]);
  vi.spyOn(api, 'providerPresets').mockResolvedValue([]);
  render(DashboardProvidersPage, { props: { tr, locale: 'en', onConnectionChange: () => {}, onProviderCountChange: () => {} } });
}
afterEach(() => vi.restoreAllMocks());
describe('custom provider button', () => {
  test('is always visible even with zero providers and zero presets', async () => {
    setup();
    expect(await screen.findByRole('button', { name: 'Add a custom provider' })).toBeTruthy();
  });
  test('opens the create dialog and stays visible after a no-match search', async () => {
    setup();
    const button = await screen.findByRole('button', { name: 'Add a custom provider' });
    await fireEvent.click(button);
    expect(await screen.findByRole('dialog')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Add a custom provider' })).toBeTruthy();
  });
});
