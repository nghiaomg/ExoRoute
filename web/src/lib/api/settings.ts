import type { GatewayResourceLimitValues, GatewayResourceLimits, OperationalSettingsSnapshot, OperationalSettingsValues, OutputStyleSelection, OutputStylesSnapshot, PublicSettings, UpdateCheckResult } from '../types';
import { ApiError, announceMustChangePassword, authorizedRequest, request, readErrorPayload, type AdminAccessResult } from './core';
import { getStoredLocale, t } from '../i18n';

export const settingsApi = {
settings: () => request<PublicSettings>('/settings'),
updateCheck: () => request<UpdateCheckResult>('/settings/update-check'),
gatewayResourceLimits: () => request<GatewayResourceLimits>('/settings/resource-limits'),
updateGatewayResourceLimits: (limits: GatewayResourceLimitValues, expectedSaved: GatewayResourceLimitValues, confirmUnlimitedProviderConcurrency = false) => request<GatewayResourceLimits>('/settings/resource-limits', {
  method: 'PUT',
  body: JSON.stringify({
    ...limits,
    expected_saved: expectedSaved,
    confirm_unlimited_provider_concurrency: confirmUnlimitedProviderConcurrency,
  }),
}),
operationalSettings: () => request<OperationalSettingsSnapshot>('/settings/operational'),
updateOperationalSettings: (settings: OperationalSettingsValues, expectedRevision: number, confirmUnlimited: boolean) => request<OperationalSettingsSnapshot>('/settings/operational', {
  method: 'PUT',
  body: JSON.stringify({ ...settings, expected_revision: expectedRevision, confirm_unlimited: confirmUnlimited }),
}),
resetOperationalSettings: (expectedRevision: number) => request<OperationalSettingsSnapshot>('/settings/operational/reset', {
  method: 'POST',
  body: JSON.stringify({ expected_revision: expectedRevision }),
}),
outputStyles: () => request<OutputStylesSnapshot>('/settings/output-styles'),
updateOutputStyles: (styles: OutputStyleSelection[], expectedRevision: number) => request<OutputStylesSnapshot>('/settings/output-styles', {
  method: 'PUT',
  body: JSON.stringify({ styles, expected_revision: expectedRevision }),
}),
resetOutputStyles: (expectedRevision: number) => request<OutputStylesSnapshot>('/settings/output-styles/reset', {
  method: 'POST',
  body: JSON.stringify({ expected_revision: expectedRevision }),
}),
changeAdminPassword: (currentPassword: string, newPassword: string) => request<{ ok: boolean } & AdminAccessResult>('/settings/admin-password', {
  method: 'PUT',
  body: JSON.stringify({ current_password: currentPassword, new_password: newPassword }),
}),
exportDatabase: async (includeRequestLogs: boolean, stepUpToken: string): Promise<{ blob: Blob; filename: string }> => {
  const headers = new Headers({
    Accept: 'application/vnd.exoroute.lmdb-backup, application/octet-stream, application/json',
    'x-exoroute-step-up': stepUpToken,
  });
  let response: Response;
  try {
    const query = new URLSearchParams({ include_request_logs: String(includeRequestLogs) });
    response = await authorizedRequest(`/database/export?${query.toString()}`, { headers });
  } catch (error) {
    if (error instanceof ApiError) throw error;
    throw new ApiError(t(getStoredLocale(), 'Could not reach the ExoRoute API. Check that the gateway is running and try again.'), 0);
  }

  if (!response.ok) {
    const payload = await readErrorPayload(response);
    announceMustChangePassword('/database/export', payload);
    if (response.status === 401 || response.status === 403) {
      throw new ApiError(
        payload.mustChangePassword
          ? t(getStoredLocale(), 'Your current admin password is a default or weak value. Choose a new password now to unlock ExoRoute.')
          : payload.detail || t(getStoredLocale(), 'Your admin session has expired. Please sign in again.'),
        response.status,
        payload.mustChangePassword,
        false,
        payload.code,
      );
    }
    throw new ApiError(payload.detail || t(getStoredLocale(), 'Could not create database backup.'), response.status, false, false, payload.code);
  }

  const disposition = response.headers.get('Content-Disposition') ?? '';
  const encodedName = disposition.match(/filename\*=UTF-8''([^;]+)/i)?.[1];
  const plainName = disposition.match(/filename="?([^";]+)"?/i)?.[1];
  let filename = 'exoroute-backup.exoroute';
  try {
    if (encodedName) filename = decodeURIComponent(encodedName.trim());
    else if (plainName) filename = plainName.trim();
  } catch {
    if (plainName) filename = plainName.trim();
  }
  filename = filename.replace(/^.*[\\/]/, '') || 'exoroute-backup.exoroute';
  return { blob: await response.blob(), filename };
},
deleteRequestLogs: () => request<{ deleted_count: number }>('/request-logs', { method: 'DELETE' }),
importDatabase: (file: File, stepUpToken: string, confirmUnlimited = false) => {
  if (file.size === 0) {
    return Promise.reject(new ApiError(t(getStoredLocale(), 'The database backup is empty.'), 400));
  }
  return request<{ ok: boolean; authentication_reset: boolean }>('/database/import', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/vnd.exoroute.lmdb-backup',
      'x-exoroute-step-up': stepUpToken,
      'x-exoroute-ack-unlimited-concurrency': String(confirmUnlimited),
    },
    body: file,
  });
},
};


