export type MaskableRequestColumn = 'key' | 'model' | 'alias' | 'provider';

export interface MaskedColumnsState {
  key: boolean;
  model: boolean;
  alias: boolean;
  provider: boolean;
}

export interface MaskableColumnOption {
  id: MaskableRequestColumn;
  labelKey: string;
}

export const MASKABLE_COLUMNS: readonly MaskableColumnOption[] = [
  { id: 'key', labelKey: 'API key' },
  { id: 'model', labelKey: 'Model' },
  { id: 'alias', labelKey: 'Alias / Combo' },
  { id: 'provider', labelKey: 'Provider' },
];

export const DEFAULT_MASKED_COLUMNS: MaskedColumnsState = {
  alias: false,
  key: false,
  model: false,
  provider: false,
};

const STORAGE_KEY = 'exoroute:requests:masked_columns';

export function loadMaskedColumns(): MaskedColumnsState {
  if (typeof window === 'undefined') return { ...DEFAULT_MASKED_COLUMNS };
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT_MASKED_COLUMNS };
    const parsed = JSON.parse(raw);
    return {
      alias: Boolean(parsed.alias),
      key: Boolean(parsed.key),
      model: Boolean(parsed.model),
      provider: Boolean(parsed.provider),
    };
  } catch {
    return { ...DEFAULT_MASKED_COLUMNS };
  }
}

export function saveMaskedColumns(state: MaskedColumnsState): void {
  if (typeof window === 'undefined') return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {}
}

/**
 * Masks a text value with asterisks up to a maximum of 3 characters ('***').
 * For example:
 * - "a" -> "*"
 * - "ab" -> "**"
 * - "abc" -> "***"
 * - "my-custom-combo" -> "***"
 * - null/undefined/empty -> "—"
 */
export function maskTextValue(value: string | null | undefined): string {
  if (!value) return '—';
  const trimmed = value.trim();
  if (!trimmed || trimmed === '—') return '—';
  const count = Math.min(Math.max(trimmed.length, 1), 3);
  return '*'.repeat(count);
}

export function countMaskedColumns(state: MaskedColumnsState): number {
  let count = 0;
  if (state.alias) count += 1;
  if (state.model) count += 1;
  if (state.provider) count += 1;
  if (state.key) count += 1;
  return count;
}
