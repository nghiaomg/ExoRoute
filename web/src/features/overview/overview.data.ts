//! Pure selectors for the Overview control center. All figures derive from
//! existing dashboard API payloads; no synthetic or hardcoded traffic data.

import type { GatewayCombo, Provider, ProviderQuotaResult, RequestLog, RequestStatistics } from '../../lib/types';

/** Time ranges offered by the overview filter, in display order. */
export type RangeKey = '1H' | '24H' | '7D' | '30D' | '90D' | 'CUSTOM';

/**
 * Only 1d/7d/30d have real rollup data: `range_window` in
 * `src/admin/statistics.rs` accepts those three values and telemetry retains
 * minute history for 31 days. The other options stay visible but disabled so
 * the dashboard never shows fabricated numbers for an unsupported window.
 */
export const RANGES: ReadonlyArray<{
  key: RangeKey;
  apiRange: '1d' | '7d' | '30d' | null;
}> = [
  { key: '1H', apiRange: null },
  { key: '24H', apiRange: '1d' },
  { key: '7D', apiRange: '7d' },
  { key: '30D', apiRange: '30d' },
  { key: '90D', apiRange: null },
  { key: 'CUSTOM', apiRange: null },
];
export const LATENCY_TARGET_MS = 15000;

export type ProviderHealth = {
  total: number;
  active: number;
  degraded: number;
  disabled: number;
  invalidKeys: number;
};

/**
 * Provider health uses configuration flags and quota snapshots, never mocks.
 * Quotas are keyed by provider id: the caller only fetches a bounded subset of
 * enabled providers, so positional lookup would attribute another provider's
 * quota to this one whenever the list is filtered or truncated.
 */
export function providerHealth(providers: Provider[], quotas: Map<string, ProviderQuotaResult>): ProviderHealth {
  const health: ProviderHealth = { total: providers.length, active: 0, degraded: 0, disabled: 0, invalidKeys: 0 };
  providers.forEach((provider) => {
    if (!provider.enabled) {
      health.disabled += 1;
      return;
    }
    const invalid = provider.invalid_api_key_count ?? 0;
    const quota = quotas.get(provider.id);
    const limited = quota?.status === 'capacity_limited' || (quota?.remaining_percent ?? 100) <= 0;
    if (invalid > 0) health.invalidKeys += invalid;
    if (invalid > 0 || limited) health.degraded += 1;
    else health.active += 1;
  });
  return health;
}

export type TrafficRow = {
  id: string;
  label: string;
  requests: number;
  share: number;
  successRate: number;
  cacheHitRate: number | null;
};

function shareOf(requests: number, total: number): number {
  return total > 0 ? Math.round((requests / total) * 1000) / 10 : 0;
}

/** Ranked combo traffic: alias groups from statistics, matched to configured combos. */
export function comboTraffic(stats: RequestStatistics, combos: GatewayCombo[]): TrafficRow[] {
  const total = stats.route_breakdown.reduce((sum, row) => sum + row.requests, 0);
  const byId = new Map(combos.map((combo) => [combo.id, combo.name]));
  return [...stats.route_breakdown]
    .sort((a, b) => b.requests - a.requests)
    .slice(0, 5)
    .map((row, index) => ({
      // Rank index keeps the key unique when the backend reports repeated
      // aliases (same model served by different providers).
      id: `${row.route_alias}:${index}`,
      label: byId.get(row.route_alias) ?? row.route_alias,
      requests: row.requests,
      share: shareOf(row.requests, total),
      successRate: Math.round(row.success_rate),
      cacheHitRate: null,
    }));
}

/** Ranked model traffic uses the backend model breakdown when available. */
export function modelTraffic(stats: RequestStatistics): TrafficRow[] {
  const total = stats.model_breakdown.reduce((sum, row) => sum + row.requests, 0);
  return [...stats.model_breakdown]
    .sort((a, b) => b.requests - a.requests)
    .slice(0, 5)
    .map((row, index) => ({
      // The backend splits one model into a row per serving provider, so the
      // model name alone is not a unique Svelte key; qualify it with provider
      // and rank to avoid each_key_duplicate crashes on the overview page.
      id: `${row.model}:${row.provider ?? ''}:${index}`,
      label: row.model,
      requests: row.requests,
      share: shareOf(row.requests, total),
      successRate: Math.round(row.success_rate),
      cacheHitRate: Math.round(row.cache_hit_rate),
    }));
}

export type StatusGroups = { ok: number; client: number; server: number; other: number };

export function statusGroups(stats: RequestStatistics | null): StatusGroups {
  const groups: StatusGroups = { ok: 0, client: 0, server: 0, other: 0 };
  if (!stats) return groups;
  for (const row of stats.statuses.rows) {
    const status = row.status || 0;
    if (status >= 200 && status < 300) groups.ok += row.requests;
    else if (status >= 400 && status < 500) groups.client += row.requests;
    else if (status >= 500 && status < 600) groups.server += row.requests;
    else groups.other += row.requests;
  }
  return groups;
}

export type TrendPoint = { label: string; requests: number; tokens: number; latencyMs: number };

/** Buckets real backend series points into at most 12 columns for the chart. */
export function trendPoints(stats: RequestStatistics, metric: 'requests' | 'tokens' | 'latency'): TrendPoint[] {
  const points = stats.series.points;
  if (!points.length) return [];
  const step = Math.max(1, Math.ceil(points.length / 12));
  const result: TrendPoint[] = [];
  for (let i = 0; i < points.length; i += step) {
    const slice = points.slice(i, i + step);
    const tokens = slice.reduce((sum, p) => sum + p.input_tokens + p.output_tokens, 0);
    result.push({
      label: slice[0]?.start.slice(5, 16) ?? '',
      requests: slice.reduce((sum, p) => sum + p.requests, 0),
      tokens,
      latencyMs: Math.round(slice.reduce((sum, p) => sum + p.average_latency_ms, 0) / slice.length),
    });
  }
  return result;
}

export type RequestFilter = 'all' | 'success' | 'errors';

export function filterRequests(rows: RequestLog[], filter: RequestFilter, model: string): RequestLog[] {
  const query = model.trim().toLowerCase();
  return rows.filter((row) => {
    const status = row.status ?? 0;
    if (filter === 'success' && !(status >= 200 && status < 300)) return false;
    if (filter === 'errors' && (status >= 200 && status < 300)) return false;
    if (query && !`${row.model} ${row.route_alias ?? ''} ${row.provider_id ?? ''}`.toLowerCase().includes(query)) return false;
    return true;
  });
}
