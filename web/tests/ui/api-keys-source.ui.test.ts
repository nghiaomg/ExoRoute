import { render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardApiKeysPage from '../../src/features/api-keys/DashboardApiKeysPage.svelte';
import { api } from '../../src/lib/api';
import type { GatewayApiKey, Translate } from '../../src/lib/types';
import { statisticsFixture } from './support/statistics';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const keys: GatewayApiKey[] = Array.from({ length: 50 }, (_, index) => ({
  id: `key-${index}`,
  name: `key-${index}`,
  enabled: index < 37,
  created_at: '2026-11-01 00:00:00',
}));

afterEach(() => vi.restoreAllMocks());

describe('API-key overview scope', () => {
  test('labels counts as this page and omits unsupported expiry filter', async () => {
    vi.spyOn(api, 'apiKeys').mockResolvedValue({ api_keys: keys, next_cursor: 'next-page' });
    vi.spyOn(api, 'statistics').mockResolvedValue(statisticsFixture());
    vi.spyOn(api, 'providers').mockResolvedValue([]);

    render(DashboardApiKeysPage, {
      props: { tr, onNavigate: () => {} },
    });

    expect(await screen.findByText('Showing 50 API keys on this page · more available')).toBeTruthy();
    expect(screen.getByRole('option', { name: 'All keys' })).toBeTruthy();
    expect(screen.queryByRole('option', { name: 'Expiring soon' })).toBeNull();
    // The count remains explicitly page-local; no unbounded cursor walk occurs.
    expect(api.apiKeys).toHaveBeenCalledTimes(1);
  });
});
