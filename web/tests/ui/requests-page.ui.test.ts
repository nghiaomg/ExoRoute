import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import RequestsPage from '../../src/features/requests/RequestsPage.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type {
  GatewayApiKey,
  Provider,
  RequestLiveEvent,
  RequestLiveRow,
  RequestLog,
  RequestLogPage,
} from '../../src/lib/types';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

function page(requests: RequestLog[]): RequestLogPage {
  return { requests, next_cursor: null, page_size: 50 };
}

const finishedRows: RequestLog[] = [
  {
    id: 'log-success',
    model: 'ocg/minimax-m3',
    created_at: '2026-10-05T10:00:00.000Z',
    status: 200,
    duration_ms: 100,
    input_tokens: 1000,
    output_tokens: 500,
    cached_tokens: 50,
  },
  {
    id: 'log-warning',
    model: 'openai/gpt-5',
    created_at: '2026-10-05T10:01:00.000Z',
    status: 404,
    duration_ms: 9000,
    error: 'upstream 404: not found',
  },
  {
    id: 'log-critical',
    model: 'anthropic/claude',
    created_at: '2026-10-05T10:02:00.000Z',
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
  created_at: '2026-10-05T10:03:00.000Z',
};

// The page always subscribes to the live feed; the snapshot gives the tests a
// deterministic active count instead of a real SSE stream. The returned
// `emit` pushes later events through the same subscription.
function mockLiveFeed(rows: RequestLiveRow[] = [live]): (event: RequestLiveEvent) => void {
  let emit: ((event: RequestLiveEvent) => void) | null = null;
  vi.spyOn(api, 'streamRequestLive').mockImplementation((onEvent) => {
    emit = onEvent;
    onEvent({ type: 'snapshot', requests: rows, truncated: false, limit: 50, active_count: rows.length });
    return Promise.resolve();
  });
  return (event) => emit?.(event);
}

type RenderOptions = { providers?: Provider[]; apiKeys?: GatewayApiKey[] };

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

function renderPage(options: RenderOptions = {}): ReturnType<typeof render> {
  // The filter panel loads both id option lists on mount; the tests that never
  // open those fields still need the calls answered.
  vi.spyOn(api, 'providers').mockResolvedValue(options.providers ?? []);
  vi.spyOn(api, 'apiKeys').mockResolvedValue({ api_keys: options.apiKeys ?? [], next_cursor: null });
  return render(RequestsPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('requests page', () => {
  test('shows a skeleton on first load, then the page-scoped KPIs and tiered rows', async () => {
    mockLiveFeed([]);
    let resolveRequests: (value: RequestLogPage) => void = () => {};
    const pending = new Promise<RequestLogPage>((resolve) => { resolveRequests = resolve; });
    vi.spyOn(api, 'requests').mockReturnValue(pending);

    const { container } = renderPage();

    await waitFor(() => expect(container.querySelector('.request-skeleton')).not.toBeNull());
    resolveRequests(page(finishedRows));

    await screen.findByText('anthropic/claude');
    expect(container.querySelector('.request-skeleton')).toBeNull();

    // The strip stays page-scoped: the three finished rows on this page.
    expect(container.querySelector('.request-metric.active .request-metric-value')?.textContent?.trim()).toBe('0');
    expect(container.querySelector('.request-metric.total .request-metric-value')?.textContent?.trim()).toBe('3');
    expect(container.querySelector('.request-metric.success .request-metric-value')?.textContent?.trim()).toBe('33%');
    expect(container.querySelector('.request-metric.duration .request-metric-value')?.textContent?.trim()).toBe('9000 ms');

    const legend = container.querySelectorAll('.request-outcome-entry');
    expect(legend.length).toBe(2);
    expect(container.querySelector('.request-outcome-entry.success .request-outcome-count')?.textContent?.trim()).toBe('1');
    expect(container.querySelector('.request-outcome-entry.failure .request-outcome-count')?.textContent?.trim()).toBe('2');

    expect(container.querySelectorAll('tbody tr').length).toBe(3);

    // HTTP tiers: 2xx success, 4xx warning, 5xx critical.
    const successRow = screen.getByText('ocg/minimax-m3').closest('tr');
    const warningRow = screen.getByText('openai/gpt-5').closest('tr');
    const criticalRow = screen.getByText('anthropic/claude').closest('tr');
    expect(successRow?.querySelector('.status-badge')?.className).toContain('success');
    expect(warningRow?.querySelector('.status-badge')?.className).toContain('warning');
    expect(criticalRow?.querySelector('.status-badge')?.className).toContain('critical');

    // Duration tiers and the cached-token badge come from the row data.
    expect(criticalRow?.querySelector('.req-duration-value')?.className).toContain('slow');
    expect(warningRow?.querySelector('.req-duration-value')?.className).toContain('verySlow');
    expect(successRow?.querySelector('.req-token-cached')?.textContent).toContain('Cached 50');
  });

  test('clicking a row opens the detail drawer and the error button does not', async () => {
    mockLiveFeed();
    vi.spyOn(api, 'requests').mockResolvedValue(page(finishedRows));
    const { container } = renderPage();

    const criticalRow = (await screen.findByText('anthropic/claude')).closest('tr') as HTMLElement;

    await fireEvent.click(criticalRow.querySelector('.req-error-trigger-btn') as HTMLElement);
    expect(await screen.findByText('Request error details')).toBeTruthy();
    expect(screen.queryByText('Request details')).toBeNull();
    await fireEvent.click(screen.getByText('Close'));
    await waitFor(() => expect(screen.queryByText('Request error details')).toBeNull());

    await fireEvent.click(criticalRow);
    expect(await screen.findByText('Request details')).toBeTruthy();
    const drawer = container.ownerDocument.querySelector('.request-drawer-card') as HTMLElement;
    expect(drawer.textContent).toContain('Provider credential');
    expect(drawer.textContent).toContain('Client protocol');
    expect(drawer.textContent).toContain('provider exploded');
    expect(criticalRow.classList.contains('selected')).toBe(true);

    await fireEvent.click(screen.getByText('Close'));
    await waitFor(() => expect(screen.queryByText('Request details')).toBeNull());
  });

  test('the live feed marks in-flight rows and the toggle freezes the table', async () => {
    const emit = mockLiveFeed();
    vi.spyOn(api, 'requests').mockResolvedValue(page(finishedRows));
    const { container } = renderPage();

    await screen.findByText('live/preview-model');
    // The live row joins the table and the server's active count, and the
    // outcome mix gains the in-progress segment.
    expect(container.querySelector('.request-metric.active .request-metric-value')?.textContent?.trim()).toBe('1');
    expect(container.querySelector('.request-metric.total .request-metric-value')?.textContent?.trim()).toBe('4');
    expect(container.querySelectorAll('tbody tr').length).toBe(4);
    // 'In progress' also labels the outcome legend, so scope this to the row.
    const liveRow = screen.getByText('live/preview-model').closest('tr');
    expect(liveRow?.querySelector('.status-badge')?.className).toContain('live');
    expect(liveRow?.textContent).toContain('In progress');
    expect(container.querySelector('.request-outcome-entry.active .request-outcome-count')?.textContent?.trim()).toBe('1');

    const toggle = screen.getByRole('button', { name: /Live/ });
    expect(toggle.getAttribute('aria-pressed')).toBe('true');

    await fireEvent.click(toggle);
    const paused = await screen.findByRole('button', { name: /Paused/ });
    expect(paused.getAttribute('aria-pressed')).toBe('false');

    // A row that starts while paused stays out of the frozen table (and out of
    // the frozen active count) until live resumes.
    emit({ type: 'started', request: { ...live, live_id: 'live-2', model: 'live/second-model' }, active_count: 2 });
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(container.querySelectorAll('tbody tr').length).toBe(4);
    expect(container.querySelector('.request-metric.active .request-metric-value')?.textContent?.trim()).toBe('1');

    await fireEvent.click(paused);
    await waitFor(() => expect(container.querySelectorAll('tbody tr').length).toBe(5));
    expect(container.querySelector('.request-metric.active .request-metric-value')?.textContent?.trim()).toBe('2');
  });

  test('the filter panel searches the id fields, applies a chip, and removing the chip clears it', async () => {
    mockLiveFeed();
    const requests = vi.spyOn(api, 'requests').mockResolvedValue(page(finishedRows));
    const { container } = renderPage({
      apiKeys: [{ id: 'key-42', name: 'Production key', enabled: true, created_at: '2026-10-05T10:00:00.000Z' }],
      providers: [provider('p-1', 'OpenAI')],
    });

    await screen.findByText('anthropic/claude');
    expect(container.querySelector('.request-filter-chip')).toBeNull();

    // The id field is a select: typing searches the server-backed key list.
    const input = screen.getByPlaceholderText('Search API keys by name or ID');
    await fireEvent.click(input);
    await waitFor(() => expect(api.apiKeys).toHaveBeenLastCalledWith({ q: '' }, expect.anything()));
    await fireEvent.input(input, { target: { value: 'prod' } });
    await waitFor(() => expect(api.apiKeys).toHaveBeenLastCalledWith({ q: 'prod' }, expect.anything()));

    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    await fireEvent.keyDown(input, { key: 'Enter' });
    await fireEvent.click(screen.getByRole('button', { name: 'Apply filters' }));

    await waitFor(() => expect(requests).toHaveBeenCalledTimes(2));
    expect(requests).toHaveBeenLastCalledWith(
      { api_key_id: 'key-42', model: '', provider_id: '', status: '', cursor: null },
      expect.anything(),
    );
    // The chip reads by name and keeps the raw id as its tooltip.
    const chip = container.querySelector('.request-filter-chip-value');
    expect(chip?.textContent?.trim()).toBe('Production key');
    expect(chip?.getAttribute('title')).toBe('key-42');

    await fireEvent.click(screen.getByRole('button', { name: 'Remove API key ID' }));

    await waitFor(() => expect(requests).toHaveBeenCalledTimes(3));
    expect(requests).toHaveBeenLastCalledWith(
      { api_key_id: '', model: '', provider_id: '', status: '', cursor: null },
      expect.anything(),
    );
    await waitFor(() => expect(container.querySelector('.request-filter-chip')).toBeNull());
  });

  test('the provider id field offers the loaded providers and filters them locally', async () => {
    mockLiveFeed();
    vi.spyOn(api, 'requests').mockResolvedValue(page(finishedRows));
    renderPage({ providers: [provider('p-1', 'OpenAI'), provider('p-2', 'Anthropic')] });

    await screen.findByText('anthropic/claude');
    // Providers come back whole, so the field never queries per keystroke.
    await waitFor(() => expect(api.providers).toHaveBeenCalledTimes(1));

    const input = screen.getByPlaceholderText('Search providers by name or ID');
    await fireEvent.click(input);
    expect(await screen.findByText('OpenAI')).toBeTruthy();

    await fireEvent.input(input, { target: { value: 'anth' } });
    await waitFor(() => expect(screen.queryByText('OpenAI')).toBeNull());
    expect(screen.getByText('Anthropic')).toBeTruthy();
    expect(api.providers).toHaveBeenCalledTimes(1);
  });
});
