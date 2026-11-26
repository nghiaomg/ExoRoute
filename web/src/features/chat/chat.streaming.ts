import type { ChatConversationMessage } from '../../lib/types';
import { nextId } from './chat.ids';

/* ------------------------------------------------------------------ */
/* Streaming buffers                                                   */
/* ------------------------------------------------------------------ */

/**
 * Batches streaming deltas into one UI update per animation frame so a
 * fast token stream never triggers a Svelte re-render per token. Falls back
 * to a 16 ms timer where `requestAnimationFrame` is unavailable (tests).
 */
export function createDeltaBuffer(
  apply: (text: string, reasoning: string) => void,
): { push: (text: string, reasoning: string) => void; flush: () => void } {
  let pendingText = '';
  let pendingReasoning = '';
  let scheduled = false;
  const schedule =
    typeof requestAnimationFrame === 'function'
      ? (callback: () => void) => requestAnimationFrame(callback)
      : (callback: () => void) => setTimeout(callback, 16);
  const flush = (): void => {
    scheduled = false;
    const text = pendingText;
    const reasoning = pendingReasoning;
    pendingText = '';
    pendingReasoning = '';
    if (text || reasoning) apply(text, reasoning);
  };
  return {
    push(text: string, reasoning: string): void {
      pendingText += text;
      pendingReasoning += reasoning;
      if (!scheduled) {
        scheduled = true;
        schedule(flush);
      }
    },
    flush,
  };
}

/** A fresh, empty assistant turn a stream fills in. */
export function streamingAssistantMessage(): ChatConversationMessage {
  return { id: nextId(), role: 'assistant', text: '' };
}

/** One parallel-compare column: its own model, stream state, and text. */
export interface CompareColumn {
  modelId: string;
  text: string;
  reasoning: string;
  state: 'running' | 'done' | 'error' | 'stopped';
  detail: string;
  usage: { input_tokens: number; output_tokens: number } | null;
  duration_ms: number;
}

export function emptyCompareColumn(modelId: string): CompareColumn {
  return {
    modelId,
    text: '',
    reasoning: '',
    state: 'running',
    detail: '',
    usage: null,
    duration_ms: 0,
  };
}
