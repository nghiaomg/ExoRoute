import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardChatPage from '../../src/features/chat/DashboardChatPage.svelte';
import { api } from '../../src/lib/api';
import { dashboardToast } from '../../src/lib/dashboard-ui';
import type { WorkspaceChatStreamHandlers } from '../../src/lib/api/workspace-chat-stream';
import { modelsResult, sendWithDefaultModel, tr } from './support/chat-page-harness';

// The production page imports the SSE transport directly; replace only that
// function while keeping the module's shared api surface intact.
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

describe('DashboardChatPage streaming turn', () => {
  test('streams assistant deltas into one bubble and finalizes the turn', async () => {
    vi.spyOn(api, 'chatModels').mockResolvedValue(modelsResult);
    const toasts = observeToasts();
    streamChat.mockImplementation(
      (
        _input: unknown,
        handlers: WorkspaceChatStreamHandlers,
        _signal?: AbortSignal,
      ): Promise<void> => {
        // Emit through the handler contract the production page consumes.
        queueMicrotask(() => {
          handlers.onTextDelta('Hel');
          handlers.onTextDelta('lo');
          handlers.onDone({
            model: 'minimax-m3',
            finish_reason: 'stop',
            input_tokens: 5,
            output_tokens: 2,
            duration_ms: 12,
          });
        });
        return Promise.resolve();
      },
    );

    render(DashboardChatPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });
    await sendWithDefaultModel('say hi');

    // The two deltas accumulate into one assistant bubble, then the done event
    // finalizes the turn without marking it failed.
    await waitFor(() => {
      expect(screen.getByText('Hello')).toBeTruthy();
    });
    expect(screen.getByText('Hello').classList.contains('error')).toBe(false);
    await waitFor(() => {
      expect(toasts.some((message) => message.startsWith('Responded via minimax-m3'))).toBe(true);
    });
    expect(screen.getByTitle('Send')).toBeTruthy();
    expect(streamChat).toHaveBeenCalledTimes(1);
    expect(streamChat.mock.calls[0]?.[0]?.message).toBe('say hi');
  });

  test('a failed chunk marks the turn failed and the composer can retry it', async () => {
    vi.spyOn(api, 'chatModels').mockResolvedValue(modelsResult);
    let attempt = 0;
    streamChat.mockImplementation(
      (_input: unknown, handlers: WorkspaceChatStreamHandlers): Promise<void> => {
        attempt += 1;
        queueMicrotask(() => {
          if (attempt === 1) {
            handlers.onError('upstream stream broke');
            return;
          }
          handlers.onTextDelta('Recovered');
          handlers.onDone({
            model: 'minimax-m3',
            finish_reason: 'stop',
            input_tokens: 1,
            output_tokens: 1,
            duration_ms: 8,
          });
        });
        return Promise.resolve();
      },
    );

    render(DashboardChatPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });
    await sendWithDefaultModel('first');

    const failed = await screen.findByText('upstream stream broke');
    expect(failed.classList.contains('error')).toBe(true);
    // A failed chunk never leaves the send affordance stuck in Stop.
    expect(screen.getByTitle('Send')).toBeTruthy();
    expect((screen.getByPlaceholderText(composerPlaceholder) as HTMLTextAreaElement).disabled).toBe(false);

    await fireEvent.input(screen.getByPlaceholderText(composerPlaceholder), {
      target: { value: 'retry' },
    });
    await fireEvent.click(screen.getByTitle('Send'));

    await waitFor(() => expect(screen.getByText('Recovered')).toBeTruthy());
    expect(streamChat).toHaveBeenCalledTimes(2);
    expect(streamChat.mock.calls[1]?.[0]?.message).toBe('retry');
  });
});
