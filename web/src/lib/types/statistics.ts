import type { RequestLog } from './requests';

export interface StatisticsDimension {
  id: string;
  label: string;
  requests: number;
  percentage: number;
}

export interface ModelBreakdown {
  model: string;
  /** Provider ID that served the requests, or null when none was selected. */
  provider: string | null;
  input_tokens: number;
  output_tokens: number;
  cache_hit_rate: number;
  requests: number;
  successes: number;
  success_rate: number;
}

/** Cost fields shared by every history view that reports spend. */
export interface CostFields {
  /** Total, or null when some requests in the group reported no cost. */
  cost_micro_usd: number | null;
  /** Sum of the costs that were reported, even when the total is partial. */
  cost_reported_micro_usd: number;
  cost_events: number;
  missing_cost_events: number;
}

export interface ProviderBreakdown extends CostFields {
  /** Provider ID that served the requests, or null when none was selected. */
  provider: string | null;
  requests: number;
  successes: number;
  success_rate: number;
  average_latency_ms: number;
  input_tokens: number;
  output_tokens: number;
}

export interface StatusBreakdown {
  status: number;
  requests: number;
  successes: number;
  average_latency_ms: number;
}

export interface SeriesPoint extends CostFields {
  /** Bucket start, as the `YYYY-MM-DD HH:MM:SS` UTC shape history is keyed by. */
  start: string;
  requests: number;
  successes: number;
  failures: number;
  input_tokens: number;
  output_tokens: number;
  average_latency_ms: number;
}

/**
 * Percentile estimates from a fixed request-log histogram. A value is the
 * bucket's upper bound, so it can only overstate; `open_ended` is true when a
 * sample sits in the last, open bucket, which makes the reported value a floor.
 */
export interface LatencySummary {
  average_ms: number;
  p50_ms: number | null;
  /** True when that percentile's sample ran past the last bucket, so it is a floor. */
  p50_is_floor: boolean;
  p95_ms: number | null;
  p95_is_floor: boolean;
  p99_ms: number | null;
  p99_is_floor: boolean;
  open_ended: boolean;
  bucket_bounds_ms: number[];
  buckets: number[];
  samples: number;
}

export interface CostSummary extends CostFields {
  requests: number;
}

export interface SeriesView {
  points: SeriesPoint[];
  bucket_seconds: number;
  truncated: boolean;
}

/** A ranked list that a bounded scan may have cut short. */
export interface RowsView<T> {
  rows: T[];
  truncated: boolean;
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
  providers: RowsView<ProviderBreakdown>;
  statuses: RowsView<StatusBreakdown>;
  series: SeriesView;
  latency: LatencySummary;
  cost: CostSummary;
  /** True when the history scan stopped at its row limit for this range. */
  history_truncated: boolean;
  /** Mean time to the first streamed token, or null when nothing streamed. */
  average_time_to_first_token_ms: number | null;
  time_to_first_token_samples: number;
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
