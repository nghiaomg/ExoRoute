/**
 * UI tests for the release update notice in the dashboard shell.
 *
 * The shell fetches the latest GitHub release on mount and shows a dismissible
 * banner when a newer version exists. jsdom has neither fetch/Response nor a
 * persistent-storage contract here, so the tests stub `fetch` with plain
 * response objects and clear localStorage between tests.
 */
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, test, vi } from 'vitest';
import DashboardShell from '../../src/components/DashboardShell.svelte';
import { t } from '../../src/lib/i18n';
import { DASHBOARD_VERSION } from '../../src/lib/version';

const tr = (key: string, vars?: Record<string, string | number>) => t('en', key, vars);

/** The patch bump after the running version, used as the "newer release". */
function nextPatch(version: string): string {
  const parts = version.split('.').map(Number);
  parts[parts.length - 1] += 1;
  return parts.join('.');
}
const NEWER_RELEASE = nextPatch(DASHBOARD_VERSION);
const NEWEST_RELEASE = nextPatch(NEWER_RELEASE);

function renderShell(): void {
  render(DashboardShell, {
    currentPage: 'overview',
    title: 'Overview',
    tr,
    preferences: {
      locale: 'en',
      theme: 'light',
      scheme: 'light',
      setLocale: () => {},
      toggleTheme: () => {},
    },
    gateway: { state: 'loaded', address: 'http://127.0.0.1:8686' },
    providerCount: 0,
    collapsed: false,
    onToggleCollapse: () => {},
    onNavigate: () => {},
    onRefresh: () => {},
    onSignOut: async () => {},
  });
}

type StubResponse = { ok: boolean; status: number; json: () => Promise<unknown> };

/** A minimal stand-in for the GitHub response shape release-check consumes. */
function releaseFetch(tag: string | null) {
  return vi.fn(async (): Promise<StubResponse> => {
    if (tag === null) return { ok: false, status: 404, json: async () => ({ message: 'Not Found' }) };
    return {
      ok: true,
      status: 200,
      json: async () => ({
        tag_name: `v${tag}`,
        html_url: `https://github.com/nghiaomg/ExoRoute/releases/tag/v${tag}`,
      }),
    };
  });
}

/** Waits until the stubbed fetch ran and the mount's async work settled. */
async function settle(fetchMock: { mock: { calls: unknown[] } }): Promise<void> {
  await waitFor(() => expect(fetchMock.mock.calls.length).toBeGreaterThan(0));
  await new Promise((resolve) => setTimeout(resolve, 0));
}

beforeEach(() => {
  localStorage.clear();
  vi.unstubAllGlobals();
});

describe('release update notice', () => {
  test('a newer published release shows the dismissible banner with both versions', async () => {
    const fetchMock = releaseFetch(NEWER_RELEASE);
    vi.stubGlobal('fetch', fetchMock);
    renderShell();

    expect(await screen.findByText('Update available')).toBeTruthy();
    expect(screen.getByText(`ExoRoute ${NEWER_RELEASE} is available. You are running ${DASHBOARD_VERSION}.`)).toBeTruthy();
    const link = screen.getByRole('link', { name: 'View release' });
    expect(link.getAttribute('href')).toBe(`https://github.com/nghiaomg/ExoRoute/releases/tag/v${NEWER_RELEASE}`);
    expect(link.getAttribute('rel')).toContain('noopener');

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss notification' }));
    await waitFor(() => expect(screen.queryByText('Update available')).toBeNull());
    expect(localStorage.getItem('exoroute.dismissed-release')).toBe(JSON.stringify(NEWER_RELEASE));
  });

  test('a dismissed version stays dismissed on the next mount until a newer release appears', async () => {
    localStorage.setItem('exoroute.dismissed-release', JSON.stringify(NEWER_RELEASE));
    const first = releaseFetch(NEWER_RELEASE);
    vi.stubGlobal('fetch', first);
    renderShell();
    await settle(first);
    expect(screen.queryByText('Update available')).toBeNull();

    // Drop the one-hour check cache so the second mount refetches, then let
    // the newer release re-notify past the dismissal.
    vi.unstubAllGlobals();
    localStorage.removeItem('exoroute.release-check');
    const second = releaseFetch(NEWEST_RELEASE);
    vi.stubGlobal('fetch', second);
    renderShell();
    expect(await screen.findByText(`ExoRoute ${NEWEST_RELEASE} is available. You are running ${DASHBOARD_VERSION}.`)).toBeTruthy();
  });

  test('an up-to-date dashboard shows no banner', async () => {
    const fetchMock = releaseFetch(DASHBOARD_VERSION);
    vi.stubGlobal('fetch', fetchMock);
    renderShell();

    await settle(fetchMock);
    expect(screen.queryByText('Update available')).toBeNull();
  });

  test('a failed release check shows no banner and no error', async () => {
    const fetchMock = releaseFetch(null);
    vi.stubGlobal('fetch', fetchMock);
    renderShell();

    await settle(fetchMock);
    expect(screen.queryByText('Update available')).toBeNull();
  });

  test('the shell reports the build-injected dashboard version', () => {
    vi.stubGlobal('fetch', releaseFetch(null));
    renderShell();
    expect(screen.getByText(DASHBOARD_VERSION)).toBeTruthy();
  });
});
