import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardCombosPage from '../../src/features/combos/DashboardCombosPage.svelte';
import { api } from '../../src/lib/api';
import { statisticsFixture } from './support/statistics';
import type { GatewayCombo, Translate } from '../../src/lib/types';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

function combo(name: string): GatewayCombo {
  return {
    id: `combo-${name}`,
    name,
    strategy: 'priority',
    accepted_protocols: ['chat_completions'],
    enabled: true,
    targets: [{ provider_id: 'provider-a', model: 'shared/model', priority: 1, enabled: true }],
  };
}

function renderPage() {
  return render(DashboardCombosPage, {
    props: {
      tr,
      locale: 'en',
      onNavigate: () => {},
      onConnectionChange: () => {},
      onProviderCountChange: () => {},
    },
  });
}

afterEach(() => vi.restoreAllMocks());

describe('combo usage attribution', () => {
  test('uses the route alias breakdown instead of a same-named model breakdown', async () => {
    vi.spyOn(api, 'combos').mockResolvedValue([combo('shared/model')]);
    vi.spyOn(api, 'comboProviderOptions').mockResolvedValue({ providers: [], next_cursor: null });
    vi.spyOn(api, 'statistics').mockResolvedValue(statisticsFixture({
      model_breakdown: [{
        model: 'shared/model', provider: 'provider-a', requests: 99, successes: 0,
        success_rate: 0, input_tokens: 0, output_tokens: 0, cache_hit_rate: 0,
      }],
      route_breakdown: [{
        route_alias: 'shared/model', requests: 5, successes: 4, failures: 1, success_rate: 80,
      }],
      route_breakdown_truncated: false,
      route_breakdown_incomplete: false,
    }));

    renderPage();

    const requestsCard = await screen.findByRole('heading', { name: 'Requests Routed' });
    expect(requestsCard.closest('article')?.textContent).toContain('5');
    expect(requestsCard.closest('article')?.textContent).not.toContain('99');

    const comboCard = document.querySelector('.combo-card');
    expect(comboCard?.textContent).toContain('5 requests');
    expect(comboCard?.textContent).toContain('80% success');
    expect(comboCard?.textContent).toContain('1 failed');
  });

  test('does not report zero traffic when statistics are unavailable or incomplete', async () => {
    vi.spyOn(api, 'combos').mockResolvedValue([combo('combo-with-old-history')]);
    vi.spyOn(api, 'comboProviderOptions').mockResolvedValue({ providers: [], next_cursor: null });
    vi.spyOn(api, 'statistics').mockResolvedValue(statisticsFixture({
      route_breakdown: [],
      route_breakdown_truncated: false,
      route_breakdown_incomplete: true,
    }));

    renderPage();

    const requestsCard = await screen.findByRole('heading', { name: 'Requests Routed' });
    expect(requestsCard.closest('article')?.textContent).toContain('—');
    expect(requestsCard.closest('article')?.textContent).toContain('Statistics are incomplete');
    expect(document.querySelector('.combo-card')?.textContent).toContain('Statistics are incomplete');
    expect(document.querySelector('.combo-card')?.textContent).not.toContain('0 failed');
  });

  test('labels an empty route breakdown as no traffic only when the range is complete', async () => {
    vi.spyOn(api, 'combos').mockResolvedValue([combo('unused-route')]);
    vi.spyOn(api, 'comboProviderOptions').mockResolvedValue({ providers: [], next_cursor: null });
    vi.spyOn(api, 'statistics').mockResolvedValue(statisticsFixture({
      route_breakdown: [],
      route_breakdown_truncated: false,
      route_breakdown_incomplete: false,
    }));

    renderPage();

    await waitFor(() => expect(document.querySelector('.combo-card')?.textContent).toContain('no traffic yet'));
    expect(document.querySelector('.combo-card')?.textContent).toContain('0 failed');
  });
});
