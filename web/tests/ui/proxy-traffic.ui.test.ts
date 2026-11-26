import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardProxiesPage from '../../src/features/proxies/DashboardProxiesPage.svelte';
import { proxyApi } from '../../src/lib/api/proxies';
import type { GatewayProxy, Translate } from '../../src/lib/types';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const proxies: GatewayProxy[] = [
  {
    id: 'proxy-a', name: 'Residential', strategy: 'priority', enabled: true,
    endpoints: [{ url: 'https://proxy-a.example', priority: 0, enabled: true }],
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z', provider_count: 2,
  },
  {
    id: 'proxy-b', name: 'Backup', strategy: 'round_robin', enabled: false, endpoints: [],
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z', provider_count: 1,
  },
];

/** StatsCard's KPI span is the element immediately before its heading. */
function statValue(heading: string): string {
  const title = Array.from(document.querySelectorAll('h2.stat-title'))
    .find((node) => node.textContent?.trim() === heading);
  const value = title?.previousElementSibling?.textContent?.trim();
  if (value === null || value === undefined) throw new Error(`Stat card "${heading}" not found`);
  return value;
}

afterEach(() => vi.restoreAllMocks());

describe('proxy overview cards', () => {
  test('summarizes configured proxies and filters to enabled groups', async () => {
    const trafficStats = vi.spyOn(proxyApi, 'trafficStats');
    vi.spyOn(proxyApi, 'proxies').mockResolvedValue(proxies);
    vi.spyOn(proxyApi, 'proxyOptions').mockResolvedValue({ proxies: [], next_cursor: null });

    render(DashboardProxiesPage, { props: { tr, locale: 'en' } });

    expect(await screen.findByText('Total proxies')).toBeTruthy();
    expect(statValue('Total proxies')).toBe('2');
    expect(screen.getByRole('heading', { name: 'Enabled' })).toBeTruthy();
    expect(statValue('Enabled')).toBe('1');
    expect(statValue('Endpoints')).toBe('1');
    expect(screen.getByText('Across 2 proxy groups')).toBeTruthy();
    expect(screen.queryByText('Network usage')).toBeNull();
    expect(trafficStats).not.toHaveBeenCalled();

    // Enabled-only filtering is now the status <select>, not a toggle button.
    await fireEvent.change(screen.getByRole('combobox', { name: 'Filter by status' }), { target: { value: 'enabled' } });
    expect(screen.getByText('Residential')).toBeTruthy();
    expect(screen.queryByText('Backup')).toBeNull();
  });
});
