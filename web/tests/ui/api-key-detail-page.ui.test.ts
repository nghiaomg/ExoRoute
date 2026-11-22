import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import ApiKeyDetailPage from '../../src/features/api-keys/ApiKeyDetailPage.svelte';
import { api } from '../../src/lib/api';
import type { GatewayApiKey, Translate } from '../../src/lib/types';
import { statisticsFixture } from './support/statistics';

const tr: Translate = (key, vars) => key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));
const key: GatewayApiKey = { id: 'key-detail', name: 'cloud', enabled: true, created_at: '2026-11-10 17:43:00', last_used_at: '2026-11-17 19:54:00' };
const stats = statisticsFixture({
  total_requests: 2083, successes: 2068, failures: 15, success_rate: 99.28,
  input_tokens: 12000, output_tokens: 4000, cached_tokens: 3000,
  models: { top: [{ id: 'gpt-4o', label: 'gpt-4o', requests: 1800, percentage: 86.4 }], others: 283 },
  providers: { rows: [
    { provider: 'openai', requests: 1500, successes: 1490, success_rate: 99.3, average_latency_ms: 220, input_tokens: 9000, output_tokens: 3000, cost_micro_usd: 0, cost_reported_micro_usd: 0, cost_events: 0, missing_cost_events: 0 },
    { provider: 'anthropic', requests: 583, successes: 578, success_rate: 99.1, average_latency_ms: 260, input_tokens: 3000, output_tokens: 1000, cost_micro_usd: 0, cost_reported_micro_usd: 0, cost_events: 0, missing_cost_events: 0 },
  ], truncated: false },
  series: { points: [{ start: '2026-11-18 00:00:00', requests: 30, successes: 29, failures: 1, input_tokens: 10, output_tokens: 5, average_latency_ms: 100, cost_micro_usd: null, cost_reported_micro_usd: 0, cost_events: 0, missing_cost_events: 0 }], bucket_seconds: 3600, truncated: false },
}) as never;

function renderPage() {
  return render(ApiKeyDetailPage, { props: {
    apiKey: key, keyId: key.id, loading: false, error: '', tr, locale: 'en',
    onBack: vi.fn(), onEdit: vi.fn(), onToggle: vi.fn(), onNavigate: vi.fn(), onRetry: vi.fn(),
  } });
}
afterEach(() => vi.restoreAllMocks());

describe('API key detail page', () => {
  test('loads per-key statistics and renders three donut charts', async () => {
    vi.spyOn(api, 'apiKeyStatistics').mockResolvedValue(stats);
    renderPage();
    expect(await screen.findByRole('heading', { name: 'Request outcomes' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Token mix' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Model distribution' })).toBeTruthy();
    expect(screen.getAllByRole('img').length).toBeGreaterThanOrEqual(3);
    expect(api.apiKeyStatistics).toHaveBeenCalledWith('key-detail', '1d', expect.anything());
  });
  test('range buttons reload statistics', async () => {
    const spy = vi.spyOn(api, 'apiKeyStatistics').mockResolvedValue(stats);
    renderPage();
    await screen.findByRole('heading', { name: 'Request outcomes' });
    await fireEvent.click(screen.getByRole('button', { name: '7 days' }));
    await waitFor(() => expect(spy).toHaveBeenCalledWith('key-detail', '7d', expect.anything()));
  });
  test('renders token stat cards, activity heatmap and usage ranking', async () => {
    vi.spyOn(api, 'apiKeyStatistics').mockResolvedValue(stats);
    renderPage();
    expect(await screen.findByRole('heading', { name: 'Activity heatmap' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Usage ranking' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'By provider' })).toBeTruthy();
    expect(screen.getByRole('img', { name: 'Activity heatmap' })).toBeTruthy();
    expect(screen.getAllByText('Token usage').length).toBeGreaterThan(0);
  });
  test('renders a safe empty state when statistics fail', async () => {
    vi.spyOn(api, 'apiKeyStatistics').mockRejectedValue(new Error('offline'));
    renderPage();
    expect(await screen.findByRole('alert')).toBeTruthy();
    expect(screen.queryByRole('heading', { name: 'Request outcomes' })).toBeNull();
  });
});
