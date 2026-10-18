//! Derived view state for the requests page: outcome classification, the
//! page-scoped KPI numbers, duration/token ratios, and the applied-filter
//! entries the filter chips render.
//!
//! Everything here is pure and side-effect free so the page keeps ownership of
//! fetching, and the numbers stay explainable: the KPIs describe the rows the
//! table currently shows, never a lifetime total.

import { isLiveRequest, type RequestLiveRow, type RequestLog, type RequestLogFilters } from '../../lib/types';
import { requestDurationMs, type Translate } from '../../lib/format';

export type RequestOutcome = 'active' | 'success' | 'failure';

/// A finished row without a recorded status counts as a failure: the gateway
/// records a status for every completed request, so a missing one is not a
/// success.
export function requestOutcome(request: RequestLog | RequestLiveRow): RequestOutcome {
  if (isLiveRequest(request)) return 'active';
  return request.status != null && request.status >= 200 && request.status < 300 ? 'success' : 'failure';
}

export type RequestOutcomeCounts = {
  active: number;
  success: number;
  failure: number;
};

export type RequestPageMetrics = {
  counts: RequestOutcomeCounts;
  total: number;
  /// Share of finished rows that succeeded, or `null` while none finished.
  successRate: number | null;
  /// 95th-percentile duration of the finished rows, or `null` when unmeasured.
  p95DurationMs: number | null;
  /// Slowest duration on the page, used to scale the inline duration bars.
  slowestDurationMs: number | null;
};

export function requestOutcomeCounts(rows: Array<RequestLog | RequestLiveRow>): RequestOutcomeCounts {
  const counts: RequestOutcomeCounts = { active: 0, success: 0, failure: 0 };
  for (const row of rows) counts[requestOutcome(row)] += 1;
  return counts;
}

/// Page-scoped metrics for the rows the table currently shows.
export function requestPageMetrics(
  rows: Array<RequestLog | RequestLiveRow>,
  liveClockMs: number,
): RequestPageMetrics {
  const counts = requestOutcomeCounts(rows);
  const finished = counts.success + counts.failure;
  const durations = rows
    .filter((row) => !isLiveRequest(row))
    .map((row) => requestDurationMs(row, liveClockMs))
    .filter((value): value is number => value != null)
    .sort((left, right) => left - right);
  return {
    counts,
    total: rows.length,
    successRate: finished > 0 ? (counts.success / finished) * 100 : null,
    p95DurationMs: percentile(durations, 0.95),
    slowestDurationMs: durations.length ? (durations[durations.length - 1] ?? null) : null,
  };
}

/// Nearest-rank percentile over a sorted sample; `null` for an empty sample.
function percentile(sorted: number[], fraction: number): number | null {
  if (!sorted.length) return null;
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil(fraction * sorted.length) - 1));
  return sorted[index] ?? null;
}

/// The project's duration tiers. A chat completion is legitimately slow, so the
/// tiers sit well above network latency instead of next to it.
export const SLOW_DURATION_MS = 2000;
export const VERY_SLOW_DURATION_MS = 8000;

export type DurationTone = 'fast' | 'slow' | 'verySlow';

export function durationTone(ms: number | null): DurationTone {
  if (ms != null && ms >= VERY_SLOW_DURATION_MS) return 'verySlow';
  if (ms != null && ms >= SLOW_DURATION_MS) return 'slow';
  return 'fast';
}

/// Bar width for one duration relative to the slowest row on the page, floored
/// so an instant row is still visible next to a slow one.
export function durationBarPercent(ms: number | null, slowestMs: number | null): number {
  if (ms == null || slowestMs == null || slowestMs <= 0) return 0;
  return Math.max(6, Math.min(100, Math.round((ms / slowestMs) * 100)));
}

export type TokenRatio = {
  input: number;
  output: number;
};

