import type {
  DevinSession,
  DevinSettingsResult,
  DevinVerifyResult,
} from '../types';
import { request } from './core';

/**
 * Devin cloud-agent endpoints. The Rust admin layer owns the stored API key,
 * validates every field, and never returns the key itself.
 */
export const devinApi = {
  devinSettings: () => request<DevinSettingsResult>('/devin/settings'),
  devinSaveSettings: (apiKey: string) => request<{ configured: boolean }>(
    '/devin/settings',
    { method: 'PUT', body: JSON.stringify({ api_key: apiKey }) },
  ),
  devinDeleteSettings: () => request<{ configured: boolean }>(
    '/devin/settings',
    { method: 'DELETE' },
  ),
  devinVerify: () => request<DevinVerifyResult>('/devin/verify', { method: 'POST' }),
  devinListSessions: (limit: number, offset: number) =>
    request<DevinSession[]>(`/devin/sessions?limit=${limit}&offset=${offset}`),
  devinCreateSession: (payload: { prompt: string; repository?: string; branch?: string }) =>
    request<DevinSession>('/devin/sessions', { method: 'POST', body: JSON.stringify(payload) }),
  devinGetSession: (sessionId: string) =>
    request<DevinSession>(`/devin/sessions/${encodeURIComponent(sessionId)}`),
  devinSendMessage: (sessionId: string, message: string) =>
    request<unknown>(
      `/devin/sessions/${encodeURIComponent(sessionId)}/message`,
      { method: 'POST', body: JSON.stringify({ message }) },
    ),
};
