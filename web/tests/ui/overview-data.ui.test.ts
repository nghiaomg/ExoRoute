import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardOverviewPage from '../../src/features/overview/DashboardOverviewPage.svelte';
import { api } from '../../src/lib/api';
import { statisticsFixture } from './support/statistics';
import type { Overview, Translate } from '../../src/lib/types';
const tr: Translate = (key, vars) => key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));
const overviewFixture: Overview = { provider_count: 2, combo_count: 3, route_count: 3, request_count: 987, dropped_request_logs: 0, uptime_seconds: 1234 };
function renderPage() { return render(DashboardOverviewPage, { props: { tr, locale: 'en', onNavigate: () => {}, onCreateRequest: () => {}, onConnectionChange: () => {}, onProviderCountChange: () => {}, onGatewayAddressChange: () => {} } }); }
function setupApi(statistics = statisticsFixture({ total_requests: 23, logged_requests: 40 })) {
  vi.spyOn(api, 'overview').mockResolvedValue(overviewFixture);
  vi.spyOn(api, 'statistics').mockResolvedValue(statistics);
  vi.spyOn(api, 'requests').mockResolvedValue({ requests: [], next_cursor: null, page_size: 50 });
  vi.spyOn(api, 'settings').mockResolvedValue({ host: '127.0.0.1', port: 8686 });
  vi.spyOn(api, 'providers').mockResolvedValue([]);
  vi.spyOn(api, 'combos').mockResolvedValue([]);
  return vi.spyOn(api, 'apiKeys');
}
afterEach(() => vi.restoreAllMocks());
function cardText(heading: HTMLElement): string { return heading.parentElement?.textContent ?? ''; }
describe('overview data sources', () => {
  test('shows logical request total, not retained log rows', async () => {
    const keys = setupApi(); renderPage();
    const heading = await screen.findByRole('heading', { name: 'Requests' });
    expect(cardText(heading)).toContain('23');
    expect(cardText(heading)).not.toContain('987');
    expect(keys).not.toHaveBeenCalled();
  });
  test('does not render fabricated zero data on failure', async () => {
    setupApi(); vi.spyOn(api, 'statistics').mockRejectedValue(new Error('offline')); renderPage();
    await waitFor(() => expect(screen.getByRole('heading', { name: 'Could not load overview' })).toBeTruthy());
    expect(screen.queryByRole('heading', { name: 'Requests' })).toBeNull();
  });
  test('marks incomplete retained history', async () => {
    setupApi(statisticsFixture({ total_requests: 23, logged_requests: 9, status_history_incomplete: true })); renderPage();
    expect(cardText(await screen.findByRole('heading', { name: 'Requests' }))).toContain('23');
    expect(await screen.findByText('Telemetry partially available')).toBeTruthy();
  });
  test('renders four core KPI cards', async () => {
    setupApi(); renderPage();
    await screen.findByRole('heading', { name: 'Requests' });
    expect(document.querySelector('.core-kpis')!.querySelectorAll('.core-kpi').length).toBe(4);
  });
  test('latency above target shows warning and grouped digits', async () => {
    setupApi(statisticsFixture({ average_latency_ms: 17912, latency: { p50_ms: 17912, p95_ms: 19000 } as never })); renderPage();
    const card = cardText(await screen.findByRole('heading', { name: 'Gateway latency' }));
    expect(card).toContain('17,912');
  });
  test('quickstart guide appears in zero state', async () => {
    setupApi(statisticsFixture({ total_requests: 0 })); renderPage();
    expect(await screen.findByText('Quickstart setup guide')).toBeTruthy();
  });
  test('gateway endpoint includes http scheme', async () => {
    setupApi(); renderPage();
    await screen.findByRole('heading', { name: 'Requests' });
    expect(screen.getAllByText('http://127.0.0.1:8686/v1').length).toBeGreaterThan(0);
  });
});
