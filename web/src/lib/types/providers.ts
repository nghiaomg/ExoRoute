import type { UpstreamProtocol } from './protocols';

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
  /** Present when the adapter signs in with a device code. */
  method?: 'authorization_code' | 'device_code';
  /** The code the operator approves in the provider's own page. */
  user_code?: string;
  expires_in?: number;
  /** How often the dashboard may poll the device authorization. */
  interval?: number;
}

export interface ProviderAuthStatus {
  status: 'pending' | 'connected' | 'failed' | 'expired';
  message?: string;
  user_code?: string | null;
  verification_url?: string | null;
}

export interface ProviderAuthPollResult {
  status: ProviderAuthStatus['status'];
  message?: string | null;
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
