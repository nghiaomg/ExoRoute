/// Display-only column hiding for the routed requests table.
///
/// This is a UI preference only: hidden columns never change what the page
/// fetches, exports, filters, or streams. The state shape mirrors
/// `request.mask.ts`, but the semantics differ (true = hidden here, while
/// true = value-masked there), so the two stay in separate modules with
/// separate storage keys and `request.mask.ts` is untouched.

export type HideableRequestColumn = 'provider' | 'key' | 'combo';

/// True means the column (or, for `combo`, the combo chip) is hidden.
export interface HiddenRequestColumns {
  provider: boolean;
  key: boolean;
  combo: boolean;
}

export interface HideableColumnOption {
  id: HideableRequestColumn;
  labelKey: string;
}

export const HIDEABLE_REQUEST_COLUMNS: readonly HideableColumnOption[] = [
  { id: 'provider', labelKey: 'Provider' },
  { id: 'key', labelKey: 'Key' },
  { id: 'combo', labelKey: 'Combo' },
];

export const DEFAULT_HIDDEN_COLUMNS: HiddenRequestColumns = {
  provider: false,
  key: false,
  combo: false,
};

const STORAGE_KEY = 'exoroute:requests:hidden_columns';

function isHiddenColumnsState(value: unknown): value is HiddenRequestColumns {
  if (typeof value !== 'object' || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.provider === 'boolean' &&
    typeof candidate.key === 'boolean' &&
    typeof candidate.combo === 'boolean'
  );
}

export function loadHiddenColumns(): HiddenRequestColumns {
  if (typeof window === 'undefined') return { ...DEFAULT_HIDDEN_COLUMNS };
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT_HIDDEN_COLUMNS };
    const parsed: unknown = JSON.parse(raw);
    // Stored garbage (wrong shape, wrong types) falls back to the defaults
    // instead of hiding columns the user never asked to hide.
    if (!isHiddenColumnsState(parsed)) return { ...DEFAULT_HIDDEN_COLUMNS };
    return { provider: parsed.provider, key: parsed.key, combo: parsed.combo };
  } catch {
    return { ...DEFAULT_HIDDEN_COLUMNS };
  }
}

export function saveHiddenColumns(state: HiddenRequestColumns): void {
  if (typeof window === 'undefined') return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {}
}

export function countHiddenColumns(state: HiddenRequestColumns): number {
  let count = 0;
  if (state.provider) count += 1;
  if (state.key) count += 1;
  if (state.combo) count += 1;
  return count;
}
