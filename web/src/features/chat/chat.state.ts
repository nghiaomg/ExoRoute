import type {
  ChatAttachmentInput,
  ChatConversationMessage,
  ChatThinkingMode,
  WorkspaceChatHistoryMessage,
  WorkspaceChatInput,
  WorkspaceChatModelOption,
} from '../../lib/types';

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

/** Share of the history budget at which the UI warns before trimming starts. */
const HISTORY_WARNING_RATIO = 0.8;

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

/** The newest active branch of a forked turn is what the thread shows. */
export type ChatBranch = {
  /** The turn id whose following assistant reply was forked. */
  originId: string;
  /** The forked thread: every turn after (and including) the resend. */
  messages: ChatConversationMessage[];
};

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

/* ------------------------------------------------------------------ */
/* Model identity and the picker default                               */
/* ------------------------------------------------------------------ */

/** Splits a `{prefix}/{model}` picker id into its routing prefix and model. */
export function splitModelId(id: string): { prefix: string; model: string } {
  const [prefix = '', ...rest] = id.split('/');
  return { prefix, model: rest.join('/') };
}

/**
 * The readable model name: the final path segment of the id. A
 * `{prefix}/{vendor}/{model}` id stays scannable in the picker without hiding
 * the parts that tell sibling models apart (`-fast`, `-pro`, `-exp`).
 */
export function modelDisplayName(id: string): string {
  const segments = id.split('/').filter((segment) => segment.length > 0);
  return segments.length ? segments[segments.length - 1] : id;
}

/** The routing prefix the id starts with; empty for a bare model name. */
export function modelProviderPrefix(id: string): string {
  const { prefix, model } = splitModelId(id);
  return model ? prefix : '';
}

const MODEL_STORAGE_KEY = 'exoroute.workspace-chat.model.v1';
const MAX_MODEL_ID_CHARS = 256;

/** The model the operator picked last visit; empty when unset or unreadable. */
export function loadLastModelId(): string {
  if (typeof localStorage === 'undefined') return '';
  try {
    const raw = localStorage.getItem(MODEL_STORAGE_KEY);
    return raw && raw.length <= MAX_MODEL_ID_CHARS ? raw : '';
  } catch {
    return '';
  }
}

/** Remembers the choice so the next visit opens on the same target. */
export function saveLastModelId(modelId: string): void {
  if (typeof localStorage === 'undefined') return;
  try {
    if (modelId) localStorage.setItem(MODEL_STORAGE_KEY, modelId.slice(0, MAX_MODEL_ID_CHARS));
    else localStorage.removeItem(MODEL_STORAGE_KEY);
  } catch {
    // Storage may be unavailable (private mode, quota); keep memory only.
  }
}

/**
 * The model the picker starts on: the current choice while it is still
 * available, otherwise the remembered one, otherwise the first option. With
 * options loaded the picker never starts on an empty target.
 */
export function defaultModelId(
  options: WorkspaceChatModelOption[],
  current: string,
  remembered = '',
): string {
  const available = (id: string): boolean => Boolean(id) && options.some((option) => option.id === id);
  if (available(current)) return current;
  if (available(remembered)) return remembered;
  return options[0]?.id ?? '';
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

/* ------------------------------------------------------------------ */
/* Edit, resend, and branching                                         */
/* ------------------------------------------------------------------ */

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

/* ------------------------------------------------------------------ */
/* Export: Markdown and JSON                                           */
/* ------------------------------------------------------------------ */

function safeFileStamp(): string {
  return new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
}

/** Renders the thread as a portable Markdown document. */
export function conversationToMarkdown(
  conversation: ChatConversationMessage[],
  title: string,
): string {
  const lines: string[] = [`# ${title}`, ''];
  for (const message of conversation) {
    if (message.error) {
      lines.push(`> **${'Error'}:** ${message.error}`, '');
      continue;
    }
    const who = message.role === 'user' ? 'You' : message.model || 'Assistant';
    lines.push(`## ${who}`, '', message.text, '');
    if (message.reasoning) {
      lines.push('<details><summary>Reasoning</summary>', '', message.reasoning, '', '</details>', '');
    }
  }
  return lines.join('\n');
}

/** Renders the thread as the full-fidelity JSON archive. */
export function conversationToJson(conversation: ChatConversationMessage[]): string {
  return JSON.stringify(
    {
      exported_at: new Date().toISOString(),
      messages: conversation,
    },
    null,
    2,
  );
}

/** Triggers a bounded client-side download of a text attachment. */
export function downloadTextFile(filename: string, content: string, mime: string): void {
  const blob = new Blob([content], { type: mime });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1_000);
}

