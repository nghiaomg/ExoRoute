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
