import type { ChatConversationMessage } from '../../lib/types';
import { nextId } from './chat.ids';
import type { ChatDraft } from './chat.draft';

/* ------------------------------------------------------------------ */
/* Edit, resend, and branching                                         */
/* ------------------------------------------------------------------ */

/**
 * The newest active branch of a forked turn is what the thread shows.
 */
export type ChatBranch = {
  /** The turn id whose following assistant reply was forked. */
  originId: string;
  /** The forked thread: every turn after (and including) the resend. */
  messages: ChatConversationMessage[];
};

/**
 * Replaces the user turn at `index` with the edited text and drops every
 * turn after it (branching): the thread continues from the edit as the
 * newest interpretation. The returned array is a fresh array; nothing is
 * mutated in place.
 */
export function branchFromEdit(
  conversation: ChatConversationMessage[],
  index: number,
  editedText: string,
): ChatConversationMessage[] {
  if (index < 0 || index >= conversation.length) return conversation;
  const original = conversation[index];
  if (original.role !== 'user') return conversation;
  const edited: ChatConversationMessage = {
    ...original,
    id: nextId(),
    text: editedText,
    edited: true,
  };
  return [...conversation.slice(0, index), edited];
}

/** Builds the draft for resending an edited turn: same settings, new text. */
export function draftFromConversation(draft: ChatDraft, editedText: string): ChatDraft {
  return { ...draft, message: editedText };
}

/* ------------------------------------------------------------------ */
/* Regenerate and continue                                             */
/* ------------------------------------------------------------------ */

/**
 * The context turns before the final user turn, oldest first: what a
 * regenerate replays when the page re-asks that user turn as the message.
 * Error and empty turns are left in; `buildChatHistory` filters them.
 */
export function contextBeforeFinalUserTurn(
  conversation: ChatConversationMessage[],
): ChatConversationMessage[] {
  const copy = [...conversation];
  while (copy.length && copy[copy.length - 1].role === 'user') {
    copy.pop();
  }
  return copy;
}

/** Drops trailing assistant replies/errors so a regenerate can re-ask. */
export function dropTrailingAssistantTurns(
  conversation: ChatConversationMessage[],
): ChatConversationMessage[] {
  const result = [...conversation];
  while (result.length && result[result.length - 1].role === 'assistant') {
    result.pop();
  }
  return result;
}

/** Builds a "continue exactly where you left off" instruction turn. */
export function continueInstruction(): string {
  return [
    'Continue the previous reply exactly where it stopped.',
    'Do not repeat what you already wrote; do not add commentary about continuing; just continue.',
  ].join('\n');
}