export function conversationMarkdownFilename(): string {
  return `exoroute-chat-${safeFileStamp()}.md`;
}

export function conversationJsonFilename(): string {
  return `exoroute-chat-${safeFileStamp()}.json`;
}

/** Opens the browser print dialog scoped to the conversation via CSS. */
export function printConversation(): void {
  window.print();
}

/* ------------------------------------------------------------------ */
/* Price table (client-side, operator-configured, no invented numbers) */
/* ------------------------------------------------------------------ */

/** One per-model price row, entirely operator-entered; currency optional. */
export interface ModelPrice {
  /** Currency code or free-form label; never used to compute without input. */
  currency: string;
  /** Price per million input tokens; blank/0 when unknown. */
  inputPerMillion: number;
  /** Price per million output tokens; blank/0 when unknown. */
  outputPerMillion: number;
}

export type PriceTable = Record<string, ModelPrice>;

const PRICE_STORAGE_KEY = 'exoroute.workspace-chat.prices.v1';
const MAX_PRICE_ROWS = 500;
const MAX_PRICE_MODEL_KEY_CHARS = 256;

/** Loads the operator price table from localStorage; corrupt data is ignored. */
export function loadPriceTable(): PriceTable {
  if (typeof localStorage === 'undefined') return {};
  try {
    const raw = localStorage.getItem(PRICE_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
    const entries = Object.entries(parsed as Record<string, unknown>).slice(0, MAX_PRICE_ROWS);
    const table: PriceTable = {};
    for (const [key, value] of entries) {
      if (!key || key.length > MAX_PRICE_MODEL_KEY_CHARS) continue;
      if (!value || typeof value !== 'object') continue;
      const row = value as Partial<ModelPrice>;
      const input = Number(row.inputPerMillion);
      const output = Number(row.outputPerMillion);
      table[key] = {
        currency: typeof row.currency === 'string' ? row.currency.slice(0, 8) : '',
        inputPerMillion: Number.isFinite(input) && input >= 0 ? input : 0,
        outputPerMillion: Number.isFinite(output) && output >= 0 ? output : 0,
      };
    }
    return table;
  } catch {
    return {};
  }
}

/** Persists the operator price table; failures keep the in-memory copy. */
export function savePriceTable(table: PriceTable): void {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(PRICE_STORAGE_KEY, JSON.stringify(table));
  } catch {
    // Storage may be unavailable (private mode, quota); keep memory only.
  }
}

/** Estimates the session cost, or null when the model has no price row. */
export function estimateSessionCost(
  table: PriceTable,
  modelId: string,
  inputTokens: number,
  outputTokens: number,
): { currency: string; amount: number } | null {
  const row = table[modelId];
  if (!row) return null;
  const amount =
    (inputTokens / 1_000_000) * row.inputPerMillion +
    (outputTokens / 1_000_000) * row.outputPerMillion;
  return { currency: row.currency, amount };
}

/* ------------------------------------------------------------------ */
/* Prompt library (client-side; localStorage; bounded)                 */
/* ------------------------------------------------------------------ */

export interface SavedPrompt {
  id: string;
  name: string;
  /** The prompt body: used as the system prompt when applied. */
  body: string;
  createdAt: number;
}

const PROMPT_STORAGE_KEY = 'exoroute.workspace-chat.prompts.v1';
const MAX_SAVED_PROMPTS = 200;
const MAX_PROMPT_NAME_CHARS = 120;
const MAX_PROMPT_BODY_CHARS = 32 * 1024;

