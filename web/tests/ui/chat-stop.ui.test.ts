import { render, screen, waitFor, fireEvent } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import ModelPickerStub from './support/ModelPickerStub.svelte';
import { modelsResult, sendWithDefaultModel, tr } from './support/chat-page-harness';
import ChatPage from '../../src/features/chat/ChatPage.svelte';
import { api } from '../../src/lib/api';
import type { WorkspaceChatInput } from '../../src/lib/types';
import type { WorkspaceChatStreamHandlers } from '../../src/lib/api/workspace-chat-stream';

// The real picker drives an Ark portal dropdown jsdom cannot run; this file
// exercises stop/abort of an in-flight turn, not picker navigation, so the
// shared stub input replaces it.
vi.mock('../../src/features/chat/ChatModelPicker.svelte', () => ({
  default: ModelPickerStub,
}));

afterEach(() => {
  vi.restoreAllMocks();
});

describe('ChatPage stop button', () => {
  test('stops an in-flight turn and reports it as stopped rather than failed', async () => {
    vi.spyOn(api, 'chatModels').mockResolvedValue(modelsResult);
    const chat = vi.spyOn(api, 'chatStream').mockImplementation(
      (_input: WorkspaceChatInput, _handlers: WorkspaceChatStreamHandlers, signal?: AbortSignal) =>
        new Promise<void>((_resolve, reject) => {
          const onAbort = () => reject(new DOMException('Aborted', 'AbortError'));
          signal?.addEventListener('abort', onAbort, { once: true });
          // Cleanup path: the page always aborts or unmounts this promise.
          signal?.addEventListener('abort', () => {
            signal?.removeEventListener('abort', onAbort);
          }, { once: true });
        }),
    );

    render(ChatPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });
    await sendWithDefaultModel('hello there');

    // While the turn is in flight the send affordance becomes Stop.
    await waitFor(() => {
      expect(screen.getByTitle('Stop')).toBeTruthy();
    });
    expect(chat).toHaveBeenCalledTimes(1);
    const sentInput = chat.mock.calls[0]?.[0];
    expect(sentInput?.message).toBe('hello there');
    expect(sentInput?.history).toEqual([]);

    await fireEvent.click(screen.getByTitle('Stop'));

    await waitFor(() => {
      expect(screen.getByText('Generation stopped.')).toBeTruthy();
    });
    // A stop is not a failed turn: no retry card, and Send comes back.
    expect(screen.queryByTitle('Retry')).toBeNull();
    expect(screen.getByTitle('Send')).toBeTruthy();
  });
});
