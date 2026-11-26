import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardProxiesPage from '../../src/features/proxies/DashboardProxiesPage.svelte';
import { proxyApi } from '../../src/lib/api/proxies';
import { en } from '../../src/lib/locales/en';
import type { GatewayProxy, Translate } from '../../src/lib/types';
import type { ProxyCheckResponse } from '../../src/lib/types/proxies';

// Resolves through the real English dictionary so the assertions cover the
// strings the dashboard actually renders.
const tr: Translate = (key, vars) =>
  ((en as unknown as Record<string, string>)[key] ?? key).replace(
    /\{(\w+)\}/g,
    (_, name: string) => String(vars?.[name] ?? `{${name}}`),
  );

const proxy: GatewayProxy = {
  id: 'proxy-check',
  name: 'Residential',
  strategy: 'priority',
  enabled: true,
  endpoints: [{ url: 'http://127.0.0.1:8080', priority: 0, enabled: true }],
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  provider_count: 1,
};

const checkResponse: ProxyCheckResponse = {
  proxy_id: 'proxy-check',
  checked_at: 1_700_000_000,
  results: [
    { endpoint_id: 'a', label: null, url: 'http://127.0.0.1:8080/', status: 'live', latency_ms: 12, error: null },
    { endpoint_id: 'b', label: null, url: 'http://127.0.0.1:8081/', status: 'live', latency_ms: 20, error: null },
    { endpoint_id: 'c', label: null, url: 'http://127.0.0.1:9/', status: 'die', latency_ms: 4, error: 'could not connect through the proxy endpoint' },
  ],
  live_count: 2,
  die_count: 1,
};

afterEach(() => vi.restoreAllMocks());

describe('proxy live check', () => {
  test('hides the chips until a check runs, then renders live and die counts', async () => {
    vi.spyOn(proxyApi, 'proxies').mockResolvedValue([proxy]);
    vi.spyOn(proxyApi, 'proxyOptions').mockResolvedValue({ proxies: [], next_cursor: null });
    const checkProxy = vi.spyOn(proxyApi, 'checkProxy');

    render(DashboardProxiesPage, { props: { tr, locale: 'en' } });
    await screen.findByText('Residential');

    expect(screen.queryByText('Live: 2')).toBeNull();
    expect(screen.queryByText('Die: 1')).toBeNull();

    let resolveCheck: ((response: ProxyCheckResponse) => void) | undefined;
    checkProxy.mockImplementationOnce(
      () => new Promise<ProxyCheckResponse>((resolve) => { resolveCheck = resolve; }),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Check live' }));

    const running = screen.getByRole('button', { name: /Checking…/ }) as HTMLButtonElement;
    expect(running.disabled).toBe(true);
    expect(checkProxy).toHaveBeenCalledWith('proxy-check');

    resolveCheck?.(checkResponse);
    expect(await screen.findByText('Live: 2')).toBeTruthy();
    expect(screen.getByText('Die: 1')).toBeTruthy();

    const idle = screen.getByRole('button', { name: 'Check live' }) as HTMLButtonElement;
    expect(idle.disabled).toBe(false);
  });

  test('a failed check keeps the previous chips and re-enables the button', async () => {
    vi.spyOn(proxyApi, 'proxies').mockResolvedValue([proxy]);
    vi.spyOn(proxyApi, 'proxyOptions').mockResolvedValue({ proxies: [], next_cursor: null });
    vi.spyOn(proxyApi, 'checkProxy').mockRejectedValueOnce(new Error('conflict'));

    render(DashboardProxiesPage, { props: { tr, locale: 'en' } });
    await screen.findByText('Residential');

    fireEvent.click(screen.getByRole('button', { name: 'Check live' }));
    expect(await screen.findByRole('button', { name: 'Check live' })).toBeTruthy();
    expect(screen.queryByText('Live: 2')).toBeNull();
    expect(screen.queryByText('Die: 1')).toBeNull();
  });
});
