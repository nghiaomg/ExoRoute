import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardChatPage from '../../src/features/chat/DashboardChatPage.svelte';
import { ApiError, api } from '../../src/lib/api';
import { dashboardToast } from '../../src/lib/dashboard-ui';
import type { Translate } from '../../src/lib/format';
import type { WorkspaceChatModelsResult } from '../../src/lib/types';

// The production page uses the dedicated SSE transport rather than api.chatStream.
// Keep its real module exports (the shared api client spreads them) and replace
// only the transport function the page imports.
const { streamChat } = vi.hoisted(() => ({ streamChat: vi.fn() }));
vi.mock('../../src/lib/api/workspace-chat-stream', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../src/lib/api/workspace-chat-stream')>();
  return { ...actual, streamWorkspaceChat: streamChat };
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

const composerPlaceholder = 'Message the model… (Enter to send, Shift+Enter for newline)';
const toastUnsubscribers: Array<() => void> = [];

/** Capture the production toast store while a page is mounted. */
function observeToasts(): string[] {
  const messages: string[] = [];
  toastUnsubscribers.push(
    dashboardToast.subscribe((toast) => {
      if (toast) messages.push(toast.message);
    }),
  );
  return messages;
}

afterEach(() => {
  toastUnsubscribers.splice(0).forEach((unsubscribe) => unsubscribe());
  dashboardToast.set(null);
  vi.clearAllMocks();
  vi.restoreAllMocks();
});

describe('DashboardChatPage', () => {
  test('renders the searchable picker pill, composer, and the hero suggestions', async () => {
    const load = vi.spyOn(api, 'chatModels').mockResolvedValue(modelsResult);

    render(DashboardChatPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });

    // The picker is a searchable pill input, not a hidden native select.
    const picker = await screen.findByPlaceholderText('Select a model');
    expect(picker.getAttribute('role')).toBe('combobox');
    await waitFor(() => expect(load).toHaveBeenCalled());
    expect(screen.getByRole('button', { name: 'Send' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Attach image' })).toBeTruthy();
    expect(screen.getByPlaceholderText(composerPlaceholder)).toBeTruthy();
    // The production empty-state hero carries its own privacy guidance.
    expect(screen.getByText('Start chatting with AI')).toBeTruthy();
    expect(screen.getByText('Experiment with AI models through the ExoRoute Gateway.')).toBeTruthy();
    expect(screen.getByText('Chat')).toBeTruthy();
    expect(screen.getByText('Test models through the gateway before wiring them into production.')).toBeTruthy();
    expect(screen.getByText('Summarize text into 3 bullet points')).toBeTruthy();
    expect(screen.getByText('Write a Python retry decorator')).toBeTruthy();
    expect(screen.getByText('Explain SSE vs WebSocket')).toBeTruthy();

    // Parameters are collapsed until the page-level shared Dialog opens.
    expect(document.querySelector('dialog[open]')).toBeNull();
    expect(screen.queryByPlaceholderText('You are a helpful routing assistant…')).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Parameters' }));
    const dialog = await screen.findByRole('dialog');
    expect(dialog.querySelector('textarea[placeholder="You are a helpful routing assistant…"]')).toBeTruthy();
    expect(dialog.textContent).toContain('Chat parameters');
    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(document.querySelector('dialog[open]')).toBeNull());
  });

  test('starts on a model, shown as the readable name instead of the raw id', async () => {
    vi.spyOn(api, 'chatModels').mockResolvedValue(modelsResult);

    render(DashboardChatPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });

    const picker = (await screen.findByPlaceholderText('Select a model')) as HTMLInputElement;
    // The first loaded model is preselected: the pill shows `minimax-m3`, not
    // `ocg/minimax-m3`, so the composer never starts on an empty target.
    await waitFor(() => expect(picker.value).toBe('minimax-m3'));
    expect(screen.getByTitle('Send')).toBeTruthy();
  });

  test('sends the saved provider id rather than the model alias prefix', async () => {
    vi.spyOn(api, 'chatModels').mockResolvedValue(modelsResult);
    streamChat.mockResolvedValue(undefined);

    render(DashboardChatPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });

    const picker = (await screen.findByPlaceholderText('Select a model')) as HTMLInputElement;
    await waitFor(() => expect(picker.value).toBe('minimax-m3'));
    await fireEvent.input(screen.getByPlaceholderText(composerPlaceholder), {
      target: { value: 'hello' },
    });
    await fireEvent.click(screen.getByTitle('Send'));

    // `ocg` is only the model-id alias; the relay resolves the provider by
    // its saved id, which the model list reports alongside the id.
    await waitFor(() => expect(streamChat).toHaveBeenCalledTimes(1));
    expect(streamChat.mock.calls[0]?.[0]).toMatchObject({
      provider_id: 'opencode-go',
      model: 'minimax-m3',
      message: 'hello',
    });
  });

  test('shows the relay failure instead of a generic chat error', async () => {
    vi.spyOn(api, 'chatModels').mockResolvedValue(modelsResult);
    streamChat.mockRejectedValue(
      new ApiError("provider 'opencode-go' returned HTTP 404: model not found", 502),
    );

    render(DashboardChatPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });

    await screen.findByPlaceholderText('Select a model');
    await fireEvent.input(screen.getByPlaceholderText(composerPlaceholder), {
      target: { value: 'hello' },
    });
    await fireEvent.click(screen.getByTitle('Send'));

    // The failed turn carries the relay's message and is marked as failed;
    // no generic transport failure replaces it.
    const failures = await screen.findAllByText(
      "provider 'opencode-go' returned HTTP 404: model not found",
    );
    expect(failures.length).toBeGreaterThanOrEqual(1);
    expect(failures[0]?.classList.contains('error')).toBe(true);
    expect(screen.queryByText('Chat stream failed')).toBeNull();
    expect(screen.getByTitle('Send')).toBeTruthy();
  });

  test('keeps the workspace usable while the model list stalls, then reports its failure', async () => {
    // A stalled admin request can stay pending even after the abort: a queued
    // cross-tab session refresh never observes the signal, so the page must not
    // hide the composer behind a permanent loading screen.
    let rejectModels: (error: Error) => void = () => {};
    vi.spyOn(api, 'chatModels').mockImplementation(
      () => new Promise<WorkspaceChatModelsResult>((_resolve, reject) => { rejectModels = reject; }),
    );
    const toasts = observeToasts();
    const onConnectionChange = vi.fn();

    render(DashboardChatPage, { props: { tr, locale: 'en', onConnectionChange } });

    expect(screen.getByText('Chat')).toBeTruthy();
    expect(screen.getByText('Test models through the gateway before wiring them into production.')).toBeTruthy();
    expect(screen.getByPlaceholderText(composerPlaceholder)).toBeTruthy();
    expect(onConnectionChange).not.toHaveBeenCalledWith('loaded');
    expect(screen.queryByText('Loading chat…')).toBeNull();

    // The production page reports a failed load through the shared connection
    // state and toast instead of an inline timeout/retry card.
    rejectModels(new Error('gateway down'));
    await waitFor(() => expect(onConnectionChange).toHaveBeenCalledWith('error'));
    expect(toasts).toContain('gateway down');
    // The blocked state this guards against hid the composer for good.
    expect(screen.getByPlaceholderText(composerPlaceholder)).toBeTruthy();
  });

  test('a failed model list is recoverable on the next load', async () => {
    const load = vi
      .spyOn(api, 'chatModels')
      .mockRejectedValueOnce(new Error('gateway down'))
      .mockResolvedValue(modelsResult);
    const toasts = observeToasts();
    const onConnectionChange = vi.fn();
    const props = { tr, locale: 'en' as const, onConnectionChange };

    const first = render(DashboardChatPage, { props });
    const picker = await screen.findByPlaceholderText('Select a model');
    await waitFor(() => expect(onConnectionChange).toHaveBeenCalledWith('error'));
    expect(toasts).toContain('gateway down');
    // With no models loaded the pill has no preselection, but chat stays usable.
    expect((picker as HTMLInputElement).value).toBe('');
    expect(screen.getByPlaceholderText(composerPlaceholder)).toBeTruthy();
    first.unmount();

    // A fresh load of the same production page recovers without a code change.
    render(DashboardChatPage, { props });
    const recoveredPicker = (await screen.findByPlaceholderText('Select a model')) as HTMLInputElement;
    await waitFor(() => expect(recoveredPicker.value).toBe('minimax-m3'));
    expect(load).toHaveBeenCalledTimes(2);
    expect(onConnectionChange).toHaveBeenCalledWith('loaded');
  });
});
