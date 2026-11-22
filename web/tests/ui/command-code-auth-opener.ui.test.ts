import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import CommandCodeAuthAssistPanel from '../../src/features/providers/CommandCodeAuthAssistPanel.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const PROVIDER_ID = 'command-code-provider';
const AUTH_URL =
  'https://commandcode.ai/studio/auth/cli?callback=http%3A%2F%2Flocalhost%3A5959%2Fcallback&state=abc123';

interface NavigationSnapshot {
  url: string;
  openerAtNavigation: unknown;
}

/** A popup double whose opener starts non-null, mirroring a real window.open result. */
function stubPopup() {
  const openerSentinel = { label: 'dashboard-opener' } as unknown as Window;
  const navigations: NavigationSnapshot[] = [];
  const popup = {
    closed: false,
    opener: openerSentinel as unknown,
    close: vi.fn(() => {
      popup.closed = true;
    }),
    location: {
      assign: vi.fn((url: string) => {
        // Capture the opener at the exact moment navigation is requested.
        navigations.push({ url, openerAtNavigation: popup.opener });
      }),
    },
  };
  vi.spyOn(window, 'open').mockImplementation(() => popup as unknown as Window);
  return { popup, navigations, openerSentinel };
}

function renderPanel(onConnected = vi.fn()) {
  render(CommandCodeAuthAssistPanel, {
    props: { providerId: PROVIDER_ID, tr, onConnected },
  });
  return onConnected;
}

function startAuth(authUrl: string) {
  return vi.spyOn(api, 'startProviderApiKeyAuth').mockResolvedValue({
    flow_id: 'flow-1',
    auth_url: authUrl,
    callback_url: 'http://localhost:5959/callback',
    expires_at_ms: Date.now() + 300_000,
  });
}

function keepPollingPending() {
  return vi.spyOn(api, 'providerApiKeyAuthStatus').mockResolvedValue({ status: 'pending' });
}

function clickConnect() {
  return fireEvent.click(screen.getByRole('button', { name: 'Connect with Command Code Studio' }));
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('CommandCodeAuthAssistPanel popup opener hardening', () => {
  test('severs popup.opener before navigating to the third-party auth_url', async () => {
    const { popup, navigations, openerSentinel } = stubPopup();
    startAuth(AUTH_URL);
    keepPollingPending();
    renderPanel();

    expect(popup.opener).toBe(openerSentinel);

    await clickConnect();

    await waitFor(() => {
      expect(navigations).toHaveLength(1);
    });
    expect(navigations[0].url).toBe(AUTH_URL);
    // The security invariant: at navigation time the third-party page has no opener.
    expect(navigations[0].openerAtNavigation).toBeNull();
    expect(popup.opener).toBeNull();
  });

  test('a blocked popup fails gracefully without starting the auth flow', async () => {
    vi.spyOn(window, 'open').mockReturnValue(null);
    const start = startAuth(AUTH_URL);
    renderPanel();

    await clickConnect();

    expect(
      await screen.findByText('Allow pop-ups to connect with Command Code Studio.'),
    ).toBeTruthy();
    expect(start).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Connect with Command Code Studio' })).toBeTruthy();
  });

  test('a missing auth_url closes the popup without navigating', async () => {
    const { popup, navigations } = stubPopup();
    startAuth('');
    const status = keepPollingPending();
    renderPanel();

    await clickConnect();

    expect(
      await screen.findByText('Could not start Command Code Studio sign-in.'),
    ).toBeTruthy();
    expect(navigations).toHaveLength(0);
    expect(popup.close).toHaveBeenCalled();
    expect(status).not.toHaveBeenCalled();
  });
});
