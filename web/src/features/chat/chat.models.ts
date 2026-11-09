import type { WorkspaceChatModelOption } from '../../lib/types';

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
