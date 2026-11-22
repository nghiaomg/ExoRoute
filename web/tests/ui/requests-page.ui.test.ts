import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardRequestsPage from '../../src/features/requests/DashboardRequestsPage.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type {
  Provider,
  RequestLiveEvent,
  RequestLiveRow,
  RequestLog,
  RequestLogPage,
  RequestStatistics,
} from '../../src/lib/types';
import { statisticsFixture } from './support/statistics';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

function page(requests: RequestLog[]): RequestLogPage {
  return { requests, next_cursor: null, page_size: 50 };
}

// The production page filters rows to a rolling 24-hour window, so fixture
// timestamps must stay fresh.
const finishedRows: RequestLog[] = [
  {
    id: 'log-success',
    model: 'ocg/minimax-m3',
    provider_id: 'p-ocg',
    api_key_name: 'Main key',
    created_at: new Date(Date.now() - 5 * 60_000).toISOString(),
    status: 200,
    duration_ms: 100,
    input_tokens: 1000,
    output_tokens: 500,
    cached_tokens: 50,
  },
  {
    id: 'log-warning',
    model: 'openai/gpt-5',
    provider_id: 'p-openai',
    created_at: new Date(Date.now() - 4 * 60_000).toISOString(),
    status: 404,
    duration_ms: 9000,
    error: 'upstream 404: not found',
  },
  {
    id: 'log-critical',
    model: 'anthropic/claude',
    provider_id: 'p-anthropic',
    created_at: new Date(Date.now() - 3 * 60_000).toISOString(),
    status: 500,
    duration_ms: 5000,
    error: 'provider exploded',
  },
];

const live: RequestLiveRow = {
  live: true,
  live_id: 'live-1',
  started_at_ms: Date.now(),
  model: 'live/preview-model',
  created_at: new Date().toISOString(),
};

type Feed = { emit: (event: RequestLiveEvent) => void; signals: AbortSignal[] };

// The production page subscribes from a Svelte effect; emit the snapshot in a
// microtask so state writes never happen inside the effect itself. Each
// subscription is recorded so the Live toggle's abort can be asserted.
function mockLiveFeed(rows: RequestLiveRow[] = [live]): Feed {
  let emit: ((event: RequestLiveEvent) => void) | null = null;
  const signals: AbortSignal[] = [];
  vi.spyOn(api, 'streamRequestLive').mockImplementation((onEvent, signal) => {
    emit = onEvent;
    if (signal) signals.push(signal);
    queueMicrotask(() => {
      onEvent({ type: 'snapshot', requests: rows, truncated: false, limit: 50, active_count: rows.length });
    });
    return Promise.resolve();
  });
  return { emit: (event) => emit?.(event), signals };
}

function provider(id: string, name: string): Provider {
  return {
    id,
    name,
    adapter_id: 'generic',
    base_url: 'http://127.0.0.1:9/v1',
    enabled: true,
    auth_type: 'none',
    preferred_protocol: 'chat_completions',
    supported_protocols: ['chat_completions'],
  };
}

const defaultProviders = [
  provider('p-ocg', 'OpenCode Go'),
  provider('p-openai', 'OpenAI'),
  provider('p-anthropic', 'Anthropic'),
];

type RenderOptions = {
  providers?: Provider[];
  statistics?: (call: number) => RequestStatistics;
};

function renderPage(options: RenderOptions = {}): ReturnType<typeof render> {
  vi.spyOn(api, 'providers').mockResolvedValue(options.providers ?? defaultProviders);
  let statisticsCalls = 0;
  vi.spyOn(api, 'statistics').mockImplementation(async () =>
    options.statistics
      ? options.statistics(++statisticsCalls)
      : statisticsFixture({ total_requests: 3, successes: 1, failures: 2, success_rate: 33.3 }),
  );
  return render(DashboardRequestsPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });
}

/** Read a StatsCard KPI by its title. */
function statValue(container: HTMLElement, title: string): string | undefined {
  const heading = Array.from(container.querySelectorAll('h2')).find(
    (node) => node.textContent?.trim() === title,
  );
  return heading?.previousElementSibling?.textContent?.trim();
}

