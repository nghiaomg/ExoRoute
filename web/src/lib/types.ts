export type Protocol = 'chat_completions' | 'responses' | 'messages';
export type UpstreamProtocol = Protocol | 'google_generate_content';
export type ProviderCategory = 'custom' | 'cloud_api' | 'gateway' | 'oauth';
export type ProviderLabel = 'free' | 'free_tier';
export type ProviderThinkingMode = 'preserve' | 'override' | 'remove';
export type ProviderKeyStrategy = 'priority' | 'round_robin';

export interface ProviderAdapterCapabilities {
  api_keys: boolean;
  oauth_accounts: boolean;
  local_usage_meter?: boolean;
  local_quota_tracking?: boolean;
  model_discovery: boolean;
  usage_limits: boolean;
  api_key_usage: boolean;
  api_key_usage_status?: 'supported' | 'unverified' | 'unsupported';
  api_key_auth_assist: boolean;
  model_catalog_authoritative: boolean;
  event_stream_response: boolean;
  auth_panel: string | null;
  model_protocol_routing?: boolean;
  supported_upstream_protocols?: UpstreamProtocol[];
}

export interface ProviderPreset {
  id: string;
  adapter_id: string;
  name: string;
  description: string;
  category: ProviderCategory;
  labels: ProviderLabel[];
  default_base_url?: string | null;
  default_logo_url?: string | null;
  default_model_prefix?: string | null;
  default_auth_type: string;
  supported_auth_types: string[];
  default_preferred_protocol: UpstreamProtocol | string;
  default_supported_protocols: Array<UpstreamProtocol | string>;
  protocol_selectable: boolean;
  capabilities: ProviderAdapterCapabilities;
}

export interface Provider {
  id: string;
  adapter_id: string;
  capabilities?: ProviderAdapterCapabilities | null;
  name: string;
  base_url: string;
  logo_url?: string | null;
  model_prefix?: string;
  enabled: boolean;
  auth_type: string;
  auth_header?: string | null;
  custom_headers?: string[];
  api_key?: string | null;
  api_key_count?: number;
  invalid_api_key_count?: number;
  model_count?: number;
  local_rpm_target?: number;
  thinking_mode?: ProviderThinkingMode;
  thinking_override?: string | null;
  key_strategy?: ProviderKeyStrategy;
  preferred_protocol: UpstreamProtocol | string;
  supported_protocols: UpstreamProtocol[];
}

export interface ProviderCustomHeaderInput {
  name: string;
  value?: string | null;
}

export interface ProviderKey {
  id: string;
  name: string;
  credential_type?: 'api_key' | 'oauth';
  enabled: boolean;
  invalid: boolean;
  usage_budget_5h_micros?: number | null;
  usage_budget_7d_micros?: number | null;
  usage_budget_30d_micros?: number | null;
  last_error?: string | null;
  last_test_passed?: boolean | null;
  last_test_status?: number | null;
  created_at: string;
  last_used_at?: string | null;
}

export interface ProviderUsageQuota {
  id: string;
  label: string;
  /** Deprecated consumed percentage. Use remaining_percent for quota UI. */
  used_percent: number;
  /** 0 means quota exhausted; 100 means the full quota remains. */
  remaining_percent: number;
  used_amount?: number | null;
  limit_amount?: number | null;
  unit?: string | null;
  /** Unix timestamp in seconds. */
  reset_at?: number | null;
  window_seconds?: number | null;
  uncapped?: boolean;
  reset_period?: 'daily' | 'weekly' | 'monthly' | null;
}

export interface ProviderUsageSnapshot {
  plan?: string | null;
  limit_reached: boolean;
  reset_credits_available?: number | null;
  quotas: ProviderUsageQuota[];
  credit_balance?: {
    unit: string;
    monthly_remaining?: number | null;
    purchased_remaining?: number | null;
    free_remaining?: number | null;
    period_used?: number | null;
    period_ends_at?: number | null;
  } | null;
}

