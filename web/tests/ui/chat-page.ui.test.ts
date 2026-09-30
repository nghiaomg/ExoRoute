import { render, screen, waitFor, fireEvent } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import ChatPage from '../../src/features/chat/ChatPage.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type { WorkspaceChatModelsResult } from '../../src/lib/types';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const modelsResult: WorkspaceChatModelsResult = {
  models: [
    { provider_id: 'opencode-go', model: 'minimax-m3', id: 'ocg/minimax-m3' },
    { provider_id: 'openrouter', model: 'mimo-v2.6-flash', id: 'openrouter/mimo-v2.6-flash' },
  ],
  truncated: false,
};

afterEach(() => {
  vi.restoreAllMocks();
});

describe('ChatPage', () => {
  test('renders the searchable picker pill, composer, and the hero suggestions', async () => {
    const load = vi.spyOn(api, 'chatModels').mockResolvedValue(modelsResult);

    render(ChatPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });

    // The picker is a searchable pill input, not a hidden native select.
    expect(await screen.findByPlaceholderText('Select a model')).toBeTruthy();
    await waitFor(() => {
      expect(load).toHaveBeenCalled();
    });
    expect(screen.getByRole('button', { name: 'Send' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Attach' })).toBeTruthy();
    // The privacy contract stays visible on the empty-state hero.
    expect(
      screen.getByText(
        'Workspace chat runs through the gateway relay without request logging, telemetry, or usage metering. Closing this page discards the conversation.',
      ),
    ).toBeTruthy();
    expect(screen.getByText('Workspace chat')).toBeTruthy();
    expect(screen.getByText('Draft a status update')).toBeTruthy();
    // Settings (system prompt, thinking) start collapsed behind the toggle.
    expect(screen.queryByPlaceholderText('Instructions the model follows for every message')).toBeNull();
    await fireEvent.click(screen.getByTitle('Chat settings'));
    expect(
      screen.getByPlaceholderText('Instructions the model follows for every message'),
    ).toBeTruthy();
  });

  test('shows an error state when the model list cannot load', async () => {
    vi.spyOn(api, 'chatModels').mockRejectedValue(new Error('gateway down'));

    render(ChatPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Retry' })).toBeTruthy();
    });
    expect(screen.queryByPlaceholderText('Type a message…')).toBeNull();
  });
});
