import type { Collection, Provider, ProviderApiKeyAuthStartResult, ProviderApiKeyAuthStatus, ProviderAuthCompletionResult, ProviderAuthPollResult, ProviderAuthStartResult, ProviderAuthStatus, ProviderCreateResult, ProviderCustomHeaderInput, ProviderKey, ProviderKeyCreateResult, ProviderKeyPage, ProviderKeyStrategyInput, ProviderKeyStrategyResult, ProviderModelCatalog, ProviderModelImportResult, ProviderModelRoutingPage, ProviderPreset, ProviderThinkingSettingsInput, ProviderThinkingSettingsResult, ProviderUpdateResult, UpstreamProtocol } from '../types';
import { collection, request } from './core';

export const providerApi = {
providers: async () => collection(await request<Collection<Provider> | Record<string, unknown>>('/providers'), 'providers'),
providerPresets: async () => collection(await request<Collection<ProviderPreset> | Record<string, unknown>>('/provider-presets'), 'presets'),
createProvider: (provider: Omit<Provider, 'id' | 'capabilities' | 'api_key' | 'api_key_count' | 'invalid_api_key_count' | 'model_count' | 'custom_headers'> & { api_key?: string; api_keys?: string[]; custom_headers?: ProviderCustomHeaderInput[] }) => request<ProviderCreateResult>('/providers', { method: 'POST', body: JSON.stringify(provider) }),
updateProvider: (id: string, provider: Partial<Omit<Provider, 'custom_headers'>> & { custom_headers?: ProviderCustomHeaderInput[] }) => request<ProviderUpdateResult>(`/providers/${encodeURIComponent(id)}`, { method: 'PUT', body: JSON.stringify(provider) }),
updateProviderThinkingSettings: (id: string, settings: ProviderThinkingSettingsInput) => request<ProviderThinkingSettingsResult>(`/providers/${encodeURIComponent(id)}/thinking-settings`, { method: 'PUT', body: JSON.stringify(settings) }),
updateProviderKeyStrategy: (id: string, settings: ProviderKeyStrategyInput) => request<ProviderKeyStrategyResult>(`/providers/${encodeURIComponent(id)}/key-strategy`, { method: 'PUT', body: JSON.stringify(settings) }),
deleteProvider: (id: string) => request<void>(`/providers/${encodeURIComponent(id)}`, { method: 'DELETE' }),
providerKeys: (id: string, cursor?: string, limit?: number) => {
  const params = new URLSearchParams();
  if (cursor) params.set('cursor', cursor);
  if (limit) params.set('limit', String(limit));
  const query = params.toString();
  return request<ProviderKeyPage>(`/providers/${encodeURIComponent(id)}/keys${query ? `?${query}` : ''}`);
},
startProviderAuth: (id: string, callbackUrl?: string) => request<ProviderAuthStartResult>(`/providers/${encodeURIComponent(id)}/auth/start`, {
  method: 'POST',
  ...(callbackUrl ? { body: JSON.stringify({ callback_url: callbackUrl }) } : {}),
}),
providerAuthStatus: (id: string, flowId: string) => request<ProviderAuthStatus>(`/providers/${encodeURIComponent(id)}/auth/${encodeURIComponent(flowId)}`),
pollProviderAuth: (id: string, flowId: string) => request<ProviderAuthPollResult>(`/providers/${encodeURIComponent(id)}/auth/${encodeURIComponent(flowId)}/poll`, { method: 'POST' }),
completeProviderAuthCallback: (callbackUrl: string) => request<ProviderAuthCompletionResult>('/providers/auth/callback', { method: 'POST', body: JSON.stringify({ callback_url: callbackUrl }) }),
completeProviderAuthCallbackForFlow: (id: string, flowId: string, callbackUrl: string) => request<ProviderAuthCompletionResult>(`/providers/${encodeURIComponent(id)}/auth/${encodeURIComponent(flowId)}/complete`, { method: 'POST', body: JSON.stringify({ callback_url: callbackUrl }) }),
startProviderApiKeyAuth: (id: string) => request<ProviderApiKeyAuthStartResult>(`/providers/${encodeURIComponent(id)}/api-key-auth/start`, { method: 'POST' }),
providerApiKeyAuthStatus: (id: string, flowId: string) => request<ProviderApiKeyAuthStatus>(`/providers/${encodeURIComponent(id)}/api-key-auth/${encodeURIComponent(flowId)}`),
applyProviderApiKeyAuth: (id: string, flowId: string) => request<{ status: 'applied'; key?: ProviderKeyCreateResult }>(`/providers/${encodeURIComponent(id)}/api-key-auth/${encodeURIComponent(flowId)}/apply`, { method: 'POST' }),
providerModels: (id: string, options: { q?: string; limit?: number; signal?: AbortSignal } = {}) => {
  const params = new URLSearchParams();
  if (options.limit !== undefined) params.set('limit', String(options.limit));
  if (options.q !== undefined) params.set('q', options.q);
  const query = params.toString();
  const path = `/providers/${encodeURIComponent(id)}/models${query ? `?${query}` : ''}`;
  return request<ProviderModelCatalog>(path, options.signal ? { signal: options.signal } : {});
},
providerModelRouting: (id: string, options: { cursor?: string; limit?: number; signal?: AbortSignal } = {}) => {
  const params = new URLSearchParams({ limit: String(options.limit ?? 50) });
  if (options.cursor) params.set('cursor', options.cursor);
  const path = `/providers/${encodeURIComponent(id)}/models/routing?${params.toString()}`;
  return request<ProviderModelRoutingPage>(path, options.signal ? { signal: options.signal } : {});
},
updateProviderModelRouting: (id: string, model: string, upstreamProtocol: UpstreamProtocol | null) => request<{ ok?: boolean }>(
  `/providers/${encodeURIComponent(id)}/models/routing`,
  { method: 'PUT', body: JSON.stringify({ model, upstream_protocol: upstreamProtocol }) },
),
searchProviderModels: (id: string, options: { q?: string; limit?: number; signal?: AbortSignal } = {}) => {
  const params = new URLSearchParams();
  if (options.limit !== undefined) params.set('limit', String(options.limit));
  if (options.q !== undefined) params.set('q', options.q);
  const query = params.toString();
  const path = `/providers/${encodeURIComponent(id)}/models/search${query ? `?${query}` : ''}`;
  return request<ProviderModelCatalog>(path, options.signal ? { signal: options.signal } : {});
},
importProviderModels: (id: string) => request<ProviderModelImportResult>(`/providers/${encodeURIComponent(id)}/models/import`, { method: 'POST' }),
addManualProviderModel: (id: string, model: string) => request<{ model: string; saved: boolean }>(`/providers/${encodeURIComponent(id)}/models/manual`, {
  method: 'POST',
  body: JSON.stringify({ model }),
}),
deleteProviderModel: (id: string, model: string) => request<{ ok: boolean; model: string }>(`/providers/${encodeURIComponent(id)}/models`, {
  method: 'DELETE',
  body: JSON.stringify({ model }),
}),
createProviderKey: (id: string, name: string | undefined, api_key: string) => request<ProviderKeyCreateResult>(`/providers/${encodeURIComponent(id)}/keys`, { method: 'POST', body: JSON.stringify({ name, api_key }) }),
updateProviderKey: (id: string, keyId: string, input: { name?: string; enabled?: boolean }) => request<{ ok: boolean; id: string; name: string; enabled: boolean }>(`/providers/${encodeURIComponent(id)}/keys/${encodeURIComponent(keyId)}`, { method: 'PUT', body: JSON.stringify(input) }),
swapProviderKeyOrder: (id: string, keyId: string, swapWith: string) => request<{ ok: boolean; id: string; swapped_with: string }>(`/providers/${encodeURIComponent(id)}/keys/${encodeURIComponent(keyId)}/order`, { method: 'PUT', body: JSON.stringify({ swap_with: swapWith }) }),
deleteProviderKey: (id: string, keyId: string) => request<void>(`/providers/${encodeURIComponent(id)}/keys/${encodeURIComponent(keyId)}`, { method: 'DELETE' }),
};


