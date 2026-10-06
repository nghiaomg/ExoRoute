import { render, screen, waitFor, fireEvent } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import ChatPage from '../../src/features/chat/ChatPage.svelte';
import { ApiError, api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type { WorkspaceChatModelsResult } from '../../src/lib/types';

vi.mock('../../src/features/chat/chat.state', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../src/features/chat/chat.state')>();
  // The stall test needs the page's own deadline to fire quickly: waiting out
  // the production 20s would force fake timers, which interfere with
  // testing-library's polling and with vitest's own timeout accounting.
  return { ...actual, MODEL_LIST_TIMEOUT_MS: 50 };
});

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

  test('starts on a model, shown as the readable name instead of the raw id', async () => {
    vi.spyOn(api, 'chatModels').mockResolvedValue(modelsResult);

    render(ChatPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });

    const picker = (await screen.findByPlaceholderText('Select a model')) as HTMLInputElement;
    // The first saved model is preselected, so the composer never starts on an
    // empty target; the pill shows `minimax-m3`, not `ocg/minimax-m3`.
    await waitFor(() => {
      expect(picker.value).toBe('minimax-m3');
    });
    expect(screen.getByTitle('Send')).toBeTruthy();
  });

  test('sends the saved provider id rather than the model alias prefix', async () => {
    vi.spyOn(api, 'chatModels').mockResolvedValue(modelsResult);
    const stream = vi.spyOn(api, 'chatStream').mockResolvedValue();

    render(ChatPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });

    const picker = (await screen.findByPlaceholderText('Select a model')) as HTMLInputElement;
    await waitFor(() => expect(picker.value).toBe('minimax-m3'));
    await fireEvent.input(screen.getByPlaceholderText('Type a message…'), {
      target: { value: 'hello' },
    });
    await fireEvent.click(screen.getByTitle('Send'));

    // `ocg` is only the model-id alias; the relay resolves the provider by
    // its saved id, which the model list reports alongside the id.
    await waitFor(() => expect(stream).toHaveBeenCalledTimes(1));
    expect(stream.mock.calls[0]?.[0]).toMatchObject({
      provider_id: 'opencode-go',
      model: 'minimax-m3',
      message: 'hello',
    });
  });

  test('shows the relay failure instead of a generic chat error', async () => {
    vi.spyOn(api, 'chatModels').mockResolvedValue(modelsResult);
    vi.spyOn(api, 'chatStream').mockRejectedValue(
      new ApiError("provider 'opencode-go' returned HTTP 404: model not found", 502),
    );

    render(ChatPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });

    await screen.findByPlaceholderText('Select a model');
    await fireEvent.input(screen.getByPlaceholderText('Type a message…'), {
      target: { value: 'hello' },
    });
    await fireEvent.click(screen.getByTitle('Send'));

    // The failed turn and the error notice both carry the relay's message.
    const failures = await screen.findAllByText(
      "provider 'opencode-go' returned HTTP 404: model not found",
    );
    expect(failures.length).toBeGreaterThanOrEqual(1);
    expect(screen.queryByText('The chat request failed.')).toBeNull();
  });

  test('keeps the workspace usable while the model list stalls, then reports a retryable timeout', async () => {
    // A stalled admin request can stay pending even after the abort: a queued
    // cross-tab session refresh never observes the signal, so the page must end
    // its loading state on its own instead of waiting for that promise.
    vi.spyOn(api, 'chatModels').mockImplementation(() => new Promise<never>(() => {}));
    const onConnectionChange = vi.fn();

    render(ChatPage, { props: { tr, locale: 'en', onConnectionChange } });

    // The workspace renders immediately; the model list only delays the picker.
    expect(screen.getByText('Workspace chat')).toBeTruthy();
    expect(screen.getByText('Draft a status update')).toBeTruthy();
    expect(screen.getByPlaceholderText('Type a message…')).toBeTruthy();
    expect(screen.getByText('Loading saved models…')).toBeTruthy();
    expect(onConnectionChange).toHaveBeenCalledWith('loading');
    expect(screen.queryByText('Loading chat…')).toBeNull();

    expect(await screen.findByText('The model list took too long to load.')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Retry' })).toBeTruthy();
    expect(screen.queryByText('Loading saved models…')).toBeNull();
    expect(onConnectionChange).toHaveBeenCalledWith('error');
    // The blocked state this guards against hid the composer for good.
    expect(screen.getByPlaceholderText('Type a message…')).toBeTruthy();
  });

  test('recovers through the inline retry when the model list fails', async () => {
    const load = vi
      .spyOn(api, 'chatModels')
      .mockRejectedValueOnce(new Error('gateway down'))
      .mockResolvedValue(modelsResult);

    render(ChatPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });

    const retry = await screen.findByRole('button', { name: 'Retry' });
    expect(screen.getByText('Could not load the model list.')).toBeTruthy();
    expect(screen.queryByPlaceholderText('Select a model')).toBeNull();

    await fireEvent.click(retry);

    expect(await screen.findByPlaceholderText('Select a model')).toBeTruthy();
    await waitFor(() => expect(load).toHaveBeenCalledTimes(2));
    expect(screen.queryByText('Could not load the model list.')).toBeNull();
  });
});