export interface ProviderUsageAccount {
  key_id: string;
  name: string;
  status: 'fresh' | 'stale' | 'partial' | 'unknown' | 'unavailable' | 'reauth_required' | 'disabled';
  snapshot?: ProviderUsageSnapshot | null;
  provider_quota?: {
    status: 'available' | 'unsupported' | 'unknown';
    source?: string;
    message?: string;
    snapshot?: ProviderUsageSnapshot | null;
  } | null;
  local_meter?: {
    source: 'exoroute_local_meter';
    status: 'fresh' | 'stale' | 'partial' | 'unknown';
    updated_at_ms?: number;
    dropped_events?: number;
    budget_usd?: { '5h'?: number | null; '7d'?: number | null; '30d'?: number | null };
    windows: Record<string, {
      cost_micro_usd?: number | null;
      requests: number;
      input_tokens: number;
      output_tokens: number;
      cost_events: number;
      missing_cost_events: number;
    }>;
  } | null;
  /** Unix timestamp in milliseconds. */
  fetched_at_ms?: number | null;
  message?: string | null;
}

export interface ProviderUsageResult {
  accounts: ProviderUsageAccount[];
  next_cursor?: string | null;
}

export interface ProviderKeyUsageResult {
  account: ProviderUsageAccount;
}

export interface ProviderKeyPage {
  keys: ProviderKey[];
  next_cursor?: string | null;
  total?: number | null;
}

export interface ProviderKeyTest {
  index: number;
  test_passed: boolean;
  status?: number | null;
  message?: string | null;
  warning?: string | null;
}

export interface ProviderCreateResult {
  ok: boolean;
  id: string;
  key_tests: ProviderKeyTest[];
}

export interface ProviderUpdateResult {
  ok: boolean;
  key_tests: ProviderKeyTest[];
}

export interface ProviderThinkingSettingsInput {
  mode: ProviderThinkingMode;
  override_text?: string | null;
}

export interface ProviderThinkingSettingsResult {
  ok: boolean;
}

export interface ProviderKeyStrategyInput {
  strategy: ProviderKeyStrategy;
}

export interface ProviderKeyStrategyResult {
  ok: boolean;
  strategy: ProviderKeyStrategy;
}

export interface ProviderAuthStartResult {
  flow_id: string;
  authorization_url: string;
}

export interface ProviderAuthStatus {
  status: 'pending' | 'connected' | 'failed' | 'expired';
  message?: string;
}

export interface ProviderAuthCompletionResult {
  flow_id: string;
  provider_id: string;
  status: ProviderAuthStatus['status'];
}

export interface ProviderApiKeyAuthStartResult {
  flow_id: string;
  auth_url: string;
  callback_url: string;
  expires_at_ms: number;
}

export interface ProviderApiKeyAuthStatus {
  status: 'pending' | 'received' | 'applying' | 'applied' | 'failed' | 'expired';
  message?: string | null;
  user_id?: string | null;
  user_name?: string | null;
  key_name?: string | null;
  provider_key_id?: string | null;
  expires_at_ms?: number | null;
}

export type CodexOAuthStartResult = ProviderAuthStartResult;
export type CodexOAuthStatus = ProviderAuthStatus;

export interface ProviderKeyCreateResult {
  ok: boolean;
  id: string;
  test_passed: boolean;
  status?: number | null;
  warning?: string | null;
}

export interface ComboTarget {
  provider_id: string;
  model: string;
  protocol?: UpstreamProtocol | null;
  priority: number;
  enabled: boolean;
  weight?: number | null;
}

/** @deprecated Use ComboTarget for new dashboard code. */
export type RouteTarget = ComboTarget;

export interface GatewayCombo {
  id: string;
  name: string;
  strategy: string;
  accepted_protocols: Protocol[];
  targets: ComboTarget[];
}

/** @deprecated Use GatewayCombo for new dashboard code. */
export type GatewayRoute = GatewayCombo;