/** Loads the saved prompt library; corrupt entries are dropped. */
export function loadPrompts(): SavedPrompt[] {
  if (typeof localStorage === 'undefined') return [];
  try {
    const raw = localStorage.getItem(PROMPT_STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((entry): entry is Partial<SavedPrompt> => Boolean(entry) && typeof entry === 'object')
      .filter(
        (entry) =>
          typeof entry.id === 'string' &&
          typeof entry.name === 'string' &&
          typeof entry.body === 'string',
      )
      .slice(0, MAX_SAVED_PROMPTS)
      .map((entry) => ({
        id: entry.id as string,
        name: (entry.name as string).slice(0, MAX_PROMPT_NAME_CHARS),
        body: (entry.body as string).slice(0, MAX_PROMPT_BODY_CHARS),
        createdAt: typeof entry.createdAt === 'number' ? entry.createdAt : 0,
      }));
  } catch {
    return [];
  }
}

/** Persists the prompt library; failures keep the in-memory copy. */
export function savePrompts(prompts: SavedPrompt[]): void {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(
      PROMPT_STORAGE_KEY,
      JSON.stringify(prompts.slice(0, MAX_SAVED_PROMPTS)),
    );
  } catch {
    // Storage unavailable; keep memory only.
  }
}

export function makePrompt(name: string, body: string): SavedPrompt {
  const trimmedName = name.trim().slice(0, MAX_PROMPT_NAME_CHARS) || 'Prompt';
  return {
    id: `prompt-${Date.now()}-${nextId()}`,
    name: trimmedName,
    body: body.slice(0, MAX_PROMPT_BODY_CHARS),
    createdAt: Date.now(),
  };
}

/** Parses `@name body` slash-command text; null when not a save command. */
export function parseSlashSave(text: string): { name: string; body: string } | null {
  const match = /^\/save\s+(\S+)\s*([\s\S]+)$/.exec(text.trim());
  if (!match) return null;
  return { name: match[1].slice(0, MAX_PROMPT_NAME_CHARS), body: match[2].trim() };
}

/** Parses `/use name`; names may contain spaces; case-insensitive lookup. */
export function parseSlashUse(text: string, prompts: SavedPrompt[]): SavedPrompt | null {
  const match = /^\/use\s+(.+)$/.exec(text.trim());
  if (!match) return null;
  const wanted = match[1].trim().toLowerCase();
  if (!wanted) return null;
  return prompts.find((prompt) => prompt.name.toLowerCase() === wanted) ?? null;
}

/** Case-insensitive prompt lookup for the palette. */
export function findPromptByName(prompts: SavedPrompt[], name: string): SavedPrompt | null {
  const wanted = name.trim().toLowerCase();
  if (!wanted) return null;
  return prompts.find((prompt) => prompt.name.toLowerCase() === wanted) ?? null;
}

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

/* ------------------------------------------------------------------ */
/* Deadline-bounded model list loading                                 */
/* ------------------------------------------------------------------ */

/** How long the picker waits for the model list before it reports a timeout. */
export const MODEL_LIST_TIMEOUT_MS = 20_000;

/** Exactly one outcome applies to a deadline-bounded request. */
export type DeadlineResult<T> =
  | { kind: 'ok'; value: T }
  | { kind: 'error'; error: unknown }
  | { kind: 'timeout' }
  | { kind: 'cancelled' };

export interface DeadlineRace<T> {
  outcome: Promise<DeadlineResult<T>>;
  cancel: () => void;
}

/**
 * Races one request against a deadline this page owns. An abort alone is not
 * enough: the shared admin client can queue behind a cross-tab session refresh
 * that never observes the abort signal, so the request promise may stay
 * pending long after its controller fired. The deadline guarantees an outcome
 * the UI can render on its own, and `cancel` releases the request when the page
 * unmounts or a newer load supersedes it.
 */
export function raceWithDeadline<T>(
  request: (signal: AbortSignal) => Promise<T>,
  timeoutMs: number,
): DeadlineRace<T> {
  const controller = new AbortController();
  let settle: (result: DeadlineResult<T>) => void = () => {};
  const outcome = new Promise<DeadlineResult<T>>((resolve) => {
    settle = resolve;
  });
  let timer: ReturnType<typeof setTimeout> | null = setTimeout(() => {
    timer = null;
    controller.abort();
    settle({ kind: 'timeout' });
  }, timeoutMs);
  const clear = (): void => {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  };
  void (async () => {
    try {
      const value = await request(controller.signal);
      clear();
      settle({ kind: 'ok', value });
    } catch (error) {
      clear();
      settle({ kind: 'error', error });
    }
  })();
  return {
    outcome,
    cancel: () => {
      clear();
      controller.abort();
      settle({ kind: 'cancelled' });
    },
  };
}