/// Share of a row's tokens that went in and out, or `null` when the row
/// recorded no usage at all.
export function tokenRatio(input?: number | null, output?: number | null): TokenRatio | null {
  const inputTokens = input != null && Number.isFinite(input) && input > 0 ? input : 0;
  const outputTokens = output != null && Number.isFinite(output) && output > 0 ? output : 0;
  const total = inputTokens + outputTokens;
  if (total <= 0) return null;
  const inputShare = Math.round((inputTokens / total) * 100);
  return { input: inputShare, output: 100 - inputShare };
}

/// Stable identity for one table row, live or finished.
export function requestRowKey(request: RequestLog | RequestLiveRow): string {
  if (isLiveRequest(request)) return request.live_id;
  return request.id ?? request.request_id ?? request.created_at;
}

/// Two-letter monogram for a model id, taken from the model segment so
/// `ocg/minimax-m3` reads as `MI` next to the full id in the cell tooltip.
export function modelMonogram(model: string): string {
  const segment = model.split('/').pop() ?? model;
  const letters = segment.replace(/[^\p{L}\p{N}]/gu, '');
  if (!letters) return '—';
  return letters.slice(0, 2).toUpperCase();
}

/// Stable accent bucket for a provider/model id, so one provider keeps the same
/// colour across rows and reloads.
export function modelAccent(value: string, buckets = 6): number {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 31 + value.charCodeAt(index)) % 1_000_003;
  }
  return ((hash % buckets) + buckets) % buckets;
}

export type RequestFilterKey = 'api_key_id' | 'model' | 'provider_id' | 'status';

export type AppliedRequestFilter = {
  key: RequestFilterKey;
  label: string;
  value: string;
  /// The exact id behind a human-readable `value`, shown as the chip tooltip.
  title?: string;
};

/// Display names for the two id filters, as loaded by the option controller.
export type RequestFilterLabels = {
  api_key_id?: string;
  provider_id?: string;
};

/// The filters currently applied, in the order the filter form shows them.
export function appliedRequestFilters(
  filters: RequestLogFilters,
  tr: Translate,
  labels: RequestFilterLabels = {},
): AppliedRequestFilter[] {
  const entries: AppliedRequestFilter[] = [];
  const apiKey = filters.api_key_id?.trim();
  if (apiKey) {
    // `unknown` is the gateway's marker for a request that carried no key; the
    // raw id would be meaningless, so the chip shows the localised placeholder.
    const unknown = apiKey.toLowerCase() === 'unknown';
    const name = unknown ? '' : labels.api_key_id?.trim() ?? '';
    entries.push({
      key: 'api_key_id',
      label: tr('API key ID'),
      value: unknown ? tr('Unknown key') : name || apiKey,
      ...(name ? { title: apiKey } : {}),
    });
  }
  const provider = filters.provider_id?.trim();
  if (provider) {
    const name = labels.provider_id?.trim() || provider;
    entries.push({
      key: 'provider_id',
      label: tr('Provider ID'),
      value: name,
      ...(name === provider ? {} : { title: provider }),
    });
  }
  const model = filters.model?.trim();
  if (model) entries.push({ key: 'model', label: tr('Requested model'), value: model });
  if (filters.status === 'success') {
    entries.push({ key: 'status', label: tr('Outcome'), value: tr('Successful') });
  } else if (filters.status === 'failure') {
    entries.push({ key: 'status', label: tr('Outcome'), value: tr('Failed') });
  }
  return entries;
}

/// The filters with one entry cleared, ready to query again.
export function clearedRequestFilter(
  filters: RequestLogFilters,
  key: RequestFilterKey,
): RequestLogFilters {
  return { ...filters, [key]: '' };
}

/// Whether two filter sets query the same page, so the form only offers Apply
/// and Clear while the draft differs from what is applied.
export function sameRequestFilters(left: RequestLogFilters, right: RequestLogFilters): boolean {
  return (
    (left.api_key_id ?? '').trim() === (right.api_key_id ?? '').trim() &&
    (left.model ?? '').trim() === (right.model ?? '').trim() &&
    (left.provider_id ?? '').trim() === (right.provider_id ?? '').trim() &&
    (left.status ?? '') === (right.status ?? '')
  );
}