export interface GatewayModel {
  id?: string;
  model: string;
  provider_id: string;
  provider_name?: string;
  enabled?: boolean;
}

export interface ModelTestTarget {
  provider_id: string;
  model: string;
}

export interface ModelTestResult extends ModelTestTarget {
  test_passed: boolean;
  status?: number | null;
  latency_ms: number;
  message?: string | null;
  provider_response_body?: string | null;
}

export interface ModelTestResponse {
  results: ModelTestResult[];
}

export interface ProviderModelCatalog {
  models: string[];
  has_more?: boolean;
}

export interface ProviderModelRoutingEntry {
  model: string;
  effective_upstream_protocol: UpstreamProtocol | null;
  override_protocol: UpstreamProtocol | null;
  routing_configured: boolean;
}

export interface ProviderModelRoutingPage {
  models: ProviderModelRoutingEntry[];
  next_cursor?: string | null;
  supported_protocols?: UpstreamProtocol[];
}

export interface ComboProviderOption {
  id: string;
  name: string;
  enabled: boolean;
  adapter_id?: string;
  supported_upstream_protocols?: UpstreamProtocol[];
}

export interface ComboProviderOptionPage {
  providers: ComboProviderOption[];
  next_cursor?: string | null;
}

export interface ProviderModelImportResult {
  models: string[];
  available: boolean;
  truncated: boolean;
  pruned?: string[];
}

export interface ProviderQuotaResult {
  provider_id: string;
  window_seconds: number;
  requests_last_60_seconds: number;
  target_rpm: number;
  /** 0 means the local target is exhausted; 100 means it is full. */
  remaining_percent: number;
  /** Deprecated consumed percentage. Use remaining_percent for quota UI. */
  used_percent: number;
  coverage_seconds: number;
  sampled_at_ms: number;
  status: 'ready' | 'warming_up' | 'capacity_limited';
  source: 'exoroute_local' | string;
  nvidia_source: 'not_documented' | string;
}

export interface RequestLog {
  id?: string;
  request_id?: string;
  route_alias?: string;
  provider_id?: string;
  provider_credential_id?: string | null;
  api_key_id?: string | null;
  api_key_name?: string | null;
  model: string;
  client_protocol?: string;
  upstream_protocol?: string;
  status?: number;
  duration_ms?: number;
  input_tokens?: number;
  output_tokens?: number;
  cached_tokens?: number;
  cache_input_tokens?: number;
  cost_micro_usd?: number | null;
  error?: string | null;
  created_at: string;
}

export interface RequestLiveRow extends RequestLog {
  live: true;
  live_id: string;
  started_at_ms: number;
}

export type RequestLiveConnectionState = 'connecting' | 'connected' | 'reconnecting' | 'unavailable';

export type RequestLiveEvent =
  | {
      type: 'snapshot';
      requests: RequestLiveRow[];
      truncated: boolean;
      limit: number;
      active_count: number;
    }
  | {
      type: 'started';
      request: RequestLiveRow;
      active_count?: number;
    }
  | {
      type: 'updated';
      live_id: string;
      provider_id: string | null;
      latest_log_id: string | null;
      input_tokens?: number | null;
      output_tokens?: number | null;
    }
  | {
      type: 'finished';
      live_id: string;
      request: RequestLog | null;
      finished_at_ms: number;
      active_count?: number;
    }
  | {
      type: 'capacity';
      truncated: boolean;
      limit: number;
      active_count?: number;
    };

export interface RequestLogPage {
  requests: RequestLog[];
  next_cursor?: string | null;
  page_size: number;
}

export interface RequestLogFilters {
  cursor?: string | null;
  api_key_id?: string;
  model?: string;
  provider_id?: string;
  status?: 'success' | 'failure' | '';
}

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

