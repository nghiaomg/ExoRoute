import type { Collection, ProxyEndpoint, ProxyProviderOption, GatewayProxy, ProxyTrafficStatsResponse } from '../types';
import type { ProxyCheckResponse } from '../types/proxies';
import { collection, request } from './core';

export const proxyApi = {
  proxies: async () => collection(await request<Collection<GatewayProxy> | Record<string, unknown>>('/proxies'), 'proxies'),
  trafficStats: () => request<ProxyTrafficStatsResponse>('/proxies/stats'),
  checkProxy: (id: string) =>
    request<ProxyCheckResponse>(`/proxies/${encodeURIComponent(id)}/check`, { method: 'POST' }),
  proxyOptions: async (options: { q?: string; cursor?: string; includeId?: string } = {}) => {
    const query = new URLSearchParams();
    if (options.q) query.set('q', options.q);
    if (options.cursor) query.set('cursor', options.cursor);
    if (options.includeId) query.set('include_id', options.includeId);
    const suffix = query.size ? `?${query.toString()}` : '';
    return request<{ proxies: ProxyProviderOption[]; next_cursor?: string | null }>(`/proxies/options${suffix}`);
  },
  createProxy: (proxy: Omit<GatewayProxy, 'id' | 'created_at' | 'updated_at' | 'provider_count'>) =>
    request<{ ok: boolean; id: string }>('/proxies', { method: 'POST', body: JSON.stringify(proxy) }),
  updateProxy: (id: string, proxy: Omit<GatewayProxy, 'id' | 'created_at' | 'updated_at' | 'provider_count'>) =>
    request<{ ok: boolean }>(`/proxies/${encodeURIComponent(id)}`, { method: 'PUT', body: JSON.stringify(proxy) }),
  deleteProxy: (id: string) => request<void>(`/proxies/${encodeURIComponent(id)}`, { method: 'DELETE' }),
  bulkCreateProxies: (proxies: Array<Omit<GatewayProxy, 'id' | 'created_at' | 'updated_at' | 'provider_count'>>) =>
    request<{ ok: boolean; created: number; ids: string[] }>('/proxies/bulk', { method: 'POST', body: JSON.stringify({ proxies }) }),
};

export type ProxyEndpointDraft = ProxyEndpoint;
