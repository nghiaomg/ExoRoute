/**
 * Key Portal API client. The raw API key is sent once in the login request and
 * never persisted: the server sets an HttpOnly session cookie, and every later
 * call relies on that cookie with same-origin credentials.
 */

export type KeyPortalRange = '24h' | '7d' | '30d';

export interface KeyPortalKey {
  id: string;
  name: string;
  prefix: string;
  enabled: boolean;
  allowed_providers: string[] | null;
  allowed_models: string[] | null;
  created_at: string;
  last_used_at: string | null;
}

/** Exact response of POST /login and GET /session (`{ authenticated, key }`). */
export interface KeyPortalSession {
  authenticated: boolean;
  key: KeyPortalKey;
}

export interface KeyPortalSummary {
  total_requests: number;
  success_requests: number;
  failed_requests: number;
  /** Fraction in 0..1. */
  success_rate: number;
  input_tokens: number;
  output_tokens: number;
  cached_tokens: number;
  total_tokens: number;
  total_cost_micro_usd: number;
  avg_latency_ms: number;
}

export interface KeyPortalModelRow {
  model: string;
  provider: string;
  requests: number;
  failed: number;
  total_tokens: number;
  cost_micro_usd: number;
  avg_latency_ms: number;
}

export interface KeyPortalRequestRow {
  id: string;
  timestamp: string;
  model: string;
  provider: string | null;
  status_code: number;
  input_tokens: number;
  output_tokens: number;
  cached_tokens: number;
  latency_ms: number;
  cost_micro_usd: number | null;
}

/** Exact response of GET /stats (`{ key, summary, models, recent_requests }`). */
export interface KeyPortalStats {
  key: KeyPortalKey;
  summary: KeyPortalSummary;
  models: KeyPortalModelRow[];
  recent_requests: KeyPortalRequestRow[];
}

export class KeyPortalError extends Error {
  status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = 'KeyPortalError';
    this.status = status;
  }
}

async function keyPortalRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
  let response: Response;
  try {
    response = await fetch(`/api/v1/key-portal${path}`, {
      ...init,
      credentials: 'same-origin',
      cache: 'no-store',
      headers: { Accept: 'application/json', ...(init.body ? { 'Content-Type': 'application/json' } : {}), ...init.headers },
    });
  } catch {
    throw new KeyPortalError('Could not reach the ExoRoute API. Try again.', 0);
  }
  const payload: unknown = response.status === 204 ? null : await response.json().catch(() => null);
  if (!response.ok) {
    const detail = payload && typeof payload === 'object' && 'detail' in payload
      ? String((payload as { detail: unknown }).detail)
      : 'Key Portal request was rejected.';
    throw new KeyPortalError(detail, response.status);
  }
  return payload as T;
}

export const keyPortalApi = {
  login: (apiKey: string) => keyPortalRequest<KeyPortalSession>('/login', {
    method: 'POST',
    body: JSON.stringify({ api_key: apiKey }),
  }),
  logout: () => keyPortalRequest<{ ok: boolean }>('/logout', { method: 'POST' }),
  session: () => keyPortalRequest<KeyPortalSession>('/session'),
  stats: (range: KeyPortalRange) => keyPortalRequest<KeyPortalStats>(`/stats?range=${encodeURIComponent(range)}`),
};
