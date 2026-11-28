import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { render, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import OverviewCharts from '../../src/features/overview/OverviewCharts.svelte';
import OverviewTopModels from '../../src/features/overview/OverviewTopModels.svelte';
import { statisticsFixture } from './support/statistics';
import type { RequestLog, Translate } from '../../src/lib/types';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

afterEach(() => vi.restoreAllMocks());

describe('overview top models and cache ratio', () => {
  test('fills the panel with ranked models, tokens and sampled latency', async () => {
    const stats = statisticsFixture({
      total_requests: 100,
      model_breakdown: [
        { model: 'small-model', provider: 'provider-b', requests: 30, input_tokens: 300, output_tokens: 100, cache_hit_rate: 0, successes: 30, success_rate: 100 },
        { model: 'top-model', provider: 'provider-a', requests: 70, input_tokens: 1_500_000, output_tokens: 50_000, cache_hit_rate: 94, successes: 70, success_rate: 100 },
      ],
    });
    const recent: RequestLog[] = [
      { model: 'top-model', provider_id: 'provider-a', duration_ms: 200, created_at: '2026-11-26 10:00:00' },
      { model: 'top-model', provider_id: 'provider-a', duration_ms: 400, created_at: '2026-11-26 10:01:00' },
    ];
    const navigate = vi.fn();
    render(OverviewTopModels, { props: { tr, locale: 'en', stats, recent, onNavigate: navigate } });
    const section = await waitFor(() => {
      const panel = document.querySelector('section[aria-label="Top models"]') as HTMLElement | null;
      expect(panel).toBeTruthy();
      expect(panel!.querySelectorAll('.model-row').length).toBe(2);
      return panel!;
    });
    const rows = section.querySelectorAll('.model-row');
    expect(rows[0].textContent).toContain('top-model');
    expect(rows[0].textContent).toContain('provider-a');
    expect(rows[0].textContent).toContain('70%');
    expect(rows[0].textContent).toContain('1.55M');
    expect(rows[0].textContent).toContain('300 ms');
    expect(rows[1].textContent).toContain('small-model');
    expect((rows[0].querySelector('.share-track i') as HTMLElement).getAttribute('style')).toMatch(/width:\s*70%/);
    expect(section.querySelector('.mini-add')?.textContent).toBe('View requests');
  });

  test('shows a clean empty state when no model traffic exists', async () => {
    render(OverviewTopModels, {
      props: {
        tr, locale: 'en', recent: [], stats: statisticsFixture({ total_requests: 0, model_breakdown: [] }), onNavigate: () => {},
      },
    });
    const section = await waitFor(() => {
      const panel = document.querySelector('section[aria-label="Top models"]') as HTMLElement | null;
      expect(panel).toBeTruthy();
      return panel!;
    });
    expect(section.textContent).toContain('No requests recorded yet');
  });

  test('renders the cache ratio once as a percentage and clamps it', async () => {
    const ratio = (1_470_000_000 / 1_550_000_000) * 100;
    render(OverviewCharts, {
      props: {
        tr, locale: 'en', health: null,
        stats: statisticsFixture({ cache_ratio: ratio, input_tokens: 1_550_000_000, cached_tokens: 1_470_000_000 }),
      },
    });
    await waitFor(() => expect(document.querySelectorAll('.mini-card svg text').length).toBeGreaterThan(0));
    const centerTexts = [...document.querySelectorAll('.mini-card svg text')].map((node) => node.textContent);
    expect(centerTexts).toContain('94.8%');
    expect(centerTexts.join(' ')).not.toContain('9455');
  });

  test('keeps the added card flat and token-driven', () => {
    const component = readFileSync(resolve(process.cwd(), 'src/features/overview/OverviewTopModels.svelte'), 'utf8');
    const charts = readFileSync(resolve(process.cwd(), 'src/features/overview/OverviewCharts.svelte'), 'utf8');
    expect(component).toContain('class="recent-card"');
    expect(component).toContain('var(--border-subtle, var(--line))');
    expect(component).toContain('var(--surface-sunken)');
    expect(component).toContain('var(--accent)');
    expect(component).not.toMatch(/box-shadow|!important|#[0-9a-fA-F]{3,8}\b|rgba?\(/);
    expect(charts).not.toContain('stats.cache_ratio * 1000');
  });
});
