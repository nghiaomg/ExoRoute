import type {
  GatewayApiKey,
  GatewayApiKeyPage,
  GatewayApiKeyScopeInput,
  GatewayApiKeyStatistics,
  RequestStatistics,
} from '../types';
import { request } from './core';

export const apiKeyApi = {
  apiKeys: (query: { cursor?: string | null; q?: string } = {}) => {
    const params = new URLSearchParams({ limit: '50' });
    if (query.cursor) params.set('cursor', query.cursor);
    if (query.q?.trim()) params.set('q', query.q.trim());
    return request<GatewayApiKeyPage>(`/api-keys?${params.toString()}`);
  },
  apiKeyStatistics: (id: string, range: RequestStatistics['range'], signal?: AbortSignal) =>
    request<GatewayApiKeyStatistics>(
      `/api-keys/${encodeURIComponent(id)}/statistics?range=${range}`,
      signal ? { signal } : {},
    ),
  createApiKey: (name: string, scope?: GatewayApiKeyScopeInput) =>
    request<GatewayApiKey & { key: string; warning: string }>('/api-keys', {
      method: 'POST',
      body: JSON.stringify(scope ? { name, ...scope } : { name }),
    }),
  /** Both scope lists are sent together; the backend rejects a partial update. */
  updateApiKey: (
    id: string,
    input: { name?: string; enabled?: boolean } & Partial<GatewayApiKeyScopeInput>,
  ) =>
    request<GatewayApiKey>(`/api-keys/${encodeURIComponent(id)}`, {
      method: 'PUT',
      body: JSON.stringify(input),
    }),
  deleteApiKey: (id: string) => request<void>(`/api-keys/${encodeURIComponent(id)}`, { method: 'DELETE' }),
};
