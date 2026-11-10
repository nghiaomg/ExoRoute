import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import DashboardRequestsPage from '../../src/features/requests/DashboardRequestsPage.svelte';
import RequestTableRow from '../../src/features/requests/RequestTableRow.svelte';
import { api } from '../../src/lib/api';
import type { RequestLiveRow, RequestLog, RequestLogPage } from '../../src/lib/types';
import type { Translate } from '../../src/lib/format';
import { statisticsFixture } from './support/statistics';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const HIDDEN_COLUMNS_KEY = 'exoroute:requests:hidden_columns';

function page(requests: RequestLog[] = []): RequestLogPage {
  return { requests, next_cursor: null, page_size: 50 };
}

const baseRow: RequestLog = {
  id: 'log-1',
  model: 'openai/gpt-5',
  route_alias: 'combo/served-model',
  provider_id: 'p-1',
  api_key_name: 'Main key',
  // The page filters rows outside its default 24-hour window, so the fixture
  // must carry a fresh timestamp rather than a pinned date.
  created_at: new Date(Date.now() - 60_000).toISOString(),
  status: 200,
  duration_ms: 120,
  input_tokens: 100,
  output_tokens: 20,
};

function row(overrides: Partial<RequestLog>): RequestLog {
  return { ...baseRow, ...overrides };
}

