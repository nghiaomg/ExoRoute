export type Theme = 'light' | 'dark';

export function getStoredTheme(): Theme {
  try {
    const saved = localStorage.getItem('exoroute.theme');
    if (saved === 'dark' || saved === 'light') return saved;
    if (typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches) {
      return 'dark';
    }
    return 'light';
  } catch {
    return 'light';
  }
}

export function applyTheme(value: Theme): void {
  if (typeof document !== 'undefined') {
    document.documentElement.dataset.theme = value;
    document.documentElement.style.colorScheme = value;
    const metaThemeColor = document.querySelector('meta[name="theme-color"]');
    if (metaThemeColor) {
      metaThemeColor.setAttribute('content', value === 'dark' ? '#11121b' : '#f6f7fb');
    }
  }
  try {
    localStorage.setItem('exoroute.theme', value);
  } catch {
    // The selected theme remains active for this page if storage is unavailable.
  }
}
