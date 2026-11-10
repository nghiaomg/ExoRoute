import { render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardProxiesPage from '../../src/features/proxies/DashboardProxiesPage.svelte';
import { proxyApi } from '../../src/lib/api/proxies';
import type { GatewayProxy, Translate } from '../../src/lib/types';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const proxy: GatewayProxy = {
  id: 'proxy-a', name: 'Residential', strategy: 'priority', enabled: true, endpoints: [],
  created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z', provider_count: 1,
};

afterEach(() => vi.restoreAllMocks());

describe('proxy network usage cards', () => {
  test('shows measured bytes and average speed per proxy', async () => {
    vi.spyOn(proxyApi, 'proxies').mockResolvedValue([proxy]);
    vi.spyOn(proxyApi, 'proxyOptions').mockResolvedValue({ proxies: [], next_cursor: null });
    vi.spyOn(proxyApi, 'trafficStats').mockResolvedValue({
      scope: 'since_startup',
      proxies: [{ proxy_id: proxy.id, received_bytes: 2048, sent_bytes: 1024, total_bytes: 3072, requests: 2, average_bytes_per_second: 512 }],
    });

    render(DashboardProxiesPage, { props: { tr, locale: 'en' } });

    expect(await screen.findByText('3 KiB')).toBeTruthy();
    expect(screen.getByText('2 KiB')).toBeTruthy();
    expect(screen.getByText('1 KiB')).toBeTruthy();
    expect(screen.getByText('512 B/s')).toBeTruthy();
    expect(screen.getByText('2 requests')).toBeTruthy();
  });
});
