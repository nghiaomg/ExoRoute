import type {
  ChatAttachmentInput,
  ChatConversationMessage,
  ChatThinkingMode,
  WorkspaceChatInput,
} from '../../lib/types';
import { nextId } from './chat.ids';
import { buildChatHistory } from './chat.history';
import {
  MAX_CHAT_ATTACHMENTS,
  MAX_MAX_TOKENS,
  MAX_MESSAGE_CHARS,
  MAX_TEMPERATURE,
  MAX_TOP_P,
  MAX_TOTAL_ATTACHMENT_BYTES,
} from './chat.limits';
import { splitModelId } from './chat.models';

export interface ChatDraft {
  /** The owning provider id, resolved by the page from the model list. */
  providerId: string;
  modelId: string;
  message: string;
  systemPrompt: string;
  thinkingMode: ChatThinkingMode;
  thinkingOverride: string;
  attachments: ChatAttachmentInput[];
  /** Sampling knobs as raw field text; an empty string means "provider default". */
  temperature: string;
  topP: string;
  maxTokens: string;
}

export function emptyDraft(): ChatDraft {
  return {
    providerId: '',
    modelId: '',
    message: '',
    systemPrompt: '',
    thinkingMode: 'default',
    thinkingOverride: '',
    attachments: [],
    temperature: '',
    topP: '',
    maxTokens: '',
  };
}

/** One optional sampling knob parsed from its raw field text. */
export type GenerationOptions =
  | { ok: true; temperature: number | null; top_p: number | null; max_tokens: number | null }
  | { ok: false; errorKey: string };

/** Parses `temperature`, `top_p`, and `max_tokens` field text, mirroring the
 * ranges the relay enforces so a bad value never reaches the server. */
export function parseGenerationOptions(draft: ChatDraft): GenerationOptions {
  const temperature = draft.temperature.trim();
  if (temperature) {
    const value = Number(temperature);
    if (!Number.isFinite(value) || value < 0 || value > MAX_TEMPERATURE) {
      return { ok: false, errorKey: 'Temperature must be a number between 0 and 2.' };
    }
  }
  const topP = draft.topP.trim();
  if (topP) {
    const value = Number(topP);
    if (!Number.isFinite(value) || value < 0 || value > MAX_TOP_P) {
      return { ok: false, errorKey: 'Top P must be a number between 0 and 1.' };
    }
  }
  const maxTokens = draft.maxTokens.trim();
  if (maxTokens) {
    const value = Number(maxTokens);
    if (!Number.isInteger(value) || value < 1 || value > MAX_MAX_TOKENS) {
      return { ok: false, errorKey: 'Max tokens must be a whole number in the allowed range.' };
    }
  }
  return {
    ok: true,
    temperature: temperature ? Number(temperature) : null,
    top_p: topP ? Number(topP) : null,
    max_tokens: maxTokens ? Number(maxTokens) : null,
  };
}

export function totalAttachmentBytes(attachments: ChatAttachmentInput[]): number {
  return attachments.reduce((total, attachment) => {
    // Exact decoded size for well-formed base64: strip padding, then every
    // four characters encode three bytes with the remainders folding in.
    const stripped = attachment.data.replace(/=+$/, '');
    return total + Math.floor((stripped.length * 3) / 4);
  }, 0);
}

/** Result of validating a draft before send; `errorKey` is a translation key. */
export type DraftValidation =
  | { ok: true; input: WorkspaceChatInput }
  | { ok: false; errorKey: string };

export function validateDraft(
  draft: ChatDraft,
  conversation: ChatConversationMessage[] = [],
): DraftValidation {
  const { prefix, model } = splitModelId(draft.modelId);
  if (!prefix || !model) {
    return { ok: false, errorKey: 'Select a model before sending.' };
  }
  // The relay resolves the provider by its saved id, which the model list
  // reports separately from the `{prefix}/{model}` alias. The prefix is only
  // the fallback for a model the loaded list did not describe.
  const providerId = draft.providerId.trim() || prefix;
  const message = draft.message.trim();
  if (!message) {
    return { ok: false, errorKey: 'Type a message before sending.' };
  }
  if (message.length > MAX_MESSAGE_CHARS) {
    return { ok: false, errorKey: 'Message is too long.' };
  }
  if (draft.attachments.length > MAX_CHAT_ATTACHMENTS) {
    return { ok: false, errorKey: 'Too many attachments.' };
  }
  if (totalAttachmentBytes(draft.attachments) > MAX_TOTAL_ATTACHMENT_BYTES) {
    return { ok: false, errorKey: 'Attachments are too large.' };
  }
  if (draft.thinkingMode === 'override' && !draft.thinkingOverride.trim()) {
    return { ok: false, errorKey: 'Thinking override text is required.' };
  }
  const options = parseGenerationOptions(draft);
  if (!options.ok) {
    return options;
  }
  return {
    ok: true,
    input: {
      provider_id: providerId,
      model,
      message,
      system_prompt: draft.systemPrompt.trim() || null,
      thinking_mode: draft.thinkingMode,
      thinking_override: draft.thinkingMode === 'override' ? draft.thinkingOverride : null,
      attachments: draft.attachments,
      history: buildChatHistory(conversation).history,
      temperature: options.temperature,
      top_p: options.top_p,
      max_tokens: options.max_tokens,
    },
  };
}

export function userMessageFromDraft(draft: ChatDraft): ChatConversationMessage {
  return {
    id: nextId(),
    role: 'user',
    text: draft.message,
    attachmentCount: draft.attachments.length,
  };
}

export function assistantMessageFromResult(
  result: import('../../lib/types').WorkspaceChatResult,
): ChatConversationMessage {
  return {
    id: nextId(),
    role: 'assistant',
    text: result.reply,
    reasoning: result.reasoning ?? null,
    model: result.model,
    finish_reason: result.finish_reason,
    usage: result.usage ?? null,
    duration_ms: result.duration_ms,
  };
}

export function errorMessage(text: string): ChatConversationMessage {
  return {
    id: nextId(),
    role: 'assistant',
    text: '',
    error: text,
  };
}

/** Removes the attachment at `index`, keeping the rest in order. */
export function removeAttachment(
  attachments: ChatAttachmentInput[],
  index: number,
): ChatAttachmentInput[] {
  return attachments.filter((_, position) => position !== index);
}
