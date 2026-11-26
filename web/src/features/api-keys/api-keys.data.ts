//! Pure selectors for the API key governance console. All figures derive from
//! existing backend payloads (`/api-keys`, `/api-keys/{id}/statistics`); no mock data.

import type { GatewayApiKey, GatewayApiKeyStatistics, RequestStatistics } from '../../lib/types';

export type KeyRange = '1d' | '7d' | '30d';
export const KEY_RANGES: ReadonlyArray<{ key: KeyRange; label: string }> = [
  { key: '1d', label: '24H' },
  { key: '7d', label: '7D' },
  { key: '30d', label: '30D' },
];
export const STALE_DAYS = 14;
export const GATEWAY_BASE_URL = 'http://127.0.0.1:8686/v1';
export const KEY_PLACEHOLDER = '<YOUR_EXOROUTE_API_KEY>';

export type KeyStatus = 'active' | 'paused' | 'erroring' | 'stale' | 'never';
export type KeySort = 'traffic' | 'failures' | 'lastUsed' | 'name';
export type KeyScopeFilter = 'all' | 'any' | 'restricted';

export function parseUtc(value: string | null | undefined): Date | null {
  if (!value) return null;
  const parsed = new Date(value.includes('T') ? value : value.replace(' ', 'T') + 'Z');
  return Number.isNaN(parsed.valueOf()) ? null : parsed;
}

/** Days since the key was last used, or null when never used/unparseable. */
export function idleDays(key: GatewayApiKey, now = Date.now()): number | null {
  const date = parseUtc(key.last_used_at);
  if (!date) return null;
  return Math.max(0, Math.floor((now - date.valueOf()) / 86_400_000));
}

export function isStale(key: GatewayApiKey, now = Date.now()): boolean {
  const days = idleDays(key, now);
  return days !== null && days > STALE_DAYS;
}

/** Coarse relative label; callers localize the unit via `tr`. */
export function relativeIdle(key: GatewayApiKey, now = Date.now()): { value: number; unit: 'days ago' | 'hours ago' | 'minutes ago' | 'just now' | 'Never used' } {
  if (!key.last_used_at) return { value: 0, unit: 'Never used' };
  const date = parseUtc(key.last_used_at);
  if (!date) return { value: 0, unit: 'Never used' };
  const minutes = Math.max(0, Math.floor((now - date.valueOf()) / 60_000));
  if (minutes < 1) return { value: 0, unit: 'just now' };
  if (minutes < 60) return { value: minutes, unit: 'minutes ago' };
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return { value: hours, unit: 'hours ago' };
  return { value: Math.floor(hours / 24), unit: 'days ago' };
}

/**
 * Complete relative-time label. Units are bare (no "ago"/"trước" suffix) so
 * `Used {time} ago` supplies the suffix exactly once.
 */
export function formatIdleLabel(
  key: GatewayApiKey,
  tr: (key: string, vars?: Record<string, string | number>) => string,
  now = Date.now(),
): string {
  const idle = relativeIdle(key, now);
  if (idle.unit === 'Never used') return tr('Never used');
  if (idle.unit === 'just now') return tr('just now');
  const unit = idle.unit === 'minutes ago' ? tr('minutes') : idle.unit === 'hours ago' ? tr('hours') : tr('days');
  return tr('Used {time} ago', { time: `${idle.value} ${unit}` });
}
export interface KeyMetrics {
  requests: number;
  tokens: number;
  failures: number;
  successRate: number | null;
  loaded: boolean;
  error: boolean;
}

export const EMPTY_METRICS: KeyMetrics = { requests: 0, tokens: 0, failures: 0, successRate: null, loaded: false, error: false };

/** Fields needed from either a full or per-key statistics payload. */
type MetricsSource = Pick<RequestStatistics, 'total_requests' | 'input_tokens' | 'output_tokens' | 'cached_tokens' | 'failures' | 'success_rate'>;

export function metricsFromStatistics(stats: MetricsSource): KeyMetrics {
  return {
    requests: stats.total_requests,
    tokens: stats.input_tokens + stats.output_tokens + stats.cached_tokens,
    failures: stats.failures,
    successRate: stats.success_rate,
    loaded: true,
    error: false,
  };
}

/**
 * Bounded, cancellable per-key statistics fan-out. A single AbortController
 * governs all workers; aborted requests are ignored rather than recorded as
 * errors so superseded UI state never overwrites fresh data.
 */
