import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import App from '../../src/App.svelte';
import { setAdminAccessToken } from '../../src/lib/api';
import type { SessionRestoreOutcome } from '../../src/lib/auth-flow';

// The route bundles are code-split dynamic imports; these tests assert where the
// router lands, so the loaded chunk only has to stay pending.
const { restore, loadFeature } = vi.hoisted(() => ({
  restore: vi.fn(),
  loadFeature: vi.fn(),
}));

vi.mock('../../src/lib/auth-flow', () => ({
  restoreSessionOnce: restore,
  createSessionRetryCountdown: () => ({ start: () => {}, stop: () => {} }),
}));

vi.mock('../../src/lib/feature-registry', () => ({
  featureComponentLoader: (page: string) => () => loadFeature(page),
}));

const AUTHENTICATED: SessionRestoreOutcome = {
  kind: 'ready',
  authenticated: true,
  mustChangePassword: false,
};
const ANONYMOUS: SessionRestoreOutcome = { kind: 'ready', authenticated: false, mustChangePassword: false };
const MUST_CHANGE_PASSWORD: SessionRestoreOutcome = {
  kind: 'ready',
  authenticated: true,
  mustChangePassword: true,
};

/** Mirrors the real restore: a successful refresh puts an access token in memory. */
function session(outcome: SessionRestoreOutcome): void {
  restore.mockImplementation(async () => {
    if (outcome.authenticated) setAdminAccessToken('restored-token', 600);
    return outcome;
  });
}

/** A page whose chunk never resolves, so the dashboard shell stays on screen. */
function holdFeatureChunks(): void {
  loadFeature.mockImplementation(() => new Promise(() => {}));
}

function visit(path: string): void {
  window.history.pushState(null, '', path);
}

afterEach(() => {
  vi.restoreAllMocks();
  restore.mockReset();
  loadFeature.mockReset();
  setAdminAccessToken('');
  window.history.pushState(null, '', '/');
});

describe('an authenticated visit to the login route', () => {
  test('lands on the dashboard instead of the sign-in form', async () => {
    session(AUTHENTICATED);
    holdFeatureChunks();
    visit('/login');

    render(App, {});

    await waitFor(() => expect(window.location.pathname).toBe('/overview'));
    expect(await screen.findByText('Loading Overview…')).toBeTruthy();
    expect(screen.queryByRole('heading', { name: 'Sign in' })).toBeNull();
  });

  test('honours the return path the redirect carried', async () => {
    session(AUTHENTICATED);
    holdFeatureChunks();
    visit('/login?next=%2Fsettings');

    render(App, {});

    await waitFor(() => expect(window.location.pathname).toBe('/settings'));
    expect(await screen.findByText('Loading Settings…')).toBeTruthy();
  });

  test('leaves an untrusted return path out of the destination', async () => {
    session(AUTHENTICATED);
    holdFeatureChunks();
    visit('/login?next=https%3A%2F%2Fevil.example%2Fsteal');

    render(App, {});

    await waitFor(() => expect(window.location.pathname).toBe('/overview'));
  });

  test('leaves a session that must change its password on the login route', async () => {
    session(MUST_CHANGE_PASSWORD);
    visit('/login');

    render(App, {});

    expect(await screen.findByRole('heading', { name: 'Set new password' })).toBeTruthy();
    expect(window.location.pathname).toBe('/login');
  });

  test('keeps the sign-in form for a visitor without a session', async () => {
    session(ANONYMOUS);
    visit('/login');

    render(App, {});

    expect(await screen.findByRole('heading', { name: 'Sign in' })).toBeTruthy();
    expect(window.location.pathname).toBe('/login');
  });
});

describe('the documentation sign-in link', () => {
  test('returns an authenticated reader to the dashboard', async () => {
    session(AUTHENTICATED);
    holdFeatureChunks();
    visit('/docs');

    render(App, {});

    fireEvent.click(await screen.findByRole('link', { name: /Admin sign in/ }));

    await waitFor(() => expect(window.location.pathname).toBe('/overview'));
    expect(await screen.findByText('Loading Overview…')).toBeTruthy();
  });

  test('still reaches the sign-in form for a visitor without a session', async () => {
    session(ANONYMOUS);
    visit('/docs');

    render(App, {});

    fireEvent.click(await screen.findByRole('link', { name: /Admin sign in/ }));

    expect(await screen.findByRole('heading', { name: 'Sign in' })).toBeTruthy();
    expect(window.location.pathname).toBe('/login');
  });
});

describe('the documentation navbar logo', () => {
  test('returns an authenticated reader to the dashboard', async () => {
    session(AUTHENTICATED);
    holdFeatureChunks();
    visit('/docs/quickstart');

    render(App, {});

    fireEvent.click(await screen.findByRole('link', { name: 'ExoRoute overview' }));

    await waitFor(() => expect(window.location.pathname).toBe('/overview'));
    expect(await screen.findByText('Loading Overview…')).toBeTruthy();
  });

  test('reaches the sign-in form for a visitor without a session', async () => {
    session(ANONYMOUS);
    visit('/docs/quickstart');

    render(App, {});

    fireEvent.click(await screen.findByRole('link', { name: 'ExoRoute overview' }));

    expect(await screen.findByRole('heading', { name: 'Sign in' })).toBeTruthy();
    expect(window.location.pathname).toBe('/login');
  });

  // The logo no longer opens the documentation home, so the sidebar has to keep
  // that page reachable.
  test('leaves the documentation home reachable from the sidebar', async () => {
    session(ANONYMOUS);
    visit('/docs/quickstart');

    render(App, {});

    fireEvent.click(await screen.findByRole('link', { name: /^Overview / }));

    await waitFor(() => expect(window.location.pathname).toBe('/docs'));
  });
});
