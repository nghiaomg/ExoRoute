export interface ProxyEndpoint {
  endpoint_id?: string;
  url?: string | null;
  ref_proxy_id?: string | null;
  username?: string | null;
  /** Password plaintext is write-only; the API returns only has_secret. */
  secret?: string | null;
  has_secret?: boolean;
  label?: string | null;
  priority: number;
  enabled: boolean;
}

export interface GatewayProxy {
  id: string;
  name: string;
  strategy: 'priority' | 'round_robin';
  enabled: boolean;
  endpoints: ProxyEndpoint[];
  created_at: string;
  updated_at: string;
  provider_count: number;
}

export interface ProxyProviderOption {
  id: string;
  name: string;
  enabled: boolean;
}

export interface ProxyTrafficStats {
  proxy_id: string;
  received_bytes: number;
  sent_bytes: number;
  total_bytes: number;
  requests: number;
  average_bytes_per_second: number | null;
}

export interface ProxyTrafficStatsResponse {
  proxies: ProxyTrafficStats[];
  scope: 'since_startup';
  truncated: boolean;
}
