import type { Collection, GatewayRoute } from '../types';
import { collection, request } from './core';

export const routeApi = {
  routes: async () => collection(await request<Collection<GatewayRoute> | Record<string, unknown>>('/routes'), 'routes'),
  createRoute: (route: Omit<GatewayRoute, 'id'>) =>
    request<GatewayRoute>('/routes', { method: 'POST', body: JSON.stringify(route) }),
  updateRoute: (id: string, route: Partial<GatewayRoute>) =>
    request<GatewayRoute>(`/routes/${encodeURIComponent(id)}`, {
      method: 'PUT',
      body: JSON.stringify(route),
    }),
  deleteRoute: (id: string) => request<void>(`/routes/${encodeURIComponent(id)}`, { method: 'DELETE' }),
};
