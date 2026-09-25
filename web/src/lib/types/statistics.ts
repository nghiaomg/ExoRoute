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
