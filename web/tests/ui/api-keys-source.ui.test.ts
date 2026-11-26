import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardApiKeysPage from '../../src/features/api-keys/DashboardApiKeysPage.svelte';
import ApiKeyInspector from '../../src/features/api-keys/ApiKeyInspector.svelte';
import { api } from '../../src/lib/api';
import type { GatewayApiKey, Translate } from '../../src/lib/types';
import { statisticsFixture } from './support/statistics';
import { connectSnippet, filterAndSortKeys, idleDays, isStale, KEY_PLACEHOLDER } from '../../src/features/api-keys/api-keys.data';

const tr: Translate = (key, vars) => key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const keys: GatewayApiKey[] = [
  { id: 'key-alpha', name: 'hao', enabled: true, created_at: '2026-11-01 00:00:00', last_used_at: '2026-11-16 19:42:18', scope_restricted: true, allowed_models: ['cx/gpt-6-luna', 'kc/particle-ai/particle-1:free', 'third/model'] },
  { id: 'key-bravo', name: 'exc', enabled: true, created_at: '2026-10-01 00:00:00', last_used_at: '2026-10-23 03:25:11' },
  { id: 'key-charlie', name: 'key', enabled: false, created_at: '2026-09-01 00:00:00' },
];

function setup() {
  vi.spyOn(api, 'apiKeys').mockResolvedValue({ api_keys: keys, next_cursor: null });
  vi.spyOn(api, 'statistics').mockResolvedValue(statisticsFixture({ total_requests: 11416, failures: 449, success_rate: 96.1, input_tokens: 1000, output_tokens: 500, cached_tokens: 200 }) as never);
  vi.spyOn(api, 'providers').mockResolvedValue([]);
  vi.spyOn(api, 'apiKeyStatistics').mockImplementation(async (id) => statisticsFixture({ total_requests: id === 'key-alpha' ? 9000 : 10, failures: id === 'key-alpha' ? 449 : 0 }) as never);
  return render(DashboardApiKeysPage, { props: { tr, locale: 'en', onNavigate: () => {} } });
}
afterEach(() => vi.restoreAllMocks());

describe('API-key governance', () => {
  test('KPI strip no longer duplicates the page-count helper text', async () => {
    setup();
    await screen.findByRole('button', { name: 'hao' });
    expect(screen.queryAllByText(/Showing 3 API keys on this page/)).toHaveLength(0);
    expect(screen.getAllByText('Active').length).toBeGreaterThan(0);
    expect(screen.getAllByText(/Stale/).length).toBeGreaterThan(0);
  });
  test('stale detection uses a 14-day idle threshold', () => {
    const now = new Date('2026-11-16T20:00:00Z').valueOf();
    expect(isStale(keys[0], now)).toBe(false);
    expect(isStale(keys[1], now)).toBe(true);
    expect(idleDays(keys[2], now)).toBeNull();
  });
  test('per-key telemetry is joined from per-key statistics endpoint', async () => {
    setup(); await screen.findByRole('button', { name: 'hao' });
    await waitFor(() => expect(screen.getAllByText(/9,000/).length).toBeGreaterThan(0));
    expect(api.apiKeyStatistics).toHaveBeenCalledWith('key-alpha', '1d', expect.anything());
  });
  test('search, status and scope filters and sorting are applied', () => {
    const metrics = { 'key-alpha': { requests: 9000, tokens: 100, failures: 449, successRate: 95, loaded: true, error: false } };
    expect(filterAndSortKeys(keys, { query: 'exc', status: 'all', scope: 'all', sort: 'name' }, metrics).map((k) => k.name)).toEqual(['exc']);
    expect(filterAndSortKeys(keys, { query: '', status: 'erroring', scope: 'all', sort: 'traffic' }, metrics).map((k) => k.name)).toEqual(['hao']);
    expect(filterAndSortKeys(keys, { query: '', status: 'stale', scope: 'all', sort: 'traffic' }, metrics).map((k) => k.name)).toEqual(['exc']);
    expect(filterAndSortKeys(keys, { query: '', status: 'all', scope: 'any', sort: 'traffic' }, metrics).map((k) => k.name)).toEqual(['exc', 'key']);
  });
  test('quick-connect snippets always use the placeholder, never a secret', () => {
    for (const kind of ['curl', 'openai', 'anthropic'] as const) {
      const snippet = connectSnippet(kind, 'gpt-4o');
      expect(snippet).toContain(KEY_PLACEHOLDER);
      expect(snippet).toContain('http://127.0.0.1:8686/v1');
      expect(snippet).not.toMatch(/sk-[A-Za-z0-9]{12,}/);
    }
  });
  test('the key row opens its detail route, and the inspector exposes quick-connect tabs', async () => {
    const onNavigate = vi.fn();
    vi.spyOn(api, 'apiKeys').mockResolvedValue({ api_keys: keys, next_cursor: null });
    vi.spyOn(api, 'statistics').mockResolvedValue(statisticsFixture() as never);
    vi.spyOn(api, 'providers').mockResolvedValue([]);
    vi.spyOn(api, 'apiKeyStatistics').mockImplementation(async (id) => statisticsFixture({ total_requests: id === 'key-alpha' ? 9000 : 10 }) as never);
    render(DashboardApiKeysPage, { props: { tr, locale: 'en', onNavigate } });
    const button = await screen.findByRole('button', { name: 'hao' });
    await fireEvent.click(button);
    expect(onNavigate).toHaveBeenCalledWith('api-key-detail:key-alpha');

    // The inspector is a presentational dialog: assert its quick-connect contract directly.
    render(ApiKeyInspector, { props: { tr, locale: 'en', apiKey: keys[0], metrics: { requests: 9000, tokens: 100, failures: 449, successRate: 95, loaded: true, error: false }, onClose: () => {}, onNavigate: () => {} } });
    expect(await screen.findByRole('heading', { name: 'Quick connect' })).toBeTruthy();
    expect(screen.getByRole('tab', { name: 'cURL' })).toBeTruthy();
  });
});
