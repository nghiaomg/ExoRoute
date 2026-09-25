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
