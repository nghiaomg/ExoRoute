import type { RequestLiveEvent, RequestLiveRow, RequestLog, RequestLogFilters } from '../../lib/types';

export type RequestLiveState = {
  requests: Map<string, RequestLiveRow>;
  truncated: boolean;
  pendingFinished: Map<string, RequestLog>;
  activeCount: number;
};

export type RequestLiveEventResult = {
  state: RequestLiveState;
  finished: RequestLog | null;
};

export function createRequestLiveState(): RequestLiveState {
  return {
    requests: new Map(),
    truncated: false,
    pendingFinished: new Map(),
    activeCount: 0,
  };
}

export function hasRequestFilters(value: RequestLogFilters): boolean {
  return Boolean(value.api_key_id?.trim() || value.model?.trim() || value.provider_id?.trim() || value.status);
}

export function matchesRequestFilters(request: RequestLog, filters: RequestLogFilters): boolean {
  const apiKeyFilter = filters.api_key_id?.trim();
  if (apiKeyFilter) {
    if (apiKeyFilter.toLowerCase() === 'unknown') {
      if (request.api_key_id) return false;
    } else if (request.api_key_id !== apiKeyFilter) {
      return false;
    }
  }
  const modelFilter = filters.model?.trim();
  if (modelFilter && request.model !== modelFilter) return false;
  const providerFilter = filters.provider_id?.trim();
  if (providerFilter && request.provider_id !== providerFilter) return false;
  if (filters.status && isLiveRequest(request)) return false;
  if (filters.status === 'success') return request.status != null && request.status >= 200 && request.status < 300;
  if (filters.status === 'failure') return request.status != null && (request.status < 200 || request.status >= 300);
  return true;
}

export function requestIdentity(request: RequestLog): string {
  return request.id ?? `${request.request_id ?? ''}:${request.created_at}`;
}

export function isLiveRequest(request: RequestLog | RequestLiveRow): request is RequestLiveRow {
  return 'live' in request && request.live === true;
}

export function mergeFinishedRequests(
  loaded: RequestLog[],
  cursor: string | null,
  pendingFinished: Map<string, RequestLog>,
  filters: RequestLogFilters,
): { requests: RequestLog[]; pendingFinished: Map<string, RequestLog> } {
  if (cursor !== null || pendingFinished.size === 0) {
    return { requests: loaded, pendingFinished };
  }

  const loadedIdentities = new Set(loaded.map(requestIdentity));
  const pending = Array.from(pendingFinished.values()).filter((request) => matchesRequestFilters(request, filters));
  const remaining = new Map(pendingFinished);
  for (const request of loaded) remaining.delete(requestIdentity(request));
  return {
    requests: [...pending.filter((request) => !loadedIdentities.has(requestIdentity(request))), ...loaded].slice(0, 50),
    pendingFinished: remaining,
  };
}

export function applyRequestLiveEvent(
  state: RequestLiveState,
  event: RequestLiveEvent,
  filters: RequestLogFilters,
): RequestLiveEventResult {
  if (event.type === 'snapshot') {
    return {
      state: {
        ...state,
        requests: new Map(event.requests.map((request) => [request.live_id, request])),
        truncated: event.truncated,
        activeCount: event.active_count,
      },
      finished: null,
    };
  }

  if (event.type === 'started') {
    return {
      state: {
        ...state,
        requests: new Map(state.requests).set(event.request.live_id, event.request),
        activeCount: event.active_count ?? state.activeCount + 1,
      },
      finished: null,
    };
  }

  if (event.type === 'updated') {
    const request = state.requests.get(event.live_id);
    if (!request) return { state, finished: null };
    return {
      state: {
        ...state,
        requests: new Map(state.requests).set(event.live_id, {
          ...request,
          id: event.latest_log_id ?? request.id,
          provider_id: event.provider_id ?? undefined,
          input_tokens: event.input_tokens === undefined ? request.input_tokens : event.input_tokens ?? undefined,
          output_tokens: event.output_tokens === undefined ? request.output_tokens : event.output_tokens ?? undefined,
        }),
      },
      finished: null,
    };
  }

  if (event.type === 'capacity') {
    return {
      state: {
        ...state,
        truncated: event.truncated,
        activeCount: event.active_count ?? state.activeCount,
      },
      finished: null,
    };
  }

  const requests = new Map(state.requests);
  requests.delete(event.live_id);
  let pendingFinished = state.pendingFinished;
  if (event.request && matchesRequestFilters(event.request, filters)) {
    pendingFinished = rememberFinishedRequest(pendingFinished, event.request);
  }
  return {
    state: {
      ...state,
      requests,
      pendingFinished,
      activeCount: event.active_count ?? Math.max(0, state.activeCount - 1),
    },
    finished: event.request && matchesRequestFilters(event.request, filters) ? event.request : null,
  };
}

function rememberFinishedRequest(pendingFinished: Map<string, RequestLog>, request: RequestLog): Map<string, RequestLog> {
  const identity = requestIdentity(request);
  const next = new Map(pendingFinished);
  next.delete(identity);
  next.set(identity, request);
  while (next.size > 50) {
    const oldest = next.keys().next().value;
    if (oldest === undefined) break;
    next.delete(oldest);
  }
  return next;
}