export interface GatewayResourceLimitValues {
  gateway_body_limit_mib: number;
  /** Zero means unlimited; finite values are limited to 1–8. */
  gateway_body_processing_concurrency: number;
  /** Zero means unlimited; finite values must be at least 64 KiB. */
  sse_frame_limit_kib: number;
  /** Zero means unlimited; finite values must be at least the finite frame size. */
  sse_buffer_limit_kib: number;
  /** Zero means unlimited; finite values are limited to 1–32. */
  provider_max_concurrency: number;
  stream_continuity_enabled: boolean;
  /** Always finite to bound detached work; valid values are 1–64. */
  stream_continuity_max_concurrency: number;
}

export interface GatewayResourceLimits {
  active: GatewayResourceLimitValues;
  saved: GatewayResourceLimitValues;
  restart_required: boolean;
}

export interface OperationalSettingsValues {
  connect_timeout_ms: number;
  request_timeout_ms: number;
  stream_idle_timeout_ms: number;
  circuit_breaker_enabled: boolean;
  circuit_breaker_threshold: number;
  circuit_breaker_cooldown_seconds: number;
  /** Zero disables the total gateway admission limit. */
  gateway_max_in_flight: number;
  upstream_response_limit_mib: number;
  admin_api_max_requests: number;
  admin_api_window_seconds: number;
  gateway_key_capacity: number;
  gateway_key_refill_tokens: number;
  gateway_key_refill_interval_ms: number;
  request_log_retention_days: number;
  request_log_max_rows: number;
  upstream: OperationalUpstreamSettingsValues;
}

export interface OperationalUpstreamSettingsValues {
  server_retry_max_attempts: number;
  server_retry_delay_first_ms: number;
  server_retry_delay_second_ms: number;
  continuity_retry_base_delay_ms: number;
  continuity_retry_max_delay_ms: number;
  gateway_body_processing_queue_timeout_ms: number;
  continuity_replay_bytes_per_run_mib: number;
  continuity_replay_bytes_total_mib: number;
  continuity_replay_events_per_run: number;
  continuity_retained_runs: number;
  continuity_resume_page_size: number;
  continuity_poll_interval_ms: number;
  continuity_heartbeat_interval_seconds: number;
  continuity_setup_error_kib: number;
  continuity_retention_seconds: number;
  continuity_request_timeout_seconds: number;
  provider_live_events_max_duration_seconds: number;
  provider_live_events_keepalive_seconds: number;
  model_discovery_max_pages: number;
  discovery_request_timeout_seconds: number;
  usage_response_limit_kib: number;
  opencode_usage_timeout_seconds: number;
  command_code_usage_timeout_seconds: number;
  command_code_optional_usage_timeout_seconds: number;
  freebuff_auxiliary_timeout_seconds: number;
  freebuff_auxiliary_response_limit_kib: number;
  remote_image_max_count: number;
  remote_image_max_mib: number;
  remote_image_total_max_mib: number;
  remote_image_connect_timeout_seconds: number;
  remote_image_request_timeout_seconds: number;
  codex_image_preparation_timeout_seconds: number;
  provider_client_cache_ttl_seconds: number;
  provider_client_cache_max_entries: number;
  provider_client_max_resolved_addresses: number;
}

export interface OperationalSettingsSnapshot {
  settings: OperationalSettingsValues;
  revision: number;
  overridden: boolean;
  source: 'database' | 'environment';
}

export type OutputStyleId = 'terse-prose' | 'less-code' | 'ponytail';
export type OutputStyleLevel = 'lite' | 'full' | 'ultra';

export interface OutputStyleSelection {
  id: OutputStyleId;
  level: OutputStyleLevel;
}

export interface OutputStylesSnapshot {
  styles: OutputStyleSelection[];
  revision: number;
  overridden: boolean;
  source: 'database' | 'default';
}

export type PublicSettings = Record<string, string | number | boolean | null>;

export type UpdateCheckStatus = 'up_to_date' | 'update_available' | 'unavailable';

export interface UpdateCheckResult {
  status: UpdateCheckStatus;
  current_version: string;
  latest_version: string | null;
  update_available: boolean;
  release_url: string | null;
}

export type Collection<T> = T[] | { data: T[]; total?: number };
