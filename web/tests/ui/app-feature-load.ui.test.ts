import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import App from '../../src/App.svelte';
import ApiKeysPage from '../../src/features/api-keys/DashboardApiKeysPage.svelte';
import ChatPage from '../../src/features/chat/ChatPage.svelte';
import HelpPage from '../../src/features/help/DashboardHelpPage.svelte';
import SettingsPage from '../../src/features/settings/DashboardSettingsPage.svelte';
import { api, setAdminAccessToken } from '../../src/lib/api';
import type { WorkspaceChatModelsResult } from '../../src/lib/types';

// The route bundles are code-split dynamic imports, so a missing or unrunnable
// chunk is a normal production failure. Mock the registry to drive it.
const { loadFeature } = vi.hoisted(() => ({ loadFeature: vi.fn() }));

vi.mock('../../src/lib/feature-registry', () => ({
  featureComponentLoader: (page: string) => () => loadFeature(page),
}));

vi.mock('../../src/lib/auth-flow', () => ({
  restoreSessionOnce: async () => ({ kind: 'ready', authenticated: true, mustChangePassword: false }),
  createSessionRetryCountdown: () => ({ start: () => {}, stop: () => {} }),
}));

const modelsResult: WorkspaceChatModelsResult = {
  models: [{ provider_id: 'opencode-go', model: 'minimax-m3', id: 'ocg/minimax-m3' }],
  truncated: false,
};

let chunkAvailable = true;

afterEach(() => {
  vi.restoreAllMocks();
  setAdminAccessToken('');
  window.history.pushState(null, '', '/');
  chunkAvailable = true;
});

describe('dashboard feature loading', () => {
  test.each([
    ['api-keys', '/api-keys', ApiKeysPage, 'API keys'],
    ['settings', '/settings', SettingsPage, 'Settings'],
    ['help', '/help', HelpPage, 'Help Center'],
  ] as const)('%s renders its page content inside the dashboard shell', async (page, path, component, heading) => {
    loadFeature.mockResolvedValue({ default: component });

    if (page === 'api-keys') {
      vi.spyOn(api, 'apiKeys').mockRejectedValue(new Error('offline'));
      vi.spyOn(api, 'statistics').mockRejectedValue(new Error('offline'));
      vi.spyOn(api, 'providers').mockRejectedValue(new Error('offline'));
    } else if (page === 'settings') {
      vi.spyOn(api, 'settings').mockRejectedValue(new Error('offline'));
      vi.spyOn(api, 'operationalSettings').mockRejectedValue(new Error('offline'));
      vi.spyOn(api, 'gatewayResourceLimits').mockRejectedValue(new Error('offline'));
    } else {
      vi.spyOn(api, 'overview').mockRejectedValue(new Error('offline'));
      vi.spyOn(api, 'statistics').mockRejectedValue(new Error('offline'));
    }

    setAdminAccessToken('test-token');
    window.history.pushState(null, '', path);
    render(App, {});

    expect(await screen.findByRole('heading', { name: heading })).toBeTruthy();
  });

  test('a failed feature import offers Retry instead of a permanent loading screen', async () => {
    chunkAvailable = false;
    loadFeature.mockImplementation(() =>
      chunkAvailable
        ? Promise.resolve({ default: ChatPage })
        : Promise.reject(new Error('Failed to fetch dynamically imported module')),
    );
    vi.spyOn(api, 'chatModels').mockResolvedValue(modelsResult);
    setAdminAccessToken('test-token');
    window.history.pushState(null, '', '/chat');

    render(App, {});

    expect(await screen.findByRole('button', { name: 'Retry' })).toBeTruthy();
    // A page the browser memoized as a failed chunk needs a document reload.
    expect(screen.getByRole('button', { name: 'Reload page' })).toBeTruthy();
    expect(screen.getByText('Could not load this page.')).toBeTruthy();
    // The unrecoverable state this guards against was an unexplained spinner.
    expect(screen.queryByText(/Loading/i)).toBeNull();

    chunkAvailable = true;
    await fireEvent.click(screen.getByRole('button', { name: 'Retry' }));

    await waitFor(() => expect(screen.getByText('Workspace chat')).toBeTruthy());
  });
});
