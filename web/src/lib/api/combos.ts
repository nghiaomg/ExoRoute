import type { Collection, ComboProviderOptionPage, GatewayCombo } from '../types';
import { collection, request } from './core';

export const comboApi = {
combos: async () => collection(await request<Collection<GatewayCombo> | Record<string, unknown>>('/combos'), 'combos'),
comboProviderOptions: (cursor?: string, signal?: AbortSignal) => request<ComboProviderOptionPage>(`/combos/provider-options${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ''}`, signal ? { signal } : {}),
createCombo: (combo: Omit<GatewayCombo, 'id'>) => request<{ ok: boolean; id: string; combo_id: string }>('/combos', { method: 'POST', body: JSON.stringify(combo) }),
updateCombo: (id: string, combo: Partial<GatewayCombo>) => request<{ ok: boolean }>(`/combos/${encodeURIComponent(id)}`, { method: 'PUT', body: JSON.stringify(combo) }),
deleteCombo: (id: string) => request<void>(`/combos/${encodeURIComponent(id)}`, { method: 'DELETE' }),
// Legacy aliases remain available for clients that still call /routes.
};



