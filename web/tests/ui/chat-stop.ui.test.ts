import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardChatPage from '../../src/features/chat/DashboardChatPage.svelte';
import { api } from '../../src/lib/api';
import { dashboardToast } from '../../src/lib/dashboard-ui';
import { modelsResult, sendWithDefaultModel, tr } from './support/chat-page-harness';

const { streamChat } = vi.hoisted(() => ({ streamChat: vi.fn() }));
vi.mock('../../src/lib/api/workspace-chat-stream', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../src/lib/api/workspace-chat-stream')>();
  return { ...actual, streamWorkspaceChat: streamChat };
});

const composerPlaceholder = 'Message the model… (Enter to send, Shift+Enter for newline)';
const toastUnsubscribers: Array<() => void> = [];

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

describe('DashboardChatPage stop button', () => {
  test('stops an in-flight turn and reports it as stopped rather than failed', async () => {
    vi.spyOn(api, 'chatModels').mockResolvedValue(modelsResult);
    const toasts = observeToasts();
    streamChat.mockImplementation(
      (_input: unknown, _handlers: unknown, signal?: AbortSignal) =>
        new Promise<void>((_resolve, reject) => {
          const onAbort = () => reject(new DOMException('Aborted', 'AbortError'));
          signal?.addEventListener('abort', onAbort, { once: true });
          // Cleanup path: the page always aborts or unmounts this promise.
          signal?.addEventListener('abort', () => {
            signal?.removeEventListener('abort', onAbort);
          }, { once: true });
        }),
    );

    render(DashboardChatPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });
    await sendWithDefaultModel('hello there');

    // While the turn is in flight the send affordance becomes Stop.
    const stop = await screen.findByTitle('Stop generating');
    await waitFor(() => {
      expect((screen.getByPlaceholderText(composerPlaceholder) as HTMLTextAreaElement).disabled).toBe(true);
    });
    expect(streamChat).toHaveBeenCalledTimes(1);
    const sentInput = streamChat.mock.calls[0]?.[0];
    expect(sentInput?.message).toBe('hello there');
    // A first turn has no prior messages, so the production input carries null.
    expect(sentInput?.history).toBeNull();

    await fireEvent.click(stop);

    await waitFor(() => expect(toasts).toContain('Generation stopped'));
    // A stop is not a failed turn: no retry card, and Send comes back with an
    // enabled composer instead of a stuck Stop button.
    expect(screen.queryByTitle('Retry')).toBeNull();
    expect(screen.getByTitle('Send')).toBeTruthy();
    expect((screen.getByPlaceholderText(composerPlaceholder) as HTMLTextAreaElement).disabled).toBe(false);
  });
});
