import type { RequestLiveConnectionState, RequestLiveEvent, UpstreamLiveSnapshot } from './types';
import { parseRequestLiveEvent } from './requestParsers';

const UPSTREAM_EVENT_PATH = '/overview/upstream-events';
const REQUEST_EVENT_PATH = '/requests/events';
const MAX_UPSTREAM_EVENT_FRAME_CHARS = 8 * 1024;
const MAX_REQUEST_EVENT_FRAME_CHARS = 256 * 1024;
const UPSTREAM_EVENT_READ_CHUNK_BYTES = 4 * 1024;
const UPSTREAM_EVENT_RETRY_INITIAL_MS = 500;
const UPSTREAM_EVENT_RETRY_MAX_MS = 30_000;

export type AdminSseEventHandler = (eventName: string, data: string) => void;
export type AdminSseErrorPayload = { detail: string; code: string; mustChangePassword: boolean };
export type AdminSseClientDependencies = {
  authorizedRequest: (path: string, init?: RequestInit) => Promise<Response>;
  readErrorPayload: (response: Response) => Promise<AdminSseErrorPayload>;
  announceMustChangePassword: (path: string, payload: AdminSseErrorPayload) => void;
  invalidateAdminAccess: (broadcast?: boolean) => void;
};

function dispatchSseFrame(frame: string, onEvent: AdminSseEventHandler): void {
  let eventName = '';
  const dataLines: string[] = [];
  for (const line of frame.split(/\r\n|\r|\n/)) {
    if (!line || line.startsWith(':')) continue;
    const separator = line.indexOf(':');
    const field = separator < 0 ? line : line.slice(0, separator);
    let value = separator < 0 ? '' : line.slice(separator + 1);
    if (value.startsWith(' ')) value = value.slice(1);
    if (field === 'event') eventName = value;
    else if (field === 'data') dataLines.push(value);
  }
  if (dataLines.length > 0) onEvent(eventName, dataLines.join('\n'));
}

function dispatchUpstreamSseEvent(eventName: string, data: string, onSnapshot: (snapshot: UpstreamLiveSnapshot) => void): void {
  if (eventName !== 'upstream-count') return;
  try {
    const payload = JSON.parse(data) as Partial<UpstreamLiveSnapshot>;
    const count = payload.enabled_provider_count;
    if (typeof count === 'number' && Number.isSafeInteger(count) && count >= 0) {
      onSnapshot({ enabled_provider_count: count });
    }
  } catch {
    // Ignore malformed or unrelated events and keep the stream alive.
  }
}

async function readAdminEventStream(
  response: Response,
  onEvent: AdminSseEventHandler,
  signal: AbortSignal,
  maxFrameChars: number,
): Promise<void> {
  if (!response.body) throw new Error('admin event stream has no body');
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const separatorPattern = /\r\n\r\n|\n\n|\r\r/;
  let pending = '';
  let completed = false;

  try {
    while (!signal.aborted) {
      const { done, value } = await reader.read();
      if (done) {
        completed = true;
        pending += decoder.decode();
        break;
      }
      for (let offset = 0; offset < value.byteLength; offset += UPSTREAM_EVENT_READ_CHUNK_BYTES) {
        const chunk = value.subarray(offset, Math.min(value.byteLength, offset + UPSTREAM_EVENT_READ_CHUNK_BYTES));
        pending += decoder.decode(chunk, { stream: true });
        let separator: RegExpExecArray | null;
        while ((separator = separatorPattern.exec(pending)) !== null) {
          const frame = pending.slice(0, separator.index);
          if (frame.length > maxFrameChars) throw new Error('admin event frame exceeds the client limit');
          dispatchSseFrame(frame, onEvent);
          pending = pending.slice(separator.index + separator[0].length);
        }
        if (pending.length > maxFrameChars) throw new Error('admin event frame exceeds the client limit');
      }
    }
  } finally {
    if (!completed) await reader.cancel().catch(() => undefined);
    reader.releaseLock();
  }
}

function waitForAdminEventRetry(delayMs: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal.aborted) {
      resolve();
      return;
    }
    const finish = (): void => {
      clearTimeout(timeout);
      signal.removeEventListener('abort', finish);
      resolve();
    };
    const timeout = setTimeout(finish, delayMs);
    signal.addEventListener('abort', finish, { once: true });
  });
}

export async function streamAdminEvents(
  path: string,
  onEvent: AdminSseEventHandler,
  signal: AbortSignal,
  maxFrameChars: number,
  dependencies: AdminSseClientDependencies,
  onStatus?: (state: RequestLiveConnectionState) => void,
): Promise<void> {
  let retryDelay = UPSTREAM_EVENT_RETRY_INITIAL_MS;
  let connected = false;
  while (!signal.aborted) {
    const attemptStartedAt = Date.now();
    onStatus?.(connected ? 'reconnecting' : 'connecting');
    try {
      const response = await dependencies.authorizedRequest(path, {
        headers: { Accept: 'text/event-stream' },
        signal,
      });
      if (!response.ok) {
        const payload = await dependencies.readErrorPayload(response);
        dependencies.announceMustChangePassword(path, payload);
        if (response.status === 401) {
          dependencies.invalidateAdminAccess(true);
          onStatus?.('unavailable');
          return;
        }
        if (response.status === 403 || (response.status >= 400 && response.status < 500
          && response.status !== 408 && response.status !== 429)) {
          onStatus?.('unavailable');
          return;
        }
        await response.body?.cancel().catch(() => undefined);
      } else {
        const contentType = response.headers.get('content-type') ?? '';
        if (!contentType.toLowerCase().includes('text/event-stream')) {
          await response.body?.cancel().catch(() => undefined);
          throw new Error('admin endpoint did not return an event stream');
        }
        connected = true;
        onStatus?.('connected');
        await readAdminEventStream(response, onEvent, signal, maxFrameChars);
      }
    } catch {
      if (signal.aborted) return;
    }

    if (signal.aborted) return;
    if (Date.now() - attemptStartedAt >= 30_000) retryDelay = UPSTREAM_EVENT_RETRY_INITIAL_MS;
    onStatus?.('reconnecting');
    await waitForAdminEventRetry(retryDelay, signal);
    retryDelay = Math.min(UPSTREAM_EVENT_RETRY_MAX_MS, retryDelay * 2);
  }
}

export async function streamUpstreamLiveEvents(
  onSnapshot: (snapshot: UpstreamLiveSnapshot) => void,
  signal: AbortSignal,
  dependencies: AdminSseClientDependencies,
): Promise<void> {
  await streamAdminEvents(
    UPSTREAM_EVENT_PATH,
    (eventName, data) => dispatchUpstreamSseEvent(eventName, data, onSnapshot),
    signal,
    MAX_UPSTREAM_EVENT_FRAME_CHARS,
    dependencies,
  );
}

export async function streamRequestLiveEvents(
  onEvent: (event: RequestLiveEvent) => void,
  signal: AbortSignal,
  dependencies: AdminSseClientDependencies,
  onStatus?: (state: RequestLiveConnectionState) => void,
): Promise<void> {
  await streamAdminEvents(
    REQUEST_EVENT_PATH,
    (eventName, data) => {
      const event = parseRequestLiveEvent(eventName, data);
      if (event) onEvent(event);
    },
    signal,
    MAX_REQUEST_EVENT_FRAME_CHARS,
    dependencies,
    onStatus,
  );
}
