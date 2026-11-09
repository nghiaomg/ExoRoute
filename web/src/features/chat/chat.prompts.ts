import { nextId } from './chat.ids';

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
