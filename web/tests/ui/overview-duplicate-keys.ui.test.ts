import { render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardOverviewPage from '../../src/features/overview/DashboardOverviewPage.svelte';
import { api } from '../../src/lib/api';
import { proxyApi } from '../../src/lib/api/proxies';
import { statisticsFixture } from './support/statistics';
import type { Overview, Translate } from '../../src/lib/types';

const tr: Translate = (key, vars) => key.replace(/\{(\w+)\}/g, (_, n) => String(vars?.[n] ?? '{'+n+'}'));
const overviewFixture: Overview = { provider_count: 2, combo_count: 3, route_count: 3, request_count: 987, dropped_request_logs: 0, uptime_seconds: 1234 };

afterEach(() => vi.restoreAllMocks());

function setup(series, requests) {
  vi.spyOn(api, 'overview').mockResolvedValue(overviewFixture);
  vi.spyOn(api, 'statistics').mockResolvedValue(statisticsFixture({ series, total_requests: 10, logged_requests: 10 }));
  vi.spyOn(api, 'requests').mockResolvedValue({ requests, next_cursor: null, page_size: 50 });
  vi.spyOn(api, 'settings').mockResolvedValue({ host: '127.0.0.1', port: 8686 });
  vi.spyOn(api, 'providers').mockResolvedValue([]);
  vi.spyOn(api, 'combos').mockResolvedValue([]);
  vi.spyOn(proxyApi, 'proxies').mockResolvedValue([]);
}

const point = (start, requests) => ({ start, end: start, requests, succeeded: requests, failed: 0, input_tokens: 0, output_tokens: 0, cached_tokens: 0, cache_input_tokens: 0, duration_ms: 0, cost_micro_usd: 0, cost_events: 0, missing_cost_events: 0, cache_ratio: 0, average_latency_ms: 0 });

describe('overview duplicate keys', () => {
  test('series points with repeated labels and values do not crash', async () => {
    const s = [point('2026-11-19 10:00:00', 0), point('2026-11-19 11:00:00', 0), point('2026-11-19 12:00:00', 0)];
    setup({ points: s, granularity: 'hour' }, []);
    render(DashboardOverviewPage, { props: { tr, locale: 'en', onNavigate: () => {}, onCreateRequest: () => {}, onConnectionChange: () => {}, onProviderCountChange: () => {}, onGatewayAddressChange: () => {} } });
    expect(await screen.findByRole('heading', { name: 'Requests' })).toBeTruthy();
  });

  test('model breakdown rows that share a model name do not crash', async () => {
    setup({ points: [], granularity: 'hour' }, []);
    vi.mocked(api.statistics).mockResolvedValue(statisticsFixture({
      total_requests: 15,
      logged_requests: 15,
      model_breakdown: [{"model":"shared/model","provider":"provider-0","input_tokens":10,"output_tokens":2,"cache_hit_rate":0,"requests":5,"successes":5,"success_rate":100},{"model":"shared/model","provider":"provider-1","input_tokens":10,"output_tokens":2,"cache_hit_rate":0,"requests":5,"successes":5,"success_rate":100},{"model":"shared/model","provider":"provider-2","input_tokens":10,"output_tokens":2,"cache_hit_rate":0,"requests":5,"successes":5,"success_rate":100}],
    }));
    render(DashboardOverviewPage, { props: { tr, locale: 'en', onNavigate: () => {}, onCreateRequest: () => {}, onConnectionChange: () => {}, onProviderCountChange: () => {}, onGatewayAddressChange: () => {} } });
    expect(await screen.findByRole('heading', { name: 'Requests' })).toBeTruthy();
    expect(await screen.findByText('Top models')).toBeTruthy();
  });
  test('request rows sharing created_at do not crash', async () => {
    const req = (i) => ({ id: undefined, request_id: undefined, model: 'm', created_at: '2026-11-19 10:00:00', status: 200, duration_ms: 1, input_tokens: 1, output_tokens: 1 });
    setup({ points: [], granularity: 'hour' }, [req(1), req(2), req(3)]);
    render(DashboardOverviewPage, { props: { tr, locale: 'en', onNavigate: () => {}, onCreateRequest: () => {}, onConnectionChange: () => {}, onProviderCountChange: () => {}, onGatewayAddressChange: () => {} } });
    expect(await screen.findByRole('heading', { name: 'Requests' })).toBeTruthy();
  });
});
