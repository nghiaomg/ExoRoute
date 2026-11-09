import type {
  ChatConversationMessage,
  WorkspaceChatHistoryMessage,
} from '../../lib/types';
import { MAX_HISTORY_CHARS, MAX_HISTORY_TURNS } from './chat.limits';

/** Share of the history budget at which the UI warns before trimming starts. */
const HISTORY_WARNING_RATIO = 0.8;

/** The bounded slice of the thread the relay replays as context. */
export interface ChatHistorySlice {
  history: WorkspaceChatHistoryMessage[];
  turns: number;
  characters: number;
  /** Turns left out of the slice because a budget was already full. */
  droppedTurns: number;
}

/**
 * Selects the newest turns that fit the relay's turn and character budgets,
 * returning them oldest first. Error turns and empty text are skipped, and the
 * turn being asked is passed separately, never from this slice.
 */
export function buildChatHistory(conversation: ChatConversationMessage[]): ChatHistorySlice {
  const selected: WorkspaceChatHistoryMessage[] = [];
  let characters = 0;
  let droppedTurns = 0;
  for (let index = conversation.length - 1; index >= 0; index -= 1) {
    const message = conversation[index];
    const text = message.text.trim();
    if (message.error || !text) {
      continue;
    }
    if (selected.length >= MAX_HISTORY_TURNS || characters + text.length > MAX_HISTORY_CHARS) {
      droppedTurns += 1;
      continue;
    }
    characters += text.length;
    selected.push({ role: message.role, text });
  }
  selected.reverse();
  return { history: selected, turns: selected.length, characters, droppedTurns };
}

/** Whether the thread is close to, or past, the history budget. */
export function historyPressure(slice: ChatHistorySlice): 'ok' | 'near' | 'over' {
  if (slice.droppedTurns > 0) return 'over';
  const turnPressure = slice.turns / MAX_HISTORY_TURNS;
  const charPressure = slice.characters / MAX_HISTORY_CHARS;
  return Math.max(turnPressure, charPressure) >= HISTORY_WARNING_RATIO ? 'near' : 'ok';
}

/** Cumulative token totals for the session meter. */
export function sessionTokens(conversation: ChatConversationMessage[]): {
  input: number;
  output: number;
} {
  return conversation.reduce(
    (totals, message) => ({
      input: totals.input + (message.usage?.input_tokens ?? 0),
      output: totals.output + (message.usage?.output_tokens ?? 0),
    }),
    { input: 0, output: 0 },
  );
}