function skeletons(container: HTMLElement): number {
  return container.querySelectorAll('.stats-grid > div[aria-hidden="true"]').length;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('production requests page', () => {
  test('shows a skeleton on first load, then page-scoped KPIs and tiered rows', async () => {
    mockLiveFeed([]);
    let resolveRequests: (value: RequestLogPage) => void = () => {};
    const pending = new Promise<RequestLogPage>((resolve) => { resolveRequests = resolve; });
    vi.spyOn(api, 'requests').mockReturnValue(pending);

    const { container } = renderPage();

    await waitFor(() => expect(skeletons(container)).toBe(4));
    resolveRequests(page(finishedRows));

    await screen.findByText('anthropic/claude');
    expect(skeletons(container)).toBe(0);

    // The strip is page-scoped: the three finished rows from this response.
    expect(container.textContent).toContain('Showing 3 of 3 requests');
    expect(container.textContent).toContain('page success 33%');
    expect(statValue(container, 'Requests')).toBe('3');
    expect(statValue(container, 'Success Rate')).toBe('33.3%');

    expect(container.querySelectorAll('tbody tr').length).toBe(3);

    // HTTP tiers: 2xx success, 4xx warning, 5xx critical.
    const successRow = screen.getByText('ocg/minimax-m3').closest('tr');
    const warningRow = screen.getByText('openai/gpt-5').closest('tr');
    const criticalRow = screen.getByText('anthropic/claude').closest('tr');
    expect(successRow?.querySelector('.status')?.classList.contains('complete')).toBe(true);
    expect(warningRow?.querySelector('.status')?.classList.contains('progress')).toBe(true);
    expect(criticalRow?.querySelector('.status')?.classList.contains('error')).toBe(true);

    // Duration tiers and the cached-token badge come from the row data.
    expect(successRow?.textContent).toContain('100 ms');
    expect(warningRow?.textContent).toContain('9.0 s');
    expect(criticalRow?.textContent).toContain('5.0 s');
    expect(successRow?.textContent).toContain('Cache: 50');
  });

  test('clicking a row opens the detail dialog with its error and shell close hook', async () => {
    mockLiveFeed();
    vi.spyOn(api, 'requests').mockResolvedValue(page(finishedRows));
    const { container } = renderPage();

    const criticalRow = (await screen.findByText('anthropic/claude')).closest('tr') as HTMLElement;
    expect(criticalRow.querySelector('.req-error-trigger-btn')).toBeNull();

    await fireEvent.click(criticalRow);
    const dialog = await screen.findByRole('dialog');
    expect(dialog.getAttribute('aria-labelledby')).toBe('req-dialog-title');
    expect(dialog.textContent).toContain('Request detail');
    expect(dialog.textContent).toContain('Request info');
    expect(dialog.textContent).toContain('provider exploded');
    expect((dialog as HTMLDialogElement).open).toBe(true);
    expect(container.querySelector('.req-error-note')?.textContent).toBe('provider exploded');

    // Closing is delegated to the production shell via data-close-modal; the
    // page's contract is to expose that hook on a button inside the dialog.
    const closeButton = screen.getByRole('button', { name: 'Close' });
    expect(closeButton.getAttribute('data-close-modal')).toBe('true');
    expect(dialog.contains(closeButton)).toBe(true);
    await fireEvent.click(closeButton);
    expect((dialog as HTMLDialogElement).open).toBe(true);
  });

  test('the live feed marks in-flight rows and the Live switch freezes the table', async () => {
    const feed = mockLiveFeed();
    vi.spyOn(api, 'requests').mockResolvedValue(page(finishedRows));
    const { container } = renderPage();

    await screen.findByText('live/preview-model');
    expect(statValue(container, 'Active now')).toBe('1');
    expect(container.querySelectorAll('tbody tr').length).toBe(4);
    const liveRow = screen.getByText('live/preview-model').closest('tr');
    expect(liveRow?.querySelector('.status')?.classList.contains('progress')).toBe(true);
    expect(liveRow?.textContent).toContain('now');
    expect(liveRow?.textContent).toContain('LIVE');

    const toggle = container.querySelector('input[type="checkbox"]') as HTMLInputElement;
    expect(toggle.checked).toBe(true);
    await fireEvent.click(toggle);
    // Turning Live off aborts the SSE subscription: no more rows can arrive.
    expect(toggle.checked).toBe(false);
    expect(feed.signals.length).toBe(1);
    expect(feed.signals[0]?.aborted).toBe(true);

    await fireEvent.click(toggle);
    await waitFor(() => expect(feed.signals.length).toBe(2));
    feed.emit({ type: 'started', request: { ...live, live_id: 'live-2', model: 'live/second-model' }, active_count: 2 });
    await waitFor(() => expect(container.querySelectorAll('tbody tr').length).toBe(5));
    expect(statValue(container, 'Active now')).toBe('2');
  });

  test('the toolbar search and status filter narrow and restore the rows', async () => {
    mockLiveFeed([]);
    vi.spyOn(api, 'requests').mockResolvedValue(page(finishedRows));
    const { container } = renderPage();

    await screen.findByText('anthropic/claude');
    const search = screen.getByPlaceholderText('Search model, key, provider…');
    await fireEvent.input(search, { target: { value: 'gpt-5' } });
    await waitFor(() => expect(container.querySelectorAll('tbody tr').length).toBe(1));
    expect(screen.getByText('openai/gpt-5')).toBeTruthy();

    await fireEvent.input(search, { target: { value: '' } });
    await waitFor(() => expect(container.querySelectorAll('tbody tr').length).toBe(3));

    const status = screen.getByLabelText('Filter by status');
    await fireEvent.change(status, { target: { value: 'failure' } });
    await waitFor(() => expect(container.querySelectorAll('tbody tr').length).toBe(2));
    expect(screen.queryByText('ocg/minimax-m3')).toBeNull();

    await fireEvent.change(status, { target: { value: 'all' } });
    await waitFor(() => expect(container.querySelectorAll('tbody tr').length).toBe(3));
  });

  test('the provider field offers loaded providers and filters to the selected one', async () => {
    mockLiveFeed([]);
    const requests = vi.spyOn(api, 'requests').mockImplementation(async (filters) =>
      page(filters.provider_id ? finishedRows.filter((row) => row.provider_id === filters.provider_id) : finishedRows),
    );
    const { container } = renderPage();

    await screen.findByText('anthropic/claude');
    const select = screen.getByLabelText('Filter by provider');
    expect(Array.from(select.querySelectorAll('option')).map((option) => option.textContent)).toEqual([
      'Select…',
      'All providers',
      'OpenCode Go',
      'OpenAI',
      'Anthropic',
    ]);

    await fireEvent.change(select, { target: { value: 'p-openai' } });
    await waitFor(() => expect(requests).toHaveBeenLastCalledWith({ provider_id: 'p-openai' }));
    await waitFor(() => expect(container.querySelectorAll('tbody tr').length).toBe(1));
    expect(screen.getByText('openai/gpt-5')).toBeTruthy();

    // Clearing the filter restores every provider without a remount.
    await fireEvent.change(select, { target: { value: 'all' } });
    await waitFor(() => expect(requests).toHaveBeenLastCalledWith({}));
    await waitFor(() => expect(container.querySelectorAll('tbody tr').length).toBe(3));
  });

  test('the live feed renders cached tokens when a finished event arrives', async () => {
    const feed = mockLiveFeed([]);
    vi.spyOn(api, 'requests').mockResolvedValue(page([]));
    const { container } = renderPage();

    await waitFor(() => expect(skeletons(container)).toBe(0));

    feed.emit({
      type: 'finished',
      live_id: 'live-finish-1',
      finished_at_ms: Date.now(),
      active_count: 0,
      request: {
        id: 'log-finished-cached',
        model: 'openai/gpt-4o',
        created_at: new Date().toISOString(),
        status: 200,
        duration_ms: 1500,
        input_tokens: 25000,
        output_tokens: 300,
        cached_tokens: 18000,
      },
    });

    await screen.findByText('openai/gpt-4o');
    const row = screen.getByText('openai/gpt-4o').closest('tr');
    expect(row?.textContent).toContain('Cache: 18,000');
  });

  test('a finished live event silently refetches the traffic summary', async () => {
    const feed = mockLiveFeed([]);
    let statisticsCalls = 0;
    vi.spyOn(api, 'requests').mockResolvedValue(page([]));
    const { container } = renderPage({
      statistics: (call) => statisticsFixture({ total_requests: call === 1 ? 100 : 101 }),
    });

    // The initial load happens on mount; the summary renders once it lands.
    await waitFor(() => expect(statValue(container, 'Requests')).toBe('100'));
    statisticsCalls = 1;

    feed.emit({
      type: 'finished',
      live_id: 'live-stats-1',
      finished_at_ms: Date.now(),
      active_count: 0,
      request: {
        id: 'log-finished-stats',
        model: 'openai/gpt-4o',
        created_at: new Date().toISOString(),
        status: 200,
        duration_ms: 800,
        input_tokens: 100,
        output_tokens: 10,
      },
    });

    // The live-triggered refetch is debounced and silent: the number moves
    // without a progress bar ever appearing.
    await waitFor(() => expect(statValue(container, 'Requests')).toBe('101'), { timeout: 6000 });
    expect(container.querySelector('.range-progress')).toBeNull();
    expect(api.statistics).toHaveBeenCalledTimes(statisticsCalls + 1);
  });
});
