import type { Collection } from '../types';
import { getStoredLocale, t } from '../i18n';
import { streamRequestLiveEvents, streamUpstreamLiveEvents } from '../sseClient';

const API_BASE = '/api/v1/admin';
const ADMIN_SESSION_STORAGE = 'exoroute.admin-session';
const LEGACY_ADMIN_KEY_STORAGE = 'exoroute.admin-key';
const REFRESH_LOCK_NAME = 'exoroute-admin-refresh';
const REFRESH_LOCK_STORAGE = 'exoroute.admin-refresh-lock';
const ACCESS_REFRESH_SKEW_MS = 30_000;

type ErrorPayload = { detail: string; code: string; mustChangePassword: boolean };
type RefreshOutcome =
  | { kind: 'ok'; mustChangePassword: boolean }
  | { kind: 'invalid' }
  | { kind: 'transient'; error: ApiError };

let adminAccessToken = '';
let adminAccessExpiresAt = 0;
let refreshFlight: Promise<RefreshOutcome> | null = null;
let clientAuthStateInitialized = false;

function removeLegacyAdminTokens(): void {
  if (typeof window === 'undefined') return;
  try {
    sessionStorage.removeItem(ADMIN_SESSION_STORAGE);
    sessionStorage.removeItem(LEGACY_ADMIN_KEY_STORAGE);
  } catch {
    // Storage may be unavailable in private browsing.
  }
}

function broadcastAuthInvalidation(): void {
  if (typeof window === 'undefined') return;
  try {
    if ('BroadcastChannel' in window) {
      const channel = new BroadcastChannel('exoroute-admin-auth');
      channel.postMessage({ type: 'session-invalidated' });
      channel.close();
    }
  } catch {
    // Local authentication state is still cleared if cross-tab messaging is unavailable.
  }
}

function invalidateAdminAccess(broadcast = false): void {
  adminAccessToken = '';
  adminAccessExpiresAt = 0;
  if (broadcast) broadcastAuthInvalidation();
  if (typeof window !== 'undefined') window.dispatchEvent(new Event('exoroute:auth-required'));
}

function initializeClientAuthState(): void {
  if (clientAuthStateInitialized || typeof window === 'undefined') return;
  clientAuthStateInitialized = true;
  removeLegacyAdminTokens();
  if (!('BroadcastChannel' in window)) return;
  try {
    const authChannel = new BroadcastChannel('exoroute-admin-auth');
    authChannel.onmessage = (event: MessageEvent<unknown>): void => {
      if (typeof event.data === 'object' && event.data !== null && 'type' in event.data && event.data.type === 'session-invalidated') {
        invalidateAdminAccess();
      }
    };
  } catch {
    // BroadcastChannel is an optional cross-tab enhancement.
  }
}

export function announceMustChangePassword(path: string, payload: ErrorPayload): void {
  if (typeof window !== 'undefined' && path !== '/auth/login' && payload.mustChangePassword) {
    window.dispatchEvent(new Event('exoroute:password-change-required'));
  }
}

export class ApiError extends Error {
  status: number;
  code: string;
  mustChangePassword: boolean;
  invalidCurrentPassword: boolean;
  retryAfterSeconds: number;

  constructor(message: string, status: number, mustChangePassword = false, invalidCurrentPassword = false, code = '', retryAfterSeconds = 0) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
    this.mustChangePassword = mustChangePassword;
    this.invalidCurrentPassword = invalidCurrentPassword;
    this.retryAfterSeconds = retryAfterSeconds;
  }
}

function parseRetryAfterSeconds(value: string | null): number {
  if (!value) return 0;
  const numericValue = Number(value);
  if (Number.isFinite(numericValue) && numericValue >= 0) {
    return Math.min(3600, Math.ceil(numericValue));
  }
  const retryAt = Date.parse(value);
  if (!Number.isFinite(retryAt)) return 0;
  return Math.min(3600, Math.max(0, Math.ceil((retryAt - Date.now()) / 1000)));
}

export async function readErrorPayload(response: Response): Promise<ErrorPayload> {
  try {
    const payload = await readBoundedJson<{ message?: string; must_change_password?: boolean; error?: string | { code?: string; message?: string; must_change_password?: boolean } }>(response, 64 * 1024);
    return {
      detail: payload.message ?? (typeof payload.error === 'string' ? payload.error : payload.error?.message ?? ''),
      code: typeof payload.error === 'object' && payload.error !== null ? payload.error.code ?? '' : '',
      mustChangePassword: payload.must_change_password === true
        || (typeof payload.error === 'object' && payload.error !== null && payload.error.must_change_password === true),
    };
  } catch {
    // The response may be empty or non-JSON.
    return { detail: '', code: '', mustChangePassword: false };
  }
}

