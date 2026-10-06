import type { RequestLog } from './requests';

export interface StatisticsDimension {
  id: string;
  label: string;
  requests: number;
  percentage: number;
}

export interface ModelBreakdown {
  model: string;
  combo: string | null;
  input_tokens: number;
  output_tokens: number;
  cache_hit_rate: number;
  requests: number;
  successes: number;
  success_rate: number;
}

export interface RequestStatistics {
  range: '1d' | '7d' | '30d';
  total_requests: number;
  successes: number;
  failures: number;
  success_rate: number;
  average_latency_ms: number;
  input_tokens: number;
  output_tokens: number;
  cached_tokens: number;
  cache_ratio: number;
  as_of: string;
  collection_started_at: string | null;
  complete: boolean;
  dropped_events: number;
  aggregation_current: boolean;
  api_keys: { top: StatisticsDimension[]; others: number };
  models: { top: StatisticsDimension[]; others: number };
  model_breakdown: ModelBreakdown[];
  model_breakdown_truncated: boolean;
  /** Request-log rows the retained history covers for this range. */
  logged_requests: number;
  /** True when the retained history covers fewer requests than the totals. */
  model_breakdown_incomplete: boolean;
}

export interface GatewayApiKeyStatistics extends Omit<RequestStatistics, 'api_keys'> {
  api_key_id: string;
  api_key_name: string;
}

export interface GatewayApiKey {
  id: string;
  name: string;
  enabled: boolean;
  created_at: string;
  last_used_at?: string | null;
  request_count?: number;
  /** Provider IDs this key may use. Empty allows every provider. */
  allowed_provider_ids?: string[] | null;
  /** Exact model names or trailing-`*` prefixes. Empty allows every model. */
  allowed_models?: string[] | null;
  /** True when the key has at least one provider or model restriction. */
  scope_restricted?: boolean;
  /** False when the stored scope is unreadable and the gateway rejects the key. */
  scope_valid?: boolean;
}

/** Provider and model restrictions submitted together for one API key. */
export interface GatewayApiKeyScopeInput {
  allowed_provider_ids: string[];
  allowed_models: string[];
}

export interface GatewayApiKeyPage {
  api_keys: GatewayApiKey[];
  next_cursor?: string | null;
}

export interface Overview {
  provider_count: number;
  combo_count: number;
  route_count: number;
  request_count: number;
  dropped_request_logs: number;
  uptime_seconds: number;
}

export interface UpstreamLiveSnapshot {
  enabled_provider_count: number;
}
