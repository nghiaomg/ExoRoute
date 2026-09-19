import type { ProviderKeyUsageResult, ProviderQuotaResult, ProviderUsageResult } from '../types';
import { request } from './core';

export const quotaApi = {
  providerUsage: (id: string, cursor?: string) => request<ProviderUsageResult>(
    `/providers/${encodeURIComponent(id)}/usage${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ''}`,
  ),
  refreshProviderUsage: (id: string, cursor?: string) => request<ProviderUsageResult>(
    `/providers/${encodeURIComponent(id)}/usage${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ''}`,
    { method: 'POST' },
  ),
  providerKeyUsage: (id: string, keyId: string) => request<ProviderKeyUsageResult>(
    `/providers/${encodeURIComponent(id)}/keys/${encodeURIComponent(keyId)}/usage`,
  ),
  refreshProviderKeyUsage: (id: string, keyId: string) => request<ProviderKeyUsageResult>(
    `/providers/${encodeURIComponent(id)}/keys/${encodeURIComponent(keyId)}/usage`,
    { method: 'POST' },
  ),
  updateProviderUsageBudget: (
    id: string,
    keyId: string,
    budget: { five_hour_usd?: number | null; seven_day_usd?: number | null; thirty_day_usd?: number | null },
  ) => request<{
    ok: boolean;
    key_id: string;
    budget_usd: { '5h': number | null; '7d': number | null; '30d': number | null };
  }>(
    `/providers/${encodeURIComponent(id)}/keys/${encodeURIComponent(keyId)}/usage-budget`,
    { method: 'PUT', body: JSON.stringify(budget) },
  ),
  providerQuota: (id: string, signal?: AbortSignal) => request<ProviderQuotaResult>(
    `/providers/${encodeURIComponent(id)}/quota`,
    signal ? { signal } : {},
  ),
  updateProviderQuotaTarget: (id: string, localRpmTarget: number) => request<{
    ok: boolean;
    provider_id: string;
    local_rpm_target: number;
  }>(
    `/providers/${encodeURIComponent(id)}/quota`,
    { method: 'PUT', body: JSON.stringify({ local_rpm_target: localRpmTarget }) },
  ),
};