async function readBoundedJson<T>(response: Response, maxBytes: number): Promise<T> {
  if (!response.body) throw new Error('response body is empty');
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let totalBytes = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      if (value.byteLength > maxBytes - totalBytes) {
        await reader.cancel();
        throw new Error('response body exceeds the authentication response limit');
      }
      chunks.push(value);
      totalBytes += value.byteLength;
    }
  } finally {
    reader.releaseLock();
  }
  const bytes = new Uint8Array(totalBytes);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes)) as T;
}

export interface AdminAccessResult {
  access_token: string;
  access_expires_in_seconds: number;
  must_change_password: boolean;
}

export interface AdminLoginResult extends AdminAccessResult {
  authenticated: boolean;
}

export function getAdminAccessToken(): string {
  initializeClientAuthState();
  return adminAccessToken;
}

export function setAdminAccessToken(value: string, expiresInSeconds = 600): void {
  initializeClientAuthState();
  adminAccessToken = value.trim();
  adminAccessExpiresAt = adminAccessToken
    ? Date.now() + Math.max(1, Math.min(600, expiresInSeconds)) * 1000
    : 0;
  removeLegacyAdminTokens();
}

export function notifyAdminLogout(): void {
  invalidateAdminAccess(true);
}

async function fetchApi(path: string, init: RequestInit): Promise<Response> {
  const headers = new Headers(init.headers);
  if (!headers.has('Accept')) headers.set('Accept', 'application/json');
  if (init.body && typeof init.body === 'string' && !headers.has('Content-Type')) headers.set('Content-Type', 'application/json');
  const token = getAdminAccessToken();
  if (token) headers.set('Authorization', `Bearer ${token}`);
  else headers.delete('Authorization');
  try {
    return await fetch(`${API_BASE}${path}`, { ...init, headers, credentials: 'same-origin', cache: 'no-store' });
  } catch {
    throw new ApiError(t(getStoredLocale(), 'Could not reach the ExoRoute API. Check that the gateway is running and try again.'), 0);
  }
}

