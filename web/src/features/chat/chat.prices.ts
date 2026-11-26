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
