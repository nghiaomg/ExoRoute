import type { RequestLog } from '../../lib/types';
import { maskTextValue, type MaskedColumnsState } from './request.mask';

/**
 * CSV serialization for the Requests page export.
 *
 * A pure function (no DOM, no fetch) so the quoting, escaping and masking are
 * exactly unit-testable. The export never includes provider credentials:
 * `provider_credential_id` is deliberately absent, and columns the user has
 * masked in the table menu stay masked here too.
 */

export interface CsvExportOptions {
  maskedColumns?: MaskedColumnsState;
}

export const MAX_EXPORT_ROWS = 1000;

const HEADER = [
  'created_at',
  'request_id',
  'api_key_id',
  'api_key_name',
  'model',
  'route_alias',
  'provider_id',
  'client_protocol',
  'upstream_protocol',
  'status',
  'duration_ms',
  'input_tokens',
  'output_tokens',
  'cached_tokens',
  'cache_input_tokens',
  'cost_micro_usd',
  'error',
] as const;

/** RFC 4180 field escaping: quote when needed, double inner quotes. */
export function escapeCsvField(value: string): string {
  if (!/[";\n\r,]/.test(value)) return value;
  return `"${value.replaceAll('"', '""')}"`;
}

function cell(row: RequestLog, field: (typeof HEADER)[number]): string {
  const value = row[field];
  if (value === null || value === undefined) return '';
  return String(value);
}

export function requestsToCsv(rows: readonly RequestLog[], options: CsvExportOptions = {}): string {
  const masked = options.maskedColumns;
  const lines: string[] = [HEADER.join(',')];
  for (const row of rows) {
    const cells: string[] = HEADER.map((field) => {
      let value = cell(row, field);
      // The masked columns mirror the table: the exported file must not
      // widen what the user already chose to hide on screen.
      if (masked?.key && field === 'api_key_name') value = value ? maskTextValue(value) : '';
      if (masked?.model && field === 'model') value = value ? maskTextValue(value) : '';
      if (masked?.alias && field === 'route_alias') value = value ? maskTextValue(value) : '';
      if (masked?.provider && field === 'provider_id') value = value ? maskTextValue(value) : '';
      return escapeCsvField(value);
    });
    lines.push(cells.join(','));
  }
  return lines.join('\n');
}

/** Desktop filenames stay sortable: ISO stamp without colons/dots. */
export function requestsCsvFilename(now: Date = new Date()): string {
  return `exoroute-requests-${now.toISOString().replace(/[:.]/g, '-').slice(0, 19)}.csv`;
}
