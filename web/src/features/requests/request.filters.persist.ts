import type { RequestLogFilters } from '../../lib/types';

/**
 * Persistence for the Requests page filter set.
 *
 * Only the four public filter fields are stored — identifiers and the status
 * enum the table already sends as query parameters; never keys, credentials
 * or session data. Follows the same guarded load/save shape as
 * request.mask.ts (masked columns) so behavior around corrupt storage is
 * identical: fail back to the empty set and never throw.
 */

const STORAGE_KEY = 'exoroute:requests:active_filters';
const MAX_FILTER_CHARS = 256;

export const EMPTY_REQUEST_FILTERS: RequestLogFilters = {
  api_key_id: '',
  model: '',
  provider_id: '',
  status: '',
};

function boundedText(value: unknown): string {
  if (typeof value !== 'string') return '';
  return value.slice(0, MAX_FILTER_CHARS);
}

function isStatus(value: unknown): value is RequestLogFilters['status'] {
  return value === 'success' || value === 'failure' || value === '';
}

export function loadSavedRequestFilters(): RequestLogFilters {
  if (typeof window === 'undefined') return { ...EMPTY_REQUEST_FILTERS };
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...EMPTY_REQUEST_FILTERS };
    const parsed = JSON.parse(raw) as Record<string, unknown> | null;
    if (typeof parsed !== 'object' || parsed === null) return { ...EMPTY_REQUEST_FILTERS };
    return {
      api_key_id: boundedText(parsed.api_key_id),
      model: boundedText(parsed.model),
      provider_id: boundedText(parsed.provider_id),
      status: isStatus(parsed.status) ? parsed.status : '',
    };
  } catch {
    return { ...EMPTY_REQUEST_FILTERS };
  }
}

export function saveSavedRequestFilters(filters: RequestLogFilters): void {
  if (typeof window === 'undefined') return;
  try {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        api_key_id: boundedText(filters.api_key_id),
        model: boundedText(filters.model),
        provider_id: boundedText(filters.provider_id),
        status: isStatus(filters.status) ? filters.status : '',
      }),
    );
  } catch {}
}
