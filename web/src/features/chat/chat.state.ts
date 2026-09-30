import type {
  ChatAttachmentInput,
  ChatConversationMessage,
  ChatThinkingMode,
  WorkspaceChatInput,
} from '../../lib/types';

/** Client-side attachment ceilings that mirror the workspace relay. */
export const MAX_CHAT_ATTACHMENTS = 8;
export const MAX_ATTACHMENT_BYTES = 6_000_000;
export const MAX_TOTAL_ATTACHMENT_BYTES = 8_000_000;
export const MAX_MESSAGE_CHARS = 128_000;

/** Allowed inline media types per attachment kind. */
export const IMAGE_MEDIA_TYPES = [
  'image/jpeg',
  'image/png',
  'image/gif',
  'image/webp',
] as const;

export const DOCUMENT_MEDIA_TYPES = [
  'application/pdf',
  'text/plain',
] as const;

export const THINKING_MODES: ReadonlyArray<{ value: ChatThinkingMode; labelKey: string }> = [
  { value: 'default', labelKey: 'Provider default' },
  { value: 'preserve', labelKey: 'Preserve thinking' },
  { value: 'remove', labelKey: 'Remove thinking' },
  { value: 'override', labelKey: 'Override thinking' },
];

let nextMessageId = 0;

function nextId(): string {
  nextMessageId += 1;
  return `chat-msg-${nextMessageId}`;
}

export interface ChatDraft {
  /** The owning provider id, resolved by the page from the model list. */
  providerId: string;
  modelId: string;
  message: string;
  systemPrompt: string;
  thinkingMode: ChatThinkingMode;
  thinkingOverride: string;
  attachments: ChatAttachmentInput[];
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

export function validateDraft(draft: ChatDraft): DraftValidation {
  // The `{prefix}/{model}` id alone identifies the target; the page resolves
  // the owning provider id from the loaded model list.
  const [prefix, ...rest] = draft.modelId.split('/');
  const model = rest.join('/');
  if (!prefix || !model) {
    return { ok: false, errorKey: 'Select a model before sending.' };
  }
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
  return {
    ok: true,
    input: {
      provider_id: prefix,
      model,
      message,
      system_prompt: draft.systemPrompt.trim() || null,
      thinking_mode: draft.thinkingMode,
      thinking_override: draft.thinkingMode === 'override' ? draft.thinkingOverride : null,
      attachments: draft.attachments,
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
