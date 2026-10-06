import { render, screen, waitFor, fireEvent } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import ModelPickerStub from './support/ModelPickerStub.svelte';
import { modelsResult, sendWithDefaultModel, tr } from './support/chat-page-harness';
import ChatPage from '../../src/features/chat/ChatPage.svelte';
import { api } from '../../src/lib/api';
import type { WorkspaceChatInput } from '../../src/lib/types';
import type { WorkspaceChatStreamHandlers } from '../../src/lib/api/workspace-chat-stream';

// The real picker drives an Ark portal dropdown jsdom cannot run; this file
// exercises the streaming turn, not picker navigation, so the shared stub
// input replaces it.
vi.mock('../../src/features/chat/ChatModelPicker.svelte', () => ({
  default: ModelPickerStub,
}));

afterEach(() => {
  vi.restoreAllMocks();
});

describe('ChatPage streaming turn', () => {
  test('streams assistant deltas into one bubble and records usage', async () => {
    vi.spyOn(api, 'chatModels').mockResolvedValue(modelsResult);
    const chat = vi.spyOn(api, 'chatStream').mockImplementation(
      (
        _input: WorkspaceChatInput,
        handlers: WorkspaceChatStreamHandlers,
        signal?: AbortSignal,
      ): Promise<void> => {
        if (signal?.aborted) return Promise.reject(new DOMException('Aborted', 'AbortError'));
        // Emit the stream through the handler contract the page consumes.
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

    render(ChatPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });
    await sendWithDefaultModel('say hi');

    // Deltas accumulate into one bubble, then done fills the usage meter.
    await waitFor(() => {
      expect(screen.getByText('Hello')).toBeTruthy();
    });
    // The meter renders twice (topbar and turn footer), so require at least
    // one match instead of exactly one.
    await waitFor(() => {
      expect(screen.getAllByText('5 in / 2 out').length).toBeGreaterThanOrEqual(1);
    });
    expect(chat).toHaveBeenCalledTimes(1);
    expect(chat.mock.calls[0]?.[0]?.message).toBe('say hi');
  });
});
