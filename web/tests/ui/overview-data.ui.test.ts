import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardOverviewPage from '../../src/features/overview/DashboardOverviewPage.svelte';
import { api } from '../../src/lib/api';
import { statisticsFixture } from './support/statistics';
import type { Overview, Translate } from '../../src/lib/types';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const overviewFixture: Overview = {
  provider_count: 2,
  combo_count: 3,
  route_count: 3,
  request_count: 987,
  dropped_request_logs: 0,
  uptime_seconds: 1234,
};

function renderPage() {
  return render(DashboardOverviewPage, {
    props: {
      tr,
      locale: 'en',
      onNavigate: () => {},
      onCreateRequest: () => {},
      onConnectionChange: () => {},
      onProviderCountChange: () => {},
      onGatewayAddressChange: () => {},
    },
  });
}

function setupApi(statistics = statisticsFixture({ total_requests: 23, logged_requests: 40 })) {
  vi.spyOn(api, 'overview').mockResolvedValue(overviewFixture);
  vi.spyOn(api, 'statistics').mockResolvedValue(statistics);
  vi.spyOn(api, 'requests').mockResolvedValue({ requests: [], next_cursor: null, page_size: 50 });
  vi.spyOn(api, 'settings').mockResolvedValue({ host: '127.0.0.1', port: 8686 });
  const keys = vi.spyOn(api, 'apiKeys');
  return keys;
}

afterEach(() => vi.restoreAllMocks());

describe('overview data sources', () => {
  test('shows the 24-hour logical request total, not retained log rows, and does not fetch a partial key count', async () => {
    const keys = setupApi();
    renderPage();

    const requestsHeading = await screen.findByRole('heading', { name: 'Requests Today' });
    expect(requestsHeading.closest('article')?.textContent).toContain('23');
    expect(requestsHeading.closest('article')?.textContent).not.toContain('987');
    expect(keys).not.toHaveBeenCalled();
    expect(screen.getByText('Manage client API keys')).toBeTruthy();
  });

  test('does not render fabricated zero data when statistics loading fails', async () => {
    setupApi();
    vi.spyOn(api, 'statistics').mockRejectedValue(new Error('statistics offline'));
    renderPage();

    await waitFor(() => expect(screen.getByRole('heading', { name: 'Could not load overview' })).toBeTruthy());
    expect(screen.queryByText('Requests Today')).toBeNull();
  });

  test('marks incomplete retained history without changing the rollup total', async () => {
    setupApi(statisticsFixture({
      total_requests: 23,
      logged_requests: 9,
      status_history_incomplete: true,
    }));
    renderPage();

    const requestsHeading = await screen.findByRole('heading', { name: 'Requests Today' });
    expect(requestsHeading.closest('article')?.textContent).toContain('23');
    expect((await screen.findAllByText('Statistics are incomplete')).length).toBeGreaterThanOrEqual(1);
  });
});
