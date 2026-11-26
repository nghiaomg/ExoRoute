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

/**
 * One replayed prior turn. Text only by design: the relay accepts a bounded
 * slice of the in-memory scratchpad, and attachments belong to the turn that
 * uploaded them.
 */
export interface WorkspaceChatHistoryMessage {
  role: 'user' | 'assistant';
  text: string;
}

export interface WorkspaceChatInput {
  provider_id: string;
  model: string;
  message: string;
  system_prompt?: string | null;
  thinking_mode?: ChatThinkingMode;
  thinking_override?: string | null;
  attachments?: ChatAttachmentInput[];
  /** Prior turns, oldest first. Omitted or empty means a single-turn ask. */
  history?: WorkspaceChatHistoryMessage[] | null;
  temperature?: number | null;
  top_p?: number | null;
  max_tokens?: number | null;
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
  /** Set when a user turn was edited and resent, branching the thread. */
  edited?: boolean;
}

/** Stable request protocol the relay speaks upstream-side. */
export type ChatClientProtocol = Protocol;
