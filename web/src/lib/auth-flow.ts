import { restoreAdminSession, type AdminSessionRestoreResult } from './api';

export type AuthBootstrapState = 'checking' | 'ready' | 'unavailable';
export type SessionCheckStatus = 'checking' | 'rate_limited' | 'unavailable' | null;

/** Outcome of one admin-session restore attempt, reduced to what the app renders. */
export type SessionRestoreOutcome =
  | { kind: 'unavailable'; checkStatus: 'rate_limited' | 'unavailable'; retryAfterSeconds: number }
  | { kind: 'ready'; authenticated: boolean; mustChangePassword: boolean };

const SESSION_RETRY_POLL_MS = 250;
const SESSION_RETRY_MAX_SECONDS = 3600;
const SESSION_RETRY_FALLBACK_SECONDS = 5;

/**
 * Reduces a session restore result to the state the app renders. A transient
 * failure (offline, 429, server error) becomes `unavailable` rather than a
 * logout, so the caller keeps the current page and offers a retry.
 */
export function classifySessionRestore(result: AdminSessionRestoreResult): SessionRestoreOutcome {
  if (!result.unavailable) {
    return {
      kind: 'ready',
      authenticated: result.authenticated,
      mustChangePassword: result.authenticated && result.mustChangePassword,
    };
  }
  const rateLimited = result.error?.status === 429;
  return {
    kind: 'unavailable',
    checkStatus: rateLimited ? 'rate_limited' : 'unavailable',
    retryAfterSeconds: rateLimited
      ? result.error?.retryAfterSeconds || SESSION_RETRY_FALLBACK_SECONDS
      : 0,
  };
}

/** Restores the admin session and classifies the result. */
export async function restoreSessionOnce(): Promise<SessionRestoreOutcome> {
  return classifySessionRestore(await restoreAdminSession());
}

/**
 * Counts down a server-provided retry delay. `onTick` receives the remaining
 * whole seconds, including the initial value and a final zero.
 */
export function createSessionRetryCountdown(onTick: (seconds: number) => void) {
  let timer: number | null = null;
  const stop = (): void => {
    if (timer !== null) window.clearInterval(timer);
    timer = null;
  };
  return {
    stop,
    start(seconds: number): void {
      stop();
      const remaining = Math.max(0, Math.min(SESSION_RETRY_MAX_SECONDS, Math.ceil(seconds)));
      onTick(remaining);
      if (remaining === 0) return;
      const retryAt = Date.now() + remaining * 1000;
      timer = window.setInterval(() => {
        const left = Math.max(0, Math.ceil((retryAt - Date.now()) / 1000));
        onTick(left);
        if (left === 0) stop();
      }, SESSION_RETRY_POLL_MS);
    },
  };
}
