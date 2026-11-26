import type { RequestLogFilters, RequestLogPage } from '../types';
import { request } from './core';

export const requestApi = {
  requests: (filters: RequestLogFilters = {}, signal?: AbortSignal) => {
    const params = new URLSearchParams({ limit: '50' });
    if (filters.cursor) params.set('cursor', filters.cursor);
    if (filters.api_key_id?.trim()) params.set('api_key_id', filters.api_key_id.trim());
    if (filters.model?.trim()) params.set('model', filters.model.trim());
    if (filters.provider_id?.trim()) params.set('provider_id', filters.provider_id.trim());
    if (filters.status) params.set('status', filters.status);
    return request<RequestLogPage>(`/requests?${params.toString()}`, signal ? { signal } : {});
  },
};
