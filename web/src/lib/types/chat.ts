import type { Protocol } from './protocols';

/** Thinking preference for one workspace chat message. */
export type ChatThinkingMode = 'default' | 'preserve' | 'remove' | 'override';

/** One image or document attachment inlined with a chat message. */
export interface ChatAttachmentInput {
  kind: 'image' | 'document';
  media_type: string;
  /** Raw attachment bytes encoded as base64 (no data URL prefix). */
  data: string;
  filename?: string | null;
}

export interface WorkspaceChatInput {
  provider_id: string;
  model: string;
  message: string;
  system_prompt?: string | null;
  thinking_mode?: ChatThinkingMode;
  thinking_override?: string | null;
  attachments?: ChatAttachmentInput[];
}

export interface WorkspaceChatUsage {
  input_tokens: number;
  output_tokens: number;
}

export interface WorkspaceChatResult {
  reply: string;
  reasoning?: string | null;
  model: string;
  finish_reason: string;
  usage?: WorkspaceChatUsage | null;
  duration_ms: number;
}

/** One `{prefix}/{model}` entry of the workspace model picker. */
export interface WorkspaceChatModelOption {
  provider_id: string;
  model: string;
  id: string;
}

export interface WorkspaceChatModelsResult {
  models: WorkspaceChatModelOption[];
  truncated: boolean;
}

/** A message turn held only in dashboard memory; nothing is stored server-side. */
export interface ChatConversationMessage {
  id: string;
  role: 'user' | 'assistant';
  text: string;
  reasoning?: string | null;
  model?: string;
  finish_reason?: string;
  usage?: WorkspaceChatUsage | null;
  duration_ms?: number;
  attachmentCount?: number;
  error?: string | null;
}

/** Stable request protocol the relay speaks upstream-side. */
export type ChatClientProtocol = Protocol;
