import type { AdminLoginResult } from './core';
import { notifyAdminLogout, request } from './core';

export const authApi = {
adminLogin: (password: string) => request<AdminLoginResult>('/auth/login', {
  method: 'POST',
  body: JSON.stringify({ password }),
}),
adminLogout: async () => {
  await request<{ ok: boolean }>('/auth/logout', { method: 'POST' });
  notifyAdminLogout();
},
reauthenticateAdmin: (password: string, scope: 'database_export' | 'database_import') => request<{ step_up_token: string; expires_in_seconds: number }>('/auth/reauth', {
  method: 'POST',
  body: JSON.stringify({ password, scope }),
}),
};



