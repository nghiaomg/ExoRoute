import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardRequestsPage from '../../src/features/requests/DashboardRequestsPage.svelte';
import { api } from '../../src/lib/api';
import type { RequestLiveEvent, RequestLog, RequestLogPage } from '../../src/lib/types';
import type { Translate } from '../../src/lib/format';
import { statisticsFixture } from './support/statistics';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

function page(requests: RequestLog[] = []): RequestLogPage {
  return { requests, next_cursor: null, page_size: 50 };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('dashboard requests live statistics', () => {
  test('finished SSE events refresh latency and all aggregate stat cards, coalescing bursts', async () => {
    let emit: ((event: RequestLiveEvent) => void) | null = null;
    const requests: RequestLog[] = [];
    let statisticsCalls = 0;

    vi.spyOn(api, 'requests').mockImplementation(async () => page(requests));
    vi.spyOn(api, 'providers').mockResolvedValue([]);
    vi.spyOn(api, 'statistics').mockImplementation(async () => {
      statisticsCalls += 1;
      return statisticsFixture(statisticsCalls === 1
        ? {
            total_requests: 10,
            successes: 8,
            failures: 2,
            success_rate: 80,
            average_latency_ms: 300,
            input_tokens: 2_500,
            output_tokens: 500,
            cached_tokens: 100,
          }
        : {
            total_requests: 12,
            successes: 10,
            failures: 2,
            success_rate: 83.3,
            average_latency_ms: 725,
            input_tokens: 3_100,
            output_tokens: 650,
            cached_tokens: 200,
          });
    });
    vi.spyOn(api, 'streamRequestLive').mockImplementation(async (onEvent) => {
      emit = onEvent;
    });

    const { container } = render(DashboardRequestsPage, {
      props: { tr, locale: 'en', onConnectionChange: () => {} },
    });

    await waitFor(() => expect(statisticsCalls).toBe(1));
    await waitFor(() => expect(statCardValue(container, 'Requests')).toBe('10'));
    expect(statCardValue(container, 'Avg Latency')).toBe('300 ms');
    expect(statCardValue(container, 'Requests')).toBe('10');

    const finish = (id: string, duration: number): RequestLiveEvent => ({
      type: 'finished',
      live_id: id,
      finished_at_ms: Date.now(),
      active_count: 0,
      request: {
        id: `log-${id}`,
        model: 'upstream/model',
        route_alias: 'combo/served-model',
        created_at: new Date().toISOString(),
        status: 200,
        duration_ms: duration,
        input_tokens: 100,
        output_tokens: 20,
        reasoning_effort: 'high',
      },
    });
    expect(emit).toBeTruthy();
    emit?.(finish('finished-1', 700));
    emit?.(finish('finished-2', 750));

    await waitFor(() => expect(statisticsCalls).toBe(2), { timeout: 6000 });
    await waitFor(() => {
      const request = screen.getAllByText('upstream/model')[0].closest('tr');
      expect(request?.textContent).toContain('combo/served-model');
      expect(request?.textContent).toContain('high');
      expect(statCardValue(container, 'Avg Latency')).toBe('725 ms');
    });
    expect(statCardValue(container, 'Requests')).toBe('12');
    expect(statCardValue(container, 'Success Rate')).toBe('83.3%');
    expect(statCardValue(container, 'Failed')).toBe('2');
    expect(statCardValue(container, 'Input tokens')).toBe('3.1K');
    expect(statCardValue(container, 'Output tokens')).toBe('650');
    expect(statCardValue(container, 'Cached tokens')).toBe('200');
    expect(statisticsCalls).toBe(2);
  });
});

function statCardValue(container: HTMLElement, heading: string): string | undefined {
  const title = Array.from(container.querySelectorAll('h2'))
    .find((node) => node.textContent?.trim() === heading);
  return title?.parentElement?.querySelector('[data-stat-value]')?.textContent?.trim();
}
