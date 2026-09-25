import type { OperationalSettingsValues, OperationalUpstreamSettingsValues } from '../../lib/types';

  export type ConfigHelp = {
    titleKey: string;
    descKey: string;
    violationKey: string;
    httpStatus: string;
    errorCode: string;
    defaultVal: string;
    safeRange: string;
  };

  export const defaults: OperationalSettingsValues = {
    connect_timeout_ms: 10_000,
    request_timeout_ms: 300_000,
    stream_idle_timeout_ms: 120_000,
    circuit_breaker_enabled: true,
    circuit_breaker_threshold: 5,
    circuit_breaker_cooldown_seconds: 30,
    gateway_max_in_flight: 64,
    upstream_response_limit_mib: 16,
    admin_api_max_requests: 300,
    admin_api_window_seconds: 60,
    gateway_key_capacity: 5_000,
    gateway_key_refill_tokens: 1,
    gateway_key_refill_interval_ms: 1,
    request_log_retention_days: 30,
    request_log_max_rows: 100_000,
    upstream: {
      server_retry_max_attempts: 3,
      server_retry_delay_first_ms: 250,
      server_retry_delay_second_ms: 500,
      continuity_retry_base_delay_ms: 250,
      continuity_retry_max_delay_ms: 60_000,
      gateway_body_processing_queue_timeout_ms: 5_000,
      continuity_replay_bytes_per_run_mib: 8,
      continuity_replay_bytes_total_mib: 128,
      continuity_replay_events_per_run: 50_000,
      continuity_retained_runs: 128,
      continuity_resume_page_size: 8,
      continuity_poll_interval_ms: 200,
      continuity_heartbeat_interval_seconds: 15,
      continuity_setup_error_kib: 64,
      continuity_retention_seconds: 86_400,
      continuity_request_timeout_seconds: 86_400,
      provider_live_events_max_duration_seconds: 300,
      provider_live_events_keepalive_seconds: 30,
      model_discovery_max_pages: 20,
      discovery_request_timeout_seconds: 10,
      usage_response_limit_kib: 256,
      opencode_usage_timeout_seconds: 8,
      command_code_usage_timeout_seconds: 10,
      command_code_optional_usage_timeout_seconds: 3,
      freebuff_auxiliary_timeout_seconds: 10,
      freebuff_auxiliary_response_limit_kib: 64,
      remote_image_max_count: 8,
      remote_image_max_mib: 6,
      remote_image_total_max_mib: 8,
      remote_image_connect_timeout_seconds: 3,
      remote_image_request_timeout_seconds: 15,
      codex_image_preparation_timeout_seconds: 30,
      provider_client_cache_ttl_seconds: 30,
      provider_client_cache_max_entries: 256,
      provider_client_max_resolved_addresses: 64,
    },
  };

  export const coreHelpMap: Record<string, ConfigHelp> = {
    connect_timeout_ms: {
      titleKey: 'Upstream connect timeout',
      descKey: 'Maximum duration to establish connection with upstream provider.',
      violationKey: 'Provider network is unreachable or handshake fails in time.',
      httpStatus: '504 Gateway Timeout',
      errorCode: 'connect_timeout',
      defaultVal: '10000 ms',
      safeRange: '100–120000 ms',
    },
    request_timeout_ms: {
      titleKey: 'Upstream request timeout',
      descKey: 'Maximum overall duration for an upstream inference request.',
      violationKey: 'Generation takes longer than this configured ceiling.',
      httpStatus: '504 Gateway Timeout',
      errorCode: 'request_timeout',
      defaultVal: '300000 ms',
      safeRange: '100 ms–24 hours',
    },
    stream_idle_timeout_ms: {
      titleKey: 'Stream idle timeout',
      descKey: 'Maximum pause allowed between streaming data chunks.',
      violationKey: 'Provider stream stalls or halts emission beyond threshold.',
      httpStatus: '504 Gateway Timeout',
      errorCode: 'stream_idle_timeout',
      defaultVal: '120000 ms',
      safeRange: '100 ms–1 hour',
    },
    circuit_breaker_enabled: {
      titleKey: 'Circuit breaker',
      descKey: 'Automated protection that halts calls to failing providers.',
      violationKey: 'Provider has repeated failures; calls are blocked to protect queue.',
      httpStatus: '503 Service Unavailable',
      errorCode: 'circuit_breaker_open',
      defaultVal: 'Enabled',
      safeRange: 'Enabled / Disabled',
    },
    circuit_breaker_threshold: {
      titleKey: 'Circuit breaker failure threshold',
      descKey: 'Consecutive failures required to trip the circuit breaker.',
      violationKey: 'Reaching this count opens circuit for all requests to provider.',
      httpStatus: '503 Service Unavailable',
      errorCode: 'circuit_breaker_tripped',
      defaultVal: '5 failures',
      safeRange: '1–100 failures',
    },
    circuit_breaker_cooldown_seconds: {
      titleKey: 'Circuit breaker cooldown',
      descKey: 'Cool-off duration an open circuit waits before test probe.',
      violationKey: 'Requests during cooldown are rejected without touching upstream.',
      httpStatus: '503 Service Unavailable',
      errorCode: 'provider_cooling_down',
      defaultVal: '30 seconds',
      safeRange: '1–86400 seconds',
    },
    gateway_max_in_flight: {
      titleKey: 'Total concurrent gateway requests',
      descKey: 'Process-wide limit for total concurrent active requests.',
      violationKey: 'Active concurrent requests exceed server capacity ceiling.',
      httpStatus: '503 Service Unavailable',
      errorCode: 'gateway_overloaded',
      defaultVal: '64 requests',
      safeRange: '0 (unlimited) or 1–64 requests',
    },
    upstream_response_limit_mib: {
      titleKey: 'Upstream response limit',
      descKey: 'Maximum response payload size accepted from upstream.',
      violationKey: 'Upstream returns response body larger than this ceiling.',
      httpStatus: '502 Bad Gateway',
      errorCode: 'response_payload_too_large',
      defaultVal: '16 MiB',
      safeRange: '1–16 MiB',
    },
    admin_api_max_requests: {
      titleKey: 'Authenticated admin API requests',
      descKey: 'Maximum API calls permitted for an admin session per window.',
      violationKey: 'Admin session exceeds allowed request quota.',
      httpStatus: '429 Too Many Requests',
      errorCode: 'admin_rate_limit_exceeded',
      defaultVal: '300 requests',
      safeRange: '1–4294967295 requests',
    },
    admin_api_window_seconds: {
      titleKey: 'Authenticated admin API window',
      descKey: 'Sliding time window for computing admin rate limits.',
      violationKey: 'Admin request quota exhausted within this rolling period.',
      httpStatus: '429 Too Many Requests',
      errorCode: 'admin_window_exhausted',
      defaultVal: '60 seconds',
      safeRange: '1–86400 seconds',
    },
    gateway_key_capacity: {
      titleKey: 'Gateway API key token capacity',
      descKey: 'Maximum burst tokens in bucket for each gateway API key.',
      violationKey: 'Client bursts requests with zero remaining bucket tokens.',
      httpStatus: '429 Too Many Requests',
      errorCode: 'rate_limit_exceeded',
      defaultVal: '5000 tokens',
      safeRange: '1–4294967295 tokens',
    },
    gateway_key_refill_tokens: {
      titleKey: 'Gateway API key refill amount',
      descKey: 'Tokens replenished into the API key bucket per tick.',
      violationKey: 'Calling API faster than refill rate drains the key bucket.',
      httpStatus: '429 Too Many Requests',
      errorCode: 'rate_limit_depleted',
      defaultVal: '1 token',
      safeRange: '1–4294967295 tokens',
    },
    gateway_key_refill_interval_ms: {
      titleKey: 'Gateway API key refill interval',
      descKey: 'Interval between token replenishment ticks for API keys.',
      violationKey: 'Requests arrive faster than the bucket refill tick rate.',
      httpStatus: '429 Too Many Requests',
      errorCode: 'rate_limit_interval_starved',
      defaultVal: '1 ms',
      safeRange: '1–86400000 ms',
    },
    request_log_retention_days: {
      titleKey: 'Request log retention',
      descKey: 'Days request telemetry and logs are kept before pruning.',
      violationKey: 'Logs older than threshold are pruned permanently in LMDB.',
      httpStatus: 'System Pruning',
      errorCode: 'log_retention_expired',
      defaultVal: '30 days',
      safeRange: '1–365 days',
    },
    request_log_max_rows: {
      titleKey: 'Maximum request log rows',
      descKey: 'Maximum number of request log records stored in database.',
      violationKey: 'Table reaches capacity; oldest records are evicted FIFO.',
      httpStatus: 'FIFO Eviction',
      errorCode: 'log_capacity_reached',
      defaultVal: '100000 rows',
      safeRange: '1–100000 rows',
    },
  };

  export type UpstreamKey = keyof OperationalUpstreamSettingsValues;

  /** Pure draft validation mirroring the backend's bounded safe ranges. */
  export function isValidOperationalSettings(draft: OperationalSettingsValues): boolean {
    return Number.isSafeInteger(draft.connect_timeout_ms)
      && draft.connect_timeout_ms >= 100 && draft.connect_timeout_ms <= 120_000
      && Number.isSafeInteger(draft.request_timeout_ms)
      && draft.request_timeout_ms >= 100 && draft.request_timeout_ms <= 86_400_000
      && Number.isSafeInteger(draft.stream_idle_timeout_ms)
      && draft.stream_idle_timeout_ms >= 100 && draft.stream_idle_timeout_ms <= 3_600_000
      && Number.isSafeInteger(draft.circuit_breaker_threshold)
      && draft.circuit_breaker_threshold >= 1 && draft.circuit_breaker_threshold <= 100
      && Number.isSafeInteger(draft.circuit_breaker_cooldown_seconds)
      && draft.circuit_breaker_cooldown_seconds >= 1 && draft.circuit_breaker_cooldown_seconds <= 86_400
      && Number.isSafeInteger(draft.gateway_max_in_flight)
      && (draft.gateway_max_in_flight === 0 || (draft.gateway_max_in_flight >= 1 && draft.gateway_max_in_flight <= 64))
      && Number.isSafeInteger(draft.upstream_response_limit_mib)
      && draft.upstream_response_limit_mib >= 1 && draft.upstream_response_limit_mib <= 16
      && Number.isSafeInteger(draft.admin_api_max_requests)
      && draft.admin_api_max_requests >= 1 && draft.admin_api_max_requests <= 4_294_967_295
      && Number.isSafeInteger(draft.admin_api_window_seconds)
      && draft.admin_api_window_seconds >= 1 && draft.admin_api_window_seconds <= 86_400
      && Number.isSafeInteger(draft.gateway_key_capacity)
      && draft.gateway_key_capacity >= 1 && draft.gateway_key_capacity <= 4_294_967_295
      && Number.isSafeInteger(draft.gateway_key_refill_tokens)
      && draft.gateway_key_refill_tokens >= 1 && draft.gateway_key_refill_tokens <= 4_294_967_295
      && Number.isSafeInteger(draft.gateway_key_refill_interval_ms)
      && draft.gateway_key_refill_interval_ms >= 1 && draft.gateway_key_refill_interval_ms <= 86_400_000
      && Number.isSafeInteger(draft.request_log_retention_days)
      && draft.request_log_retention_days >= 1 && draft.request_log_retention_days <= 365
      && Number.isSafeInteger(draft.request_log_max_rows)
      && draft.request_log_max_rows >= 1 && draft.request_log_max_rows <= 100_000
      && upstreamFields.every((field) => {
        const value = draft.upstream[field.key];
        return Number.isSafeInteger(value) && value >= field.min && value <= field.max;
      })
      && draft.upstream.continuity_replay_bytes_total_mib >= draft.upstream.continuity_replay_bytes_per_run_mib
      && draft.upstream.continuity_retry_max_delay_ms >= draft.upstream.continuity_retry_base_delay_ms
      && draft.upstream.provider_live_events_keepalive_seconds < draft.upstream.provider_live_events_max_duration_seconds
      && draft.upstream.command_code_optional_usage_timeout_seconds <= draft.upstream.command_code_usage_timeout_seconds
      && draft.upstream.remote_image_total_max_mib >= draft.upstream.remote_image_max_mib;
  }

  /** Field-wise comparison across the top level and every upstream key. */
  export function sameOperationalSettings(left: OperationalSettingsValues, right: OperationalSettingsValues): boolean {
    const topLevelKeys = (Object.keys(defaults) as (keyof OperationalSettingsValues)[])
      .filter((key) => key !== 'upstream');
    return topLevelKeys.every((key) => left[key] === right[key])
      && upstreamFields.every(({ key }) => left.upstream[key] === right.upstream[key]);
  }

  /** Keys of the top-level settings whose value is numeric. */
  type NumericSettingKey = {
    [K in keyof OperationalSettingsValues]: OperationalSettingsValues[K] extends number ? K : never;
  }[keyof OperationalSettingsValues];

  export type CoreNumericField = {
    key: NumericSettingKey;
    id: string;
    labelKey: string;
    min: number;
    max: number;
    /** Locale-neutral unit suffix (e.g. "ms", "MiB"). */
    unit?: string;
    /** Translation key for a translatable unit suffix. */
    unitKey?: string;
    /** Translation key for the hint line, when the field has one. */
    hintKey?: string;
  };

  /** Numeric fields rendered above the circuit-breaker checkbox. */
  export const coreTimeoutFields: CoreNumericField[] = [
    { key: 'connect_timeout_ms', id: 'operational-connect-timeout', labelKey: 'Upstream connect timeout', min: 100, max: 120_000, unit: 'ms', hintKey: 'Allowed range: 100–120000 ms. Default: 10000 ms.' },
    { key: 'request_timeout_ms', id: 'operational-request-timeout', labelKey: 'Upstream request timeout', min: 100, max: 86_400_000, unit: 'ms', hintKey: 'Allowed range: 100 ms–24 hours. Default: 300000 ms.' },
    { key: 'stream_idle_timeout_ms', id: 'operational-stream-timeout', labelKey: 'Stream idle timeout', min: 100, max: 3_600_000, unit: 'ms', hintKey: 'Allowed range: 100 ms–1 hour. Default: 120000 ms.' },
  ];

  /** Numeric fields rendered below the circuit-breaker checkbox. */
  export const coreLimitFields: CoreNumericField[] = [
    { key: 'circuit_breaker_threshold', id: 'operational-circuit-threshold', labelKey: 'Circuit breaker failure threshold', min: 1, max: 100, unitKey: 'failures' },
    { key: 'circuit_breaker_cooldown_seconds', id: 'operational-circuit-cooldown', labelKey: 'Circuit breaker cooldown', min: 1, max: 86_400, unitKey: 'seconds' },
    { key: 'gateway_max_in_flight', id: 'operational-gateway-in-flight', labelKey: 'Total concurrent gateway requests', min: 0, max: 64, unitKey: 'requests', hintKey: 'Choose 1–64, or 0 for unlimited. Default: 64.' },
    { key: 'upstream_response_limit_mib', id: 'operational-response-limit', labelKey: 'Upstream response limit', min: 1, max: 16, unit: 'MiB', hintKey: 'Allowed range: 1–16 MiB. The hard ceiling remains 16 MiB.' },
    { key: 'admin_api_max_requests', id: 'operational-admin-api-limit', labelKey: 'Authenticated admin API requests', min: 1, max: 4_294_967_295, unitKey: 'requests' },
    { key: 'admin_api_window_seconds', id: 'operational-admin-api-window', labelKey: 'Authenticated admin API window', min: 1, max: 86_400, unitKey: 'seconds', hintKey: 'Login and authentication-failure throttles remain fixed.' },
    { key: 'gateway_key_capacity', id: 'operational-gateway-burst', labelKey: 'Gateway API key token capacity', min: 1, max: 4_294_967_295, unitKey: 'tokens' },
    { key: 'gateway_key_refill_tokens', id: 'operational-gateway-refill-tokens', labelKey: 'Gateway API key refill amount', min: 1, max: 4_294_967_295, unitKey: 'tokens' },
    { key: 'gateway_key_refill_interval_ms', id: 'operational-gateway-refill-interval', labelKey: 'Gateway API key refill interval', min: 1, max: 86_400_000, unit: 'ms', hintKey: 'Refill amount and interval define the token-bucket refill rate.' },
    { key: 'request_log_retention_days', id: 'operational-log-retention', labelKey: 'Request log retention', min: 1, max: 365, unitKey: 'days' },
    { key: 'request_log_max_rows', id: 'operational-log-rows', labelKey: 'Maximum request log rows', min: 1, max: 100_000, unitKey: 'rows', hintKey: 'The maximum remains 100000 rows.' },
  ];

  export type UpstreamField = {
    key: UpstreamKey;
    label: string;
    unit: string;
    min: number;
    max: number;
    descKey: string;
    violationKey: string;
    httpStatus: string;
    errorCode: string;
    defaultVal: string;
    safeRange: string;
  };

  export const upstreamFields: UpstreamField[] = [
    { key: 'server_retry_max_attempts', label: 'Maximum upstream retry attempts', unit: 'attempts', min: 1, max: 8, descKey: 'Maximum retry attempts for transient upstream errors.', violationKey: 'All retry attempts fail to obtain a healthy upstream response.', httpStatus: '502 Bad Gateway', errorCode: 'upstream_retry_exhausted', defaultVal: '3 attempts', safeRange: '1–8 attempts' },
    { key: 'server_retry_delay_first_ms', label: 'First retry delay', unit: 'ms', min: 1, max: 60_000, descKey: 'Initial backoff wait before the first retry attempt.', violationKey: 'Excessive delay combined with latency exceeds request timeout.', httpStatus: '504 Gateway Timeout', errorCode: 'retry_delay_timeout', defaultVal: '250 ms', safeRange: '1–60000 ms' },
    { key: 'server_retry_delay_second_ms', label: 'Second retry delay', unit: 'ms', min: 1, max: 60_000, descKey: 'Backoff wait before the second retry attempt.', violationKey: 'Cumulative retry delays exceed client request deadline.', httpStatus: '504 Gateway Timeout', errorCode: 'retry_cumulative_timeout', defaultVal: '500 ms', safeRange: '1–60000 ms' },
    { key: 'continuity_retry_base_delay_ms', label: 'Continuity retry base delay', unit: 'ms', min: 1, max: 60_000, descKey: 'Initial backoff delay for background continuity tasks.', violationKey: 'Background task retries repeatedly when upstream is down.', httpStatus: '502 Bad Gateway', errorCode: 'continuity_upstream_unavailable', defaultVal: '250 ms', safeRange: '1–60000 ms' },
    { key: 'continuity_retry_max_delay_ms', label: 'Continuity retry maximum delay', unit: 'ms', min: 1, max: 3_600_000, descKey: 'Maximum retry backoff ceiling for background streaming.', violationKey: 'Task retries at max interval until overall timeout fires.', httpStatus: '504 Gateway Timeout', errorCode: 'continuity_retry_exhausted', defaultVal: '60000 ms', safeRange: '1–3600000 ms' },
    { key: 'gateway_body_processing_queue_timeout_ms', label: 'Gateway body queue wait', unit: 'ms', min: 100, max: 60_000, descKey: 'Maximum wait in CPU-bounded body decoding queue.', violationKey: 'Worker threads saturated; request wait exceeds threshold.', httpStatus: '503 Service Unavailable', errorCode: 'body_queue_timeout', defaultVal: '5000 ms', safeRange: '100–60000 ms' },
    { key: 'continuity_replay_bytes_per_run_mib', label: 'Continuity replay per run', unit: 'MiB', min: 1, max: 64, descKey: 'Replay buffer memory ceiling for a single stream session.', violationKey: 'Output exceeds buffer; earliest tokens are dropped.', httpStatus: '413 Payload Too Large', errorCode: 'continuity_run_buffer_overflow', defaultVal: '8 MiB', safeRange: '1–64 MiB' },
    { key: 'continuity_replay_bytes_total_mib', label: 'Continuity replay total', unit: 'MiB', min: 1, max: 1_024, descKey: 'Total aggregate replay memory across all active streams.', violationKey: 'Buffer fills up; oldest finished stream caches are evicted.', httpStatus: '503 Service Unavailable', errorCode: 'continuity_total_buffer_exhausted', defaultVal: '128 MiB', safeRange: '1–1024 MiB' },
    { key: 'continuity_replay_events_per_run', label: 'Continuity events per run', unit: 'events', min: 1, max: 1_000_000, descKey: 'Maximum SSE events stored in memory per stream run.', violationKey: 'Event count exceeds limit; oldest events are discarded.', httpStatus: '413 Payload Too Large', errorCode: 'continuity_event_cap_exceeded', defaultVal: '50000 events', safeRange: '1–1000000 events' },
    { key: 'continuity_retained_runs', label: 'Retained continuity runs', unit: 'runs', min: 1, max: 1_024, descKey: 'Maximum completed continuity runs kept in database.', violationKey: 'Run count reaches ceiling; oldest records are removed.', httpStatus: 'FIFO Eviction', errorCode: 'continuity_runs_ceiling', defaultVal: '128 runs', safeRange: '1–1024 runs' },
    { key: 'continuity_resume_page_size', label: 'Continuity resume page size', unit: 'events', min: 1, max: 64, descKey: 'SSE events batched per page when client reconnects.', violationKey: 'Paces replay delivery to prevent flooding client connection.', httpStatus: '400 Bad Request', errorCode: 'invalid_resume_page_size', defaultVal: '8 events', safeRange: '1–64 events' },
    { key: 'continuity_poll_interval_ms', label: 'Continuity resume poll interval', unit: 'ms', min: 10, max: 5_000, descKey: 'Polling interval when client catches up to live stream.', violationKey: 'Controls token update frequency and CPU usage.', httpStatus: '429 Too Many Requests', errorCode: 'continuity_poll_throttled', defaultVal: '200 ms', safeRange: '10–5000 ms' },
    { key: 'continuity_heartbeat_interval_seconds', label: 'Continuity heartbeat interval', unit: 'seconds', min: 1, max: 300, descKey: 'Interval between periodic SSE comment pings (: ping).', violationKey: 'Interval too long allows intermediate proxies to drop link.', httpStatus: '499 Client Closed Request', errorCode: 'stream_connection_dropped', defaultVal: '15 seconds', safeRange: '1–300 seconds' },
    { key: 'continuity_setup_error_kib', label: 'Continuity setup error limit', unit: 'KiB', min: 1, max: 1_024, descKey: 'Maximum upstream error details saved if setup fails.', violationKey: 'Lengthy upstream error payload is safely truncated.', httpStatus: '502 Bad Gateway', errorCode: 'upstream_error_truncated', defaultVal: '64 KiB', safeRange: '1–1024 KiB' },
    { key: 'continuity_retention_seconds', label: 'Continuity retention', unit: 'seconds', min: 3_600, max: 604_800, descKey: 'Duration a completed stream remains available to replay.', violationKey: 'Client reconnects after stream retention window expired.', httpStatus: '404 Not Found', errorCode: 'stream_run_expired', defaultVal: '86400 seconds', safeRange: '3600–604800 seconds' },
    { key: 'continuity_request_timeout_seconds', label: 'Continuity request timeout', unit: 'seconds', min: 60, max: 604_800, descKey: 'Absolute maximum runtime for background continuity tasks.', violationKey: 'Inference runs longer than ceiling; task is aborted.', httpStatus: '504 Gateway Timeout', errorCode: 'continuity_task_timeout', defaultVal: '86400 seconds', safeRange: '60–604800 seconds' },
    { key: 'provider_live_events_max_duration_seconds', label: 'Provider live event stream duration', unit: 'seconds', min: 60, max: 3_600, descKey: 'Maximum duration for listening to live provider event feeds.', violationKey: 'Stream reaches limit; connection terminates for reconnect.', httpStatus: '504 Gateway Timeout', errorCode: 'live_events_stream_expired', defaultVal: '300 seconds', safeRange: '60–3600 seconds' },
    { key: 'provider_live_events_keepalive_seconds', label: 'Provider live event keepalive', unit: 'seconds', min: 1, max: 300, descKey: 'Keepalive ping interval for live provider event sockets.', violationKey: 'Too high interval may allow silent socket disconnections.', httpStatus: '504 Gateway Timeout', errorCode: 'live_events_keepalive_timeout', defaultVal: '30 seconds', safeRange: '1–300 seconds' },
    { key: 'model_discovery_max_pages', label: 'Model discovery maximum pages', unit: 'pages', min: 1, max: 100, descKey: 'Maximum pages scanned when discovering provider models.', violationKey: 'Provider catalog has more pages; models beyond are omitted.', httpStatus: '502 Bad Gateway', errorCode: 'discovery_page_limit_reached', defaultVal: '20 pages', safeRange: '1–100 pages' },
    { key: 'discovery_request_timeout_seconds', label: 'Model discovery timeout', unit: 'seconds', min: 1, max: 300, descKey: 'Timeout when querying provider model lists (/v1/models).', violationKey: 'Provider model endpoint fails to reply within time limit.', httpStatus: '504 Gateway Timeout', errorCode: 'discovery_timeout', defaultVal: '10 seconds', safeRange: '1–300 seconds' },
    { key: 'usage_response_limit_kib', label: 'Provider usage response limit', unit: 'KiB', min: 64, max: 16_384, descKey: 'Maximum payload accepted from provider quota balance API.', violationKey: 'Quota balance response payload exceeds buffer ceiling.', httpStatus: '502 Bad Gateway', errorCode: 'usage_response_too_large', defaultVal: '256 KiB', safeRange: '64–16384 KiB' },
    { key: 'opencode_usage_timeout_seconds', label: 'OpenCode usage timeout', unit: 'seconds', min: 1, max: 300, descKey: 'Timeout when querying OpenCode quota and balances.', violationKey: 'OpenCode usage service is slow or unresponsive.', httpStatus: '504 Gateway Timeout', errorCode: 'opencode_usage_timeout', defaultVal: '8 seconds', safeRange: '1–300 seconds' },
    { key: 'command_code_usage_timeout_seconds', label: 'Command Code usage timeout', unit: 'seconds', min: 1, max: 300, descKey: 'Timeout when querying Command Code quota endpoints.', violationKey: 'Command Code quota API fails to respond within limit.', httpStatus: '504 Gateway Timeout', errorCode: 'command_code_usage_timeout', defaultVal: '10 seconds', safeRange: '1–300 seconds' },
    { key: 'command_code_optional_usage_timeout_seconds', label: 'Command Code optional usage timeout', unit: 'seconds', min: 1, max: 300, descKey: 'Timeout for optional auxiliary queries to Command Code.', violationKey: 'Optional check times out; silently bypassed.', httpStatus: '200 OK (Bypassed)', errorCode: 'optional_usage_bypassed', defaultVal: '3 seconds', safeRange: '1–300 seconds' },
    { key: 'freebuff_auxiliary_timeout_seconds', label: 'Freebuff auxiliary timeout', unit: 'seconds', min: 1, max: 300, descKey: 'Timeout for auxiliary service calls to Freebuff.', violationKey: 'Freebuff auxiliary endpoint is slow or unreachable.', httpStatus: '504 Gateway Timeout', errorCode: 'freebuff_aux_timeout', defaultVal: '10 seconds', safeRange: '1–300 seconds' },
    { key: 'freebuff_auxiliary_response_limit_kib', label: 'Freebuff auxiliary response limit', unit: 'KiB', min: 4, max: 16_384, descKey: 'Maximum response size for Freebuff auxiliary calls.', violationKey: 'Freebuff auxiliary response exceeds size limit.', httpStatus: '502 Bad Gateway', errorCode: 'freebuff_aux_response_limit', defaultVal: '64 KiB', safeRange: '4–16384 KiB' },
    { key: 'remote_image_max_count', label: 'Remote images per request', unit: 'images', min: 1, max: 64, descKey: 'Maximum remote image URLs fetched per multimodal request.', violationKey: 'Prompt contains more remote image URLs than allowed.', httpStatus: '400 Bad Request', errorCode: 'too_many_remote_images', defaultVal: '8 images', safeRange: '1–64 images' },
    { key: 'remote_image_max_mib', label: 'Remote image size', unit: 'MiB', min: 1, max: 64, descKey: 'Maximum file size allowed for any single remote image.', violationKey: 'A remote image file exceeds this download size threshold.', httpStatus: '413 Payload Too Large', errorCode: 'remote_image_too_large', defaultVal: '6 MiB', safeRange: '1–64 MiB' },
    { key: 'remote_image_total_max_mib', label: 'Remote images total size', unit: 'MiB', min: 1, max: 256, descKey: 'Maximum combined size of all remote images in a request.', violationKey: 'Cumulative size of all images exceeds total budget.', httpStatus: '413 Payload Too Large', errorCode: 'total_images_size_exceeded', defaultVal: '8 MiB', safeRange: '1–256 MiB' },
    { key: 'remote_image_connect_timeout_seconds', label: 'Remote image connect timeout', unit: 'seconds', min: 1, max: 60, descKey: 'Connection timeout when connecting to image host server.', violationKey: 'Image host server is unreachable or drops connection.', httpStatus: '504 Gateway Timeout', errorCode: 'image_connect_timeout', defaultVal: '3 seconds', safeRange: '1–60 seconds' },
    { key: 'remote_image_request_timeout_seconds', label: 'Remote image request timeout', unit: 'seconds', min: 1, max: 300, descKey: 'Total download timeout for retrieving remote image data.', violationKey: 'Image download takes longer than configured ceiling.', httpStatus: '504 Gateway Timeout', errorCode: 'image_download_timeout', defaultVal: '15 seconds', safeRange: '1–300 seconds' },
    { key: 'codex_image_preparation_timeout_seconds', label: 'Codex image preparation timeout', unit: 'seconds', min: 1, max: 600, descKey: 'Timeout for preparing image inputs for Codex protocol.', violationKey: 'Image format preparation exceeds time limit.', httpStatus: '504 Gateway Timeout', errorCode: 'image_preparation_timeout', defaultVal: '30 seconds', safeRange: '1–600 seconds' },
    { key: 'provider_client_cache_ttl_seconds', label: 'Provider client cache TTL', unit: 'seconds', min: 1, max: 3_600, descKey: 'TTL for cached provider HTTP client connection pools.', violationKey: 'Expired clients are evicted to trigger fresh DNS lookup.', httpStatus: 'Pool Eviction', errorCode: 'client_pool_evicted', defaultVal: '30 seconds', safeRange: '1–3600 seconds' },
    { key: 'provider_client_cache_max_entries', label: 'Cached provider clients', unit: 'clients', min: 1, max: 4_096, descKey: 'Maximum cached provider HTTP client instances in memory.', violationKey: 'Client count reaches ceiling; least-recently-used evicted.', httpStatus: 'LRU Eviction', errorCode: 'client_cache_overflow', defaultVal: '256 clients', safeRange: '1–4096 clients' },
    { key: 'provider_client_max_resolved_addresses', label: 'Resolved addresses per provider', unit: 'addresses', min: 1, max: 256, descKey: 'Maximum IP addresses pinned per provider for DNS defense.', violationKey: 'DNS returns more IPs; excess addresses are excluded.', httpStatus: 'DNS Pinned Limit', errorCode: 'dns_pinning_ceiling', defaultVal: '64 addresses', safeRange: '1–256 addresses' },
  ];
