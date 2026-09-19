import type { RequestLiveEvent, RequestLiveRow, RequestLog } from './types';

const MAX_VALID_EPOCH_MS = 8_640_000_000_000_000;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function boundedString(value: unknown, maxChars: number): string | undefined {
  return typeof value === 'string' && value.length > 0 && value.length <= maxChars ? value : undefined;
}

function optionalString(value: unknown, maxChars: number): string | null | undefined {
  if (value === null) return null;
  return value === undefined ? undefined : boundedString(value, maxChars);
}

function optionalSafeInteger(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isSafeInteger(value) ? value : undefined;
}

function optionalActiveCount(value: Record<string, unknown>): number | null | undefined {
  if (value.active_count === undefined) return undefined;
  const count = optionalSafeInteger(value.active_count);
  return count !== undefined && count >= 0 ? count : null;
}

function optionalEpochMs(value: unknown): number | undefined {
  const epochMs = optionalSafeInteger(value);
  return epochMs !== undefined && epochMs >= 0 && epochMs <= MAX_VALID_EPOCH_MS ? epochMs : undefined;
}

function fallbackIsoTimestamp(epochMs: number): string {
  const date = new Date(epochMs);
  return Number.isNaN(date.valueOf()) ? '' : date.toISOString();
}

function parseRequestLogValue(value: unknown, fallbackCreatedAtMs: number): RequestLog | null {
  if (!isRecord(value)) return null;
  const model = boundedString(value.model, 256);
  if (!model) return null;
  const createdAt = typeof value.created_at === 'string' && value.created_at.length <= 64
    ? value.created_at
    : fallbackIsoTimestamp(fallbackCreatedAtMs);
  if (!createdAt) return null;
  const status = optionalSafeInteger(value.status);
  return {
    id: optionalString(value.id, 128) ?? undefined,
    request_id: optionalString(value.request_id, 128) ?? undefined,
    route_alias: optionalString(value.route_alias, 256) ?? undefined,
    provider_id: optionalString(value.provider_id, 256) ?? undefined,
    provider_credential_id: optionalString(value.provider_credential_id, 256),
    api_key_id: optionalString(value.api_key_id, 128),
    api_key_name: optionalString(value.api_key_name, 256),
    model,
    client_protocol: optionalString(value.client_protocol, 64) ?? undefined,
    upstream_protocol: optionalString(value.upstream_protocol, 64) ?? undefined,
    status: status !== undefined && status >= 100 && status <= 599 ? status : undefined,
    duration_ms: optionalSafeInteger(value.duration_ms),
    input_tokens: optionalSafeInteger(value.input_tokens),
    output_tokens: optionalSafeInteger(value.output_tokens),
    cost_micro_usd: optionalSafeInteger(value.cost_micro_usd) ?? null,
    error: optionalString(value.error, 8 * 1024),
    created_at: createdAt,
  };
}

function parseRequestLiveRow(value: unknown): RequestLiveRow | null {
  if (!isRecord(value) || value.live !== true) return null;
  const liveId = boundedString(value.live_id, 128);
  const startedAtMs = optionalEpochMs(value.started_at_ms);
  if (!liveId || startedAtMs === undefined) return null;
  const request = parseRequestLogValue(value, startedAtMs);
  if (!request) return null;
  return {
    ...request,
    live: true,
    live_id: liveId,
    started_at_ms: startedAtMs,
    status: undefined,
    duration_ms: undefined,
    error: null,
  };
}

export function parseRequestLiveEvent(eventName: string, data: string): RequestLiveEvent | null {
  try {
    const value: unknown = JSON.parse(data);
    if (!isRecord(value)) return null;
    if (eventName === 'request-snapshot') {
      if (!Array.isArray(value.requests) || value.requests.length > 64 || typeof value.truncated !== 'boolean') return null;
      const requests = value.requests.map(parseRequestLiveRow);
      if (requests.some((request) => request === null)) return null;
      const limit = optionalSafeInteger(value.limit);
      if (limit === undefined || limit < 1 || limit > 64) return null;
      const activeCount = optionalActiveCount(value);
      if (activeCount === null) return null;
      return {
        type: 'snapshot',
        requests: requests as RequestLiveRow[],
        truncated: value.truncated,
        limit,
        active_count: activeCount ?? requests.length,
      };
    }
    if (eventName === 'request-started') {
      const request = parseRequestLiveRow(value);
      const activeCount = optionalActiveCount(value);
      if (activeCount === null) return null;
      return request ? { type: 'started', request, active_count: activeCount } : null;
    }
    if (eventName === 'request-updated') {
      const liveId = boundedString(value.live_id, 128);
      const providerId = optionalString(value.provider_id, 256);
      const latestLogId = optionalString(value.latest_log_id, 128);
      if (!liveId || providerId === undefined || latestLogId === undefined) return null;
      return { type: 'updated', live_id: liveId, provider_id: providerId, latest_log_id: latestLogId };
    }
    if (eventName === 'request-finished') {
      const liveId = boundedString(value.live_id, 128);
      const finishedAtMs = optionalEpochMs(value.finished_at_ms);
      if (!liveId || finishedAtMs === undefined) return null;
      const request = value.request === null ? null : parseRequestLogValue(value.request, finishedAtMs);
      if (value.request !== null && !request) return null;
      const activeCount = optionalActiveCount(value);
      if (activeCount === null) return null;
      return { type: 'finished', live_id: liveId, request, finished_at_ms: finishedAtMs, active_count: activeCount };
    }
    if (eventName === 'request-capacity') {
      const limit = optionalSafeInteger(value.limit);
      if (typeof value.truncated !== 'boolean' || limit === undefined || limit < 1 || limit > 64) return null;
      const activeCount = optionalActiveCount(value);
      if (activeCount === null) return null;
      return { type: 'capacity', truncated: value.truncated, limit, active_count: activeCount };
    }
  } catch {
    // Ignore malformed or unrelated events and keep the stream alive.
  }
  return null;
}
