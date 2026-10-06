import type { WorkspaceChatInput } from '../types';
import { adminSseDependencies, ApiError } from './core';
import { getStoredLocale, t } from '../i18n';

/**
 * Streaming chat transport. The dashboard POSTs the same input shape as the
 * one-shot relay and consumes canonical SSE events; nothing is persisted
 * server-side, so every frame only feeds the in-memory conversation.
 *
 * Bounds mirror the relay: each frame is capped, the whole stream is capped,
 * and an idle stream fails instead of hanging. Admin authentication goes
 * through the shared client, so an access token that expires while the
 * dashboard is open is refreshed and a dead session returns to sign-in
 * instead of surfacing as a failed chat turn.
 */

// Relative to the shared admin client's base, which owns the API prefix and
// the authorization header.
const STREAM_PATH = '/workspace/chat/stream';

/** Largest single SSE frame the client accepts, matching the relay's bound. */
const MAX_FRAME_CHARS = 256 * 1024;
/** Largest total stream the client accepts for one turn. */
const MAX_TOTAL_CHARS = 2 * 1024 * 1024;
/** A stream that sends nothing for this long is considered stalled. */
const IDLE_TIMEOUT_MS = 150_000;

export interface WorkspaceChatStreamDone {
  model: string;
  finish_reason: string;
  input_tokens: number | null;
  output_tokens: number | null;
  duration_ms: number;
}

export type WorkspaceChatStreamHandlers = {
  onTextDelta: (delta: string) => void;
  onReasoningDelta?: (delta: string) => void;
  onDone: (done: WorkspaceChatStreamDone) => void;
  onError: (message: string) => void;
};

function localized(message: string, vars?: Record<string, string | number>): string {
  return t(getStoredLocale(), message, vars);
}

/**
 * Sends one chat turn and consumes its canonical event stream. Exactly one of
 * `onDone`/`onError` fires, then the promise resolves. Aborting the signal
 * closes the fetch body and surfaces as `onError` with the localized stop
 * message only when the caller did not already abort intentionally — callers
 * that abort on purpose can check `signal.aborted` themselves.
 */
