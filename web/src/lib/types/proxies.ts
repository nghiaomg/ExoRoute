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
