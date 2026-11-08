import type { CredentialState } from './quota.state';

/**
 * Local persistence layer for the quota page.
 *
 * Credential states (keys + usage accounts) are cached in localStorage so a
 * revisiting user sees the last known quota immediately while fresh data
 * loads in the background. Entries older than CACHE_TTL_MS are ignored, and
 * transient flags (loading/refreshing/generation) are never persisted.
 */

const CACHE_KEY = 'exoroute.quota.cache.v1';
const AUTO_REFRESH_KEY = 'exoroute.quota.autoRefresh';
const CACHE_TTL_MS = 24 * 60 * 60 * 1000;
const MAX_CACHED_PROVIDERS = 100;
const SAVE_DEBOUNCE_MS = 400;

type CachedEntry = {
  keys: CredentialState['keys'];
  usageByKey: CredentialState['usageByKey'];
  nextCursor: string | null;
  pageCursors: Array<string | null>;
};

type CacheEnvelope = {
  savedAt: number;
  providers: Record<string, CachedEntry>;
};

function storage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

function isCredentialStateLike(value: unknown): value is CachedEntry {
  if (typeof value !== 'object' || value === null) return false;
  const entry = value as Partial<CachedEntry>;
  return Array.isArray(entry.keys)
    && typeof entry.usageByKey === 'object'
    && entry.usageByKey !== null
    && Array.isArray(entry.pageCursors);
}

/** Returns the last cached credential states, or an empty map when absent,
 * malformed, or older than the TTL. */
export function loadQuotaCache(): Record<string, CredentialState> {
  const store = storage();
  if (!store) return {};
  try {
    const raw = store.getItem(CACHE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Partial<CacheEnvelope>;
    if (typeof parsed.savedAt !== 'number' || Date.now() - parsed.savedAt > CACHE_TTL_MS) return {};
    if (typeof parsed.providers !== 'object' || parsed.providers === null) return {};
    const states: Record<string, CredentialState> = {};
    let count = 0;
    for (const [providerId, entry] of Object.entries(parsed.providers)) {
      if (count >= MAX_CACHED_PROVIDERS) break;
      if (!isCredentialStateLike(entry)) continue;
      states[providerId] = {
        keys: entry.keys,
        usageByKey: entry.usageByKey,
        nextCursor: entry.nextCursor ?? null,
        pageCursors: entry.pageCursors,
        loading: false,
        refreshing: false,
        error: '',
        generation: 0,
      };
      count += 1;
    }
    return states;
  } catch {
    return {};
  }
}

let saveTimer: ReturnType<typeof setTimeout> | undefined;

/** Debounced writer: collapses bursts of state updates into one storage write. */
export function scheduleQuotaCacheSave(states: Record<string, CredentialState>): void {
  const store = storage();
  if (!store) return;
  if (saveTimer !== undefined) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    saveTimer = undefined;
    try {
      const providers: Record<string, CachedEntry> = {};
      let count = 0;
      for (const [providerId, state] of Object.entries(states)) {
        if (count >= MAX_CACHED_PROVIDERS) break;
        if (!state.keys.length && !Object.keys(state.usageByKey).length) continue;
        providers[providerId] = {
          keys: state.keys,
          usageByKey: state.usageByKey,
          nextCursor: state.nextCursor,
          pageCursors: state.pageCursors,
        };
        count += 1;
      }
      const envelope: CacheEnvelope = { savedAt: Date.now(), providers };
      store.setItem(CACHE_KEY, JSON.stringify(envelope));
    } catch {
      // Quota cache is best-effort; storage failures must not break the page.
    }
  }, SAVE_DEBOUNCE_MS);
}

/** Whether the 60-second auto-refresh is enabled. Defaults to on. */
export function getQuotaAutoRefresh(): boolean {
  const store = storage();
  if (!store) return true;
  try {
    return store.getItem(AUTO_REFRESH_KEY) !== 'false';
  } catch {
    return true;
  }
}

export function setQuotaAutoRefresh(enabled: boolean): void {
  const store = storage();
  if (!store) return;
  try {
    store.setItem(AUTO_REFRESH_KEY, enabled ? 'true' : 'false');
  } catch {
    // Preference persistence is best-effort.
  }
}