export async function loadKeyMetrics(
  keys: GatewayApiKey[],
  range: KeyRange,
  fetchStatistics: (id: string, range: KeyRange, signal: AbortSignal) => Promise<GatewayApiKeyStatistics>,
  signal: AbortSignal,
): Promise<{ metrics: Record<string, KeyMetrics>; partialError: boolean }> {
  const next: Record<string, KeyMetrics> = {};
  const queue = [...keys];
  let partialError = false;
  const worker = async (): Promise<void> => {
    while (queue.length && !signal.aborted) {
      const key = queue.shift()!;
      try {
        next[key.id] = metricsFromStatistics(await fetchStatistics(key.id, range, signal));
      } catch {
        if (signal.aborted) return;
        next[key.id] = { ...EMPTY_METRICS, error: true };
        partialError = true;
      }
    }
  };
  await Promise.all([worker(), worker(), worker(), worker()]);
  return { metrics: next, partialError };
}

export interface KeyFilters {
  query: string;
  status: KeyStatus | 'all';
  scope: KeyScopeFilter;
  sort: KeySort;
}

export function filterAndSortKeys(keys: GatewayApiKey[], filters: KeyFilters, metrics: Record<string, KeyMetrics>, now = Date.now()): GatewayApiKey[] {
  const q = filters.query.trim().toLowerCase();
  const rows = keys.filter((key) => {
    if (q && !`${key.name} ${key.id} ${(key.allowed_models ?? []).join(' ')}`.toLowerCase().includes(q)) return false;
    const m = metrics[key.id];
    if (filters.status === 'active' && !key.enabled) return false;
    if (filters.status === 'paused' && key.enabled) return false;
    if (filters.status === 'erroring' && !(m?.loaded && m.failures > 0)) return false;
    if (filters.status === 'stale' && !isStale(key, now)) return false;
    if (filters.status === 'never' && key.last_used_at) return false;
    if (filters.scope === 'any' && key.scope_restricted) return false;
    if (filters.scope === 'restricted' && !key.scope_restricted) return false;
    return true;
  });
  return rows.sort((a, b) => {
    if (filters.sort === 'name') return a.name.localeCompare(b.name);
    if (filters.sort === 'lastUsed') return (parseUtc(b.last_used_at)?.valueOf() ?? 0) - (parseUtc(a.last_used_at)?.valueOf() ?? 0);
    if (filters.sort === 'failures') return (metrics[b.id]?.failures ?? 0) - (metrics[a.id]?.failures ?? 0);
    return (metrics[b.id]?.requests ?? b.request_count ?? 0) - (metrics[a.id]?.requests ?? a.request_count ?? 0);
  });
}

/** Ranked per-key breakdown, strictly from loaded statistics. */
export function keyBreakdown(keys: GatewayApiKey[], metrics: Record<string, KeyMetrics>, limit = 6) {
  const total = keys.reduce((sum, key) => sum + (metrics[key.id]?.requests ?? 0), 0);
  return keys
    .filter((key) => metrics[key.id]?.loaded)
    .sort((a, b) => (metrics[b.id]?.requests ?? 0) - (metrics[a.id]?.requests ?? 0))
    .slice(0, limit)
    .map((key) => ({
      id: key.id,
      name: key.name,
      requests: metrics[key.id]?.requests ?? 0,
      share: total > 0 ? Math.round(((metrics[key.id]?.requests ?? 0) / total) * 1000) / 10 : 0,
      failures: metrics[key.id]?.failures ?? 0,
      tokens: metrics[key.id]?.tokens ?? 0,
    }));
}

export type SnippetKind = 'curl' | 'openai' | 'anthropic';

/** Quick-connect snippets always use the placeholder, never a stored secret. */
export function connectSnippet(kind: SnippetKind, model: string): string {
  const url = GATEWAY_BASE_URL;
  if (kind === 'curl') return [
    `curl ${url}/chat/completions \\`,
    `  -H "Authorization: Bearer ${KEY_PLACEHOLDER}" \\`,
    '  -H "Content-Type: application/json" \\',
    `  -d '{"model":"${model}","messages":[{"role":"user","content":"Hello"}]}'`,
  ].join('\n');
  if (kind === 'openai') return `from openai import OpenAI\n\nclient = OpenAI(base_url="${url}", api_key="${KEY_PLACEHOLDER}")\nresponse = client.chat.completions.create(\n    model="${model}",\n    messages=[{"role": "user", "content": "Hello"}],\n)`;
  return `import Anthropic from '@anthropic-ai/sdk';\n\nconst client = new Anthropic({ baseURL: '${url}', apiKey: '${KEY_PLACEHOLDER}' });\nconst message = await client.messages.create({\n  model: '${model}',\n  max_tokens: 1024,\n  messages: [{ role: 'user', content: 'Hello' }],\n});`;
}