function provider(id: string, name: string) {
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

function mockApi(requests: RequestLog[]): void {
  vi.spyOn(api, 'requests').mockResolvedValue(page(requests));
  vi.spyOn(api, 'providers').mockResolvedValue([provider('p-1', 'OpenAI')]);
  vi.spyOn(api, 'statistics').mockResolvedValue(statisticsFixture());
  vi.spyOn(api, 'streamRequestLive').mockImplementation(async () => {});
}

function renderPage(requests: RequestLog[] = []): ReturnType<typeof render> {
  mockApi(requests);
  return render(DashboardRequestsPage, {
    props: { tr, locale: 'en', onConnectionChange: () => {} },
  });
}

function headers(): Array<string | undefined> {
  return Array.from(document.querySelectorAll('.req-table thead th')).map((th) => th.textContent?.trim());
}

async function openHideMenu(): Promise<void> {
  const trigger = document.querySelector('.req-vis-trigger') as HTMLElement;
  await fireEvent.click(trigger);
  await waitFor(() => expect(document.querySelector('.req-vis-dropdown')).not.toBeNull());
}

async function toggleCheckbox(name: string): Promise<void> {
  const checkbox = screen.getByRole('checkbox', { name }) as HTMLInputElement;
  await fireEvent.click(checkbox);
}

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('requests proxy chip', () => {
  test('renders proxy_name, falls back to proxy_id, and stays absent without proxy fields', async () => {
    const { container } = renderPage([
      row({ id: 'log-named', proxy_id: 'proxy-1', proxy_name: 'EU egress' }),
      row({ id: 'log-id-only', model: 'openai/gpt-4o', proxy_id: 'proxy-2', proxy_name: null }),
      row({ id: 'log-direct', model: 'anthropic/claude' }),
    ]);

    const namedRow = (await screen.findByText('openai/gpt-5')).closest('tr');
    expect(namedRow?.querySelector('.req-proxy-chip')?.textContent?.trim()).toBe('EU egress');
    expect(namedRow?.querySelector('.req-proxy-chip')?.getAttribute('title')).toBe('EU egress');

    const idOnlyRow = screen.getByText('openai/gpt-4o').closest('tr');
    expect(idOnlyRow?.querySelector('.req-proxy-chip')?.textContent?.trim()).toBe('proxy-2');

    const directRow = screen.getByText('anthropic/claude').closest('tr');
    expect(directRow?.querySelector('.req-proxy-chip')).toBeNull();
    expect(container.querySelector('.req-proxy-chip')).not.toBeNull();
  });

  test('live SSE rows render the proxy chip too', async () => {
    const liveRow: RequestLiveRow = {
      live: true,
      live_id: 'live-proxy',
      started_at_ms: Date.now(),
      model: 'live/proxy-model',
      created_at: new Date().toISOString(),
      proxy_id: 'proxy-9',
      proxy_name: 'Pool A',
    };
    vi.spyOn(api, 'requests').mockResolvedValue(page([]));
    vi.spyOn(api, 'providers').mockResolvedValue([]);
    vi.spyOn(api, 'statistics').mockResolvedValue(statisticsFixture());
    vi.spyOn(api, 'streamRequestLive').mockImplementation((onEvent) => {
      // Real SSE events land after the subscribing effect has settled; emitting
      // synchronously here would write state from inside the $effect and trip
      // Svelte's update-depth guard.
      queueMicrotask(() => {
        onEvent({ type: 'snapshot', requests: [liveRow], truncated: false, limit: 50, active_count: 1 });
      });
      return Promise.resolve();
    });
    render(DashboardRequestsPage, {
      props: { tr, locale: 'en', onConnectionChange: () => {} },
    });

    const liveTr = (await screen.findByText('live/proxy-model')).closest('tr');
    expect(liveTr?.querySelector('.req-proxy-chip')?.textContent?.trim()).toBe('Pool A');
  });
});

describe('unrouted request row proxy chip', () => {
  function renderRow(request: RequestLog): ReturnType<typeof render> {
    return render(RequestTableRow, {
      props: { tr, locale: 'en', request, liveClockMs: Date.now() },
    });
  }

  test('the alias cell shows the proxy chip next to the combo pill, with id fallback and absence', () => {
    const named = renderRow(row({ id: 'row-named', proxy_id: 'proxy-3', proxy_name: 'EU pool' }));
    const namedCell = named.container.querySelector('td.req-alias');
    expect(namedCell?.querySelector('.req-alias-pill')).not.toBeNull();
    expect(namedCell?.querySelector('.req-proxy-chip')?.textContent?.trim()).toBe('EU pool');
    named.unmount();

    const idOnly = renderRow(row({ id: 'row-id-only', route_alias: '', proxy_id: 'proxy-4', proxy_name: null }));
    const idOnlyCell = idOnly.container.querySelector('td.req-alias');
    expect(idOnlyCell?.querySelector('.req-proxy-chip')?.textContent?.trim()).toBe('proxy-4');
    idOnly.unmount();

    const direct = renderRow(row({ id: 'row-direct', route_alias: '' }));
    const directCell = direct.container.querySelector('td.req-alias');
    expect(directCell?.querySelector('.req-proxy-chip')).toBeNull();
    expect(directCell?.querySelector('.muted-cell')?.textContent).toBe('—');
    direct.unmount();
  });
});

describe('requests hide-info menu', () => {
  test('toggling Provider and Key hides and restores their columns, updating the badge', async () => {
    renderPage([row({})]);
    await screen.findByText('openai/gpt-5');

    expect(headers()).toContain('Provider');
    expect(headers()).toContain('Key');

    await openHideMenu();
    await toggleCheckbox('Provider');
    expect(headers()).not.toContain('Provider');
    // The Key column is untouched by the Provider toggle.
    expect(screen.queryByText('Main key')).not.toBeNull();

    await toggleCheckbox('Key');
    expect(headers()).not.toContain('Key');
    expect(screen.queryByText('Main key')).toBeNull();

    expect(document.querySelector('.req-vis-badge')?.textContent).toBe('2');
    // The menu stays open so several columns can be toggled in one pass.
    expect(document.querySelector('.req-vis-dropdown')).not.toBeNull();

    await toggleCheckbox('Provider');
    await toggleCheckbox('Key');
    expect(headers()).toContain('Provider');
    expect(headers()).toContain('Key');
    expect(screen.getByText('Main key')).not.toBeNull();
    expect(document.querySelector('.req-vis-badge')).toBeNull();
  });

  test('the Combo toggle hides only the combo chip while its cell and columns remain', async () => {
    renderPage([row({})]);
    await screen.findByText('openai/gpt-5');

    expect(screen.getByText('openai/gpt-5').closest('tr')?.querySelector('.req-combo-chip')?.textContent).toBe('combo/served-model');
    expect(document.querySelectorAll('.req-table thead th').length).toBe(8);

    await openHideMenu();
    await toggleCheckbox('Combo');

    const updatedRow = screen.getByText('openai/gpt-5').closest('tr');
    expect(updatedRow?.querySelector('.req-combo-chip')).toBeNull();
    // Combo is a chip inside the model cell, not a column: nothing disappears.
    expect(document.querySelectorAll('.req-table thead th').length).toBe(8);
    expect(updatedRow?.textContent).toContain('openai/gpt-5');
    expect(headers()).toContain('Provider');
    expect(headers()).toContain('Key');

    await toggleCheckbox('Combo');
    expect(screen.getByText('openai/gpt-5').closest('tr')?.querySelector('.req-combo-chip')?.textContent).toBe('combo/served-model');
  });

  test('the proxy chip is never hidden by the visibility toggles', async () => {
    renderPage([row({ proxy_id: 'proxy-1', proxy_name: 'EU egress' })]);
    await screen.findByText('openai/gpt-5');

    await openHideMenu();
    await toggleCheckbox('Provider');
    await toggleCheckbox('Key');
    await toggleCheckbox('Combo');

    const rowEl = screen.getByText('openai/gpt-5').closest('tr');
    expect(rowEl?.querySelector('.req-proxy-chip')?.textContent?.trim()).toBe('EU egress');
    // Only the always-present columns remain: Time, Status, Model, Latency, Tokens, Cost.
    expect(document.querySelectorAll('.req-table thead th').length).toBe(6);
  });

  test('the hidden-columns choice persists across remount and garbage falls back to defaults', async () => {
    const first = renderPage([row({})]);
    await screen.findByText('openai/gpt-5');
    await openHideMenu();
    await toggleCheckbox('Provider');
    expect(JSON.parse(localStorage.getItem(HIDDEN_COLUMNS_KEY) ?? '{}')).toEqual({ provider: true, key: false, combo: false });
    first.unmount();

    mockApi([row({})]);
    const second = render(DashboardRequestsPage, {
      props: { tr, locale: 'en', onConnectionChange: () => {} },
    });
    await waitFor(() => expect(screen.queryByText('openai/gpt-5')).not.toBeNull());
    expect(headers()).not.toContain('Provider');
    expect(headers()).toContain('Key');
    expect(document.querySelector('.req-vis-badge')?.textContent).toBe('1');
    second.unmount();

    // Stored garbage must not hide anything the user never asked to hide.
    localStorage.setItem(HIDDEN_COLUMNS_KEY, 'not-json');
    mockApi([row({})]);
    render(DashboardRequestsPage, {
      props: { tr, locale: 'en', onConnectionChange: () => {} },
    });
    await waitFor(() => expect(screen.queryByText('openai/gpt-5')).not.toBeNull());
    expect(headers()).toContain('Provider');
    expect(headers()).toContain('Key');
    expect(document.querySelectorAll('.req-table thead th').length).toBe(8);
  });

  test('Escape and outside click close the menu', async () => {
    renderPage([row({})]);
    await screen.findByText('openai/gpt-5');

    await openHideMenu();
    await fireEvent.keyDown(document, { key: 'Escape' });
    await waitFor(() => expect(document.querySelector('.req-vis-dropdown')).toBeNull());

    await openHideMenu();
    await fireEvent.click(document.body);
    await waitFor(() => expect(document.querySelector('.req-vis-dropdown')).toBeNull());
    expect(document.querySelector('.req-vis-trigger')?.getAttribute('aria-expanded')).toBe('false');
  });

  test('seeding storage with both columns hidden collapses the empty-state colspan', async () => {
    localStorage.setItem(HIDDEN_COLUMNS_KEY, JSON.stringify({ provider: true, key: true, combo: false }));
    renderPage([]);
    await waitFor(() => expect(document.querySelector('.empty-state')).not.toBeNull());
    expect(headers()).not.toContain('Provider');
    expect(headers()).not.toContain('Key');
    expect(document.querySelector('.req-table td[colspan]')?.getAttribute('colspan')).toBe('6');
  });
});
