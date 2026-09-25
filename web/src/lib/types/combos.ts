import type { Protocol, UpstreamProtocol } from './protocols';

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
  manual_models: string[];
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
  manual_kept?: number;
}
