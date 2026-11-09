import { writable } from 'svelte/store';

/**
 * Shared helpers for dashboard features.
 * Toast mirrors window.showToast in the static prototype; the pending request
 * search carries the topbar query into the Requests page.
 */

interface DashboardToast {
  id: number;
  message: string;
}

export const dashboardToast = writable<DashboardToast | null>(null);
let toastSeq = 0;
let toastTimer: ReturnType<typeof setTimeout> | undefined;

export function showDashboardToast(message: string, ms = 2600): void {
  toastSeq += 1;
  dashboardToast.set({ id: toastSeq, message });
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => dashboardToast.set(null), ms);
}

/** Query submitted in the topbar search; consumed by the Requests page. */
export const pendingRequestSearch = writable<string | null>(null);

export function submitTopSearch(query: string): void {
  const q = query.trim();
  if (q) pendingRequestSearch.set(q);
}