async function withCrossTabRefreshLock<T>(operation: () => Promise<T>): Promise<T> {
  if (typeof navigator === 'undefined') return operation();
  const lockManager = (navigator as Navigator & { locks?: { request: <R>(name: string, callback: () => Promise<R>) => Promise<R> } }).locks;
  if (lockManager) return lockManager.request(REFRESH_LOCK_NAME, operation);

  const owner = `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  const deadline = Date.now() + 25_000;
  while (Date.now() < deadline) {
    try {
      const current = localStorage.getItem(REFRESH_LOCK_STORAGE);
      const expiresAt = current ? Number(current.slice(current.lastIndexOf(':') + 1)) : 0;
      if (!current || !Number.isFinite(expiresAt) || expiresAt <= Date.now()) {
        const lease = `${owner}:${Date.now() + 20_000}`;
        localStorage.setItem(REFRESH_LOCK_STORAGE, lease);
        if (localStorage.getItem(REFRESH_LOCK_STORAGE) === lease) {
          try {
            return await operation();
          } finally {
            if (localStorage.getItem(REFRESH_LOCK_STORAGE) === lease) localStorage.removeItem(REFRESH_LOCK_STORAGE);
          }
        }
      }
    } catch {
      return operation();
    }
    await new Promise((resolve) => window.setTimeout(resolve, 60 + Math.floor(Math.random() * 80)));
  }
  throw new ApiError(t(getStoredLocale(), 'Could not reach the ExoRoute API. Check that the gateway is running and try again.'), 0);
}

async function refreshUnlocked(): Promise<RefreshOutcome> {
  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), 15_000);
  try {
    const response = await fetch(`${API_BASE}/auth/refresh`, {
      method: 'POST',
      headers: { Accept: 'application/json' },
      credentials: 'same-origin',
      cache: 'no-store',
      redirect: 'error',
      signal: controller.signal,
    });
    if (!response.ok) {
      const payload = await readErrorPayload(response);
      if (response.status === 401 && payload.code === 'admin_refresh_invalid') {
        invalidateAdminAccess(true);
        return { kind: 'invalid' };
      }
      const retryAfterSeconds = response.status === 429
        ? parseRetryAfterSeconds(response.headers.get('Retry-After')) || 5
        : 0;
      const errorMessage = response.status === 429
        ? t(getStoredLocale(), 'Too many requests. Try again in {seconds} seconds.', { seconds: retryAfterSeconds })
        : payload.detail || t(getStoredLocale(), response.status === 503
          ? 'The ExoRoute API is busy. Try again shortly.'
          : 'Could not verify the existing admin session. Try again.');
      return {
        kind: 'transient',
        error: new ApiError(errorMessage, response.status, false, false, payload.code, retryAfterSeconds),
      };
    }

    const result = await readBoundedJson<AdminLoginResult>(response, 8 * 1024);
    if (typeof result.access_token !== 'string'
      || !result.access_token
      || !Number.isInteger(result.access_expires_in_seconds)
      || result.access_expires_in_seconds < 1
      || result.access_expires_in_seconds > 600
      || typeof result.must_change_password !== 'boolean') {
      return { kind: 'transient', error: new ApiError(t(getStoredLocale(), 'Could not verify the existing admin session. Try again.'), 502) };
    }
    setAdminAccessToken(result.access_token, result.access_expires_in_seconds);
    return { kind: 'ok', mustChangePassword: result.must_change_password };
  } catch {
    return { kind: 'transient', error: new ApiError(t(getStoredLocale(), 'Could not reach the ExoRoute API. Check that the gateway is running and try again.'), 0) };
  } finally {
    window.clearTimeout(timeout);
  }
}

function refreshAdminAccessToken(): Promise<RefreshOutcome> {
  if (!refreshFlight) {
    refreshFlight = withCrossTabRefreshLock(refreshUnlocked).catch((error: unknown) => ({
      kind: 'transient' as const,
      error: error instanceof ApiError ? error : new ApiError(t(getStoredLocale(), 'Could not reach the ExoRoute API. Check that the gateway is running and try again.'), 0),
    })).finally(() => { refreshFlight = null; });
  }
  return refreshFlight;
}

export async function restoreAdminSession(): Promise<{ authenticated: boolean; unavailable: boolean; mustChangePassword: boolean; error?: ApiError }> {
  const result = await refreshAdminAccessToken();
  if (result.kind === 'ok') return { authenticated: true, unavailable: false, mustChangePassword: result.mustChangePassword };
  if (result.kind === 'invalid') return { authenticated: false, unavailable: false, mustChangePassword: false };
  return { authenticated: false, unavailable: true, mustChangePassword: false, error: result.error };
}

export async function authorizedRequest(path: string, init: RequestInit = {}): Promise<Response> {
  if (!path.startsWith('/auth/') && getAdminAccessToken() && adminAccessExpiresAt - Date.now() <= ACCESS_REFRESH_SKEW_MS) {
    const outcome = await refreshAdminAccessToken();
    if (outcome.kind === 'transient' && adminAccessExpiresAt <= Date.now()) throw outcome.error;
  }
  let response = await fetchApi(path, init);
  if (path !== '/auth/login' && path !== '/auth/refresh' && path !== '/auth/logout'
    && response.status === 401
    && (await readErrorPayload(response.clone())).code === 'admin_session_expired') {
    const outcome = await refreshAdminAccessToken();
    if (outcome.kind === 'transient') throw outcome.error;
    if (outcome.kind === 'ok') response = await fetchApi(path, init);
  }
  return response;
}

export const adminSseDependencies = {
  authorizedRequest,
  readErrorPayload,
  announceMustChangePassword,
  invalidateAdminAccess,
};

export async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await authorizedRequest(path, init);

  if (!response.ok) {
    const { detail, code, mustChangePassword } = await readErrorPayload(response);
    announceMustChangePassword(path, { detail, code, mustChangePassword });
    if (response.status === 401 || response.status === 403) {
      if (mustChangePassword) {
        throw new ApiError(t(getStoredLocale(), 'Your current admin password is a default or weak value. Choose a new password now to unlock ExoRoute.'), response.status, true);
      }
      if ((path === '/settings/admin-password' && detail.toLowerCase().includes('current password is incorrect')) || code === 'admin_password_invalid') {
        throw new ApiError(t(getStoredLocale(), 'The current admin password is incorrect.'), response.status, false, true, code);
      }
      const serverDetail = response.status === 403 && code !== 'admin_password_change_required' ? detail : '';
      const message = path === '/auth/login'
        ? 'Admin access was denied. Check the password and try again.'
        : 'Your admin session has expired. Please sign in again.';
      throw new ApiError(serverDetail || t(getStoredLocale(), message), response.status, false, false, code);
    }
    if (response.status === 429) {
      const retryAfterSeconds = parseRetryAfterSeconds(response.headers.get('Retry-After')) || 1;
      throw new ApiError(
        t(getStoredLocale(), 'Too many requests. Try again in {seconds} seconds.', { seconds: retryAfterSeconds }),
        response.status,
        false,
        false,
        code,
        retryAfterSeconds,
      );
    }
    throw new ApiError(detail || t(getStoredLocale(), 'Request failed ({status}).', { status: response.status }), response.status, false, false, code);
  }

  if (response.status === 204) return undefined as T;
  return await response.json() as T;
}

export function collection<T>(value: Collection<T> | Record<string, unknown>, resource: string): T[] {
  if (Array.isArray(value)) return value;
  const payload = value as Record<string, unknown>;
  const items = payload.data ?? payload[resource];
  return Array.isArray(items) ? items as T[] : [];
}


