import type { ChatThinkingMode } from '../../lib/types';

/** Client-side attachment ceilings that mirror the workspace relay. */
export const MAX_CHAT_ATTACHMENTS = 8;
export const MAX_ATTACHMENT_BYTES = 6_000_000;
export const MAX_TOTAL_ATTACHMENT_BYTES = 8_000_000;
export const MAX_MESSAGE_CHARS = 128_000;

/**
 * History budgets mirroring the relay's own bounds. The dashboard trims to
 * them before sending so a long scratchpad keeps working instead of being
 * rejected by the server.
 */
export const MAX_HISTORY_TURNS = 40;
export const MAX_HISTORY_CHARS = 256 * 1024;
export const MAX_MAX_TOKENS = 128 * 1024;
export const MAX_TEMPERATURE = 2;
export const MAX_TOP_P = 1;

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

/** Compare panel fan-out bound: the relay is per-model, so this stays small. */
export const MAX_COMPARE_MODELS = 4;