export async function streamWorkspaceChat(
  input: WorkspaceChatInput,
  handlers: WorkspaceChatStreamHandlers,
  signal?: AbortSignal,
): Promise<void> {
  // authorizedRequest owns the admin auth contract: it refreshes a token that
  // is about to expire, retries once after a session-expired rejection, and
  // reports reachability, auth, and rate-limit failures as localized
  // ApiErrors the page can show as-is.
  const response = await adminSseDependencies.authorizedRequest(STREAM_PATH, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Accept: 'text/event-stream',
    },
    credentials: 'same-origin',
    cache: 'no-store',
    body: JSON.stringify(input),
    ...(signal ? { signal } : {}),
  });

  if (!response.ok) {
    const payload = await adminSseDependencies.readErrorPayload(response);
    adminSseDependencies.announceMustChangePassword(STREAM_PATH, payload);
    if (response.status === 401 || response.status === 403) {
      if (response.status === 401) adminSseDependencies.invalidateAdminAccess(true);
      const detail =
        response.status === 403 && payload.code !== 'admin_password_change_required'
          ? payload.detail
          : '';
      throw new ApiError(
        detail || localized('Your admin session has expired. Please sign in again.'),
        response.status,
        payload.mustChangePassword,
        false,
        payload.code,
      );
    }
    // The relay reports provider and validation failures as a JSON message;
    // keeping it means the operator sees the cause instead of a generic
    // "chat request failed".
    throw new ApiError(
      payload.detail || localized('Request failed ({status}).', { status: response.status }),
      response.status,
      payload.mustChangePassword,
      false,
      payload.code,
    );
  }
  if (!response.body) {
    throw new ApiError(localized('The chat stream ended unexpectedly.'), response.status);
  }

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const frameSeparator = /\r\n\r\n|\n\n|\r\r/;
  let pending = '';
  let totalChars = 0;
  // Timers come from the global scope, matching the other transports, so the
  // idle bound is testable without a window object.
  let idleTimer: ReturnType<typeof setTimeout> | null = null;
  let finished = false;

  const stopIdleTimer = (): void => {
    if (idleTimer !== null) {
      clearTimeout(idleTimer);
      idleTimer = null;
    }
  };
  const armIdleTimer = (): void => {
    stopIdleTimer();
    idleTimer = setTimeout(() => {
      void reader.cancel().catch(() => undefined);
    }, IDLE_TIMEOUT_MS);
  };

  const dispatchFrame = (frame: string): void => {
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
    if (dataLines.length === 0) return;
    const data = dataLines.join('\n');
    if (eventName === 'text-delta') {
      const payload = safeParse(data);
      if (payload && typeof payload.delta === 'string' && payload.delta) {
        handlers.onTextDelta(payload.delta);
      }
      return;
    }
    if (eventName === 'reasoning-delta') {
      const payload = safeParse(data);
      if (payload && typeof payload.delta === 'string' && payload.delta) {
        handlers.onReasoningDelta?.(payload.delta);
      }
      return;
    }
    if (eventName === 'done') {
      const payload = safeParse(data);
      finished = true;
      handlers.onDone({
        model: typeof payload?.model === 'string' ? payload.model : '',
        finish_reason: typeof payload?.finish_reason === 'string' ? payload.finish_reason : '',
        input_tokens: numberOrNull(payload?.input_tokens),
        output_tokens: numberOrNull(payload?.output_tokens),
        duration_ms: typeof payload?.duration_ms === 'number' ? payload.duration_ms : 0,
      });
      return;
    }
    if (eventName === 'error') {
      const payload = safeParse(data);
      finished = true;
      handlers.onError(
        typeof payload?.message === 'string' && payload.message
          ? payload.message
          : localized('The chat stream ended unexpectedly.'),
      );
    }
  };

  const failIdle = (): void => {
    if (finished) return;
    finished = true;
    handlers.onError(localized('The chat stream stalled and was stopped.'));
  };

  armIdleTimer();
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      if (value.byteLength > MAX_TOTAL_CHARS - totalChars) {
        await reader.cancel().catch(() => undefined);
        if (!finished) {
          finished = true;
          handlers.onError(localized('The chat reply exceeded the size limit.'));
        }
        return;
      }
      totalChars += value.byteLength;
      pending += decoder.decode(value, { stream: true });
      if (pending.length > MAX_TOTAL_CHARS) {
        await reader.cancel().catch(() => undefined);
        if (!finished) {
          finished = true;
          handlers.onError(localized('The chat reply exceeded the size limit.'));
        }
        return;
      }
      armIdleTimer();
      // Dispatch every complete frame; keep the trailing partial in `pending`.
      while (true) {
        const match = frameSeparator.exec(pending);
        if (!match || match.index === undefined) break;
        const frame = pending.slice(0, match.index);
        pending = pending.slice(match.index + match[0].length);
        if (frame.length > MAX_FRAME_CHARS) {
          if (!finished) {
            finished = true;
            handlers.onError(localized('The chat reply exceeded the size limit.'));
          }
          await reader.cancel().catch(() => undefined);
          return;
        }
        dispatchFrame(frame);
        if (finished) return;
      }
    }
    // Stream ended: flush any trailing frame the server forgot to terminate.
    if (pending.trim()) dispatchFrame(pending);
    if (!finished) {
      finished = true;
      handlers.onError(localized('The chat stream ended unexpectedly.'));
    }
  } catch (error) {
    if (!finished) {
      finished = true;
      if (signal?.aborted) {
        handlers.onError(localized('Generation stopped.'));
      } else {
        handlers.onError(localized('The chat stream ended unexpectedly.'));
      }
    }
    void error;
  } finally {
    stopIdleTimer();
    try {
      reader.releaseLock();
    } catch {
      // The lock may already be released after cancel.
    }
  }
}

function safeParse(data: string): Record<string, unknown> | null {
  try {
    const parsed = JSON.parse(data) as unknown;
    return parsed && typeof parsed === 'object' ? (parsed as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}

function numberOrNull(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

/** Feature-scoped API surface; spread into the shared dashboard client. */
export const workspaceChatStreamApi = { chatStream: streamWorkspaceChat };
