const SIDEBAR_STORAGE_KEY = 'exoroute.sidebar-collapsed';

/** Reads the stored sidebar preference; the sidebar defaults to expanded. */
export function getStoredSidebarCollapsed(): boolean {
  try {
    if (typeof localStorage !== 'undefined') {
      return localStorage.getItem(SIDEBAR_STORAGE_KEY) === 'true';
    }
  } catch {
    // Storage can be unavailable in private browsing or restricted contexts.
  }
  return false;
}

export function saveSidebarCollapsed(collapsed: boolean): void {
  try {
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(SIDEBAR_STORAGE_KEY, collapsed ? 'true' : 'false');
    }
  } catch {
    // The collapsed layout still applies for this page even if it cannot be persisted.
  }
}
