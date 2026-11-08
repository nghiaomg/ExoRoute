const THEME_STORAGE_KEY = 'exoroute.theme';
const THEME_ATTRIBUTE_IDS = 'light,dark,tokyo-night,catppuccin,linear,cyber-neon,minimal-light';

/**
 * Applies the first-paint theme before styles load. Kept as a plain string so
 * the exact bootstrap source can be exercised and checked against THEME_BY_ID
 * in tests rather than maintaining a second hand-written list in index.html.
 */
export const themeBootstrapSource = `
(() => {
  const themeIds = '${THEME_ATTRIBUTE_IDS}'.split(',');
  const darkIds = new Set(['dark', 'tokyo-night', 'catppuccin', 'linear', 'cyber-neon']);
  let saved = null;
  try { saved = localStorage.getItem('${THEME_STORAGE_KEY}'); } catch {}
  const id = themeIds.includes(saved) ? saved : 'system';
  const scheme = id === 'system'
    ? (window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
    : (darkIds.has(id) ? 'dark' : 'light');
  const palette = id === 'system' ? scheme : id;
  const root = document.documentElement;
  root.dataset.theme = scheme;
  root.dataset.palette = palette;
  root.dataset.themeStyle = palette;
  root.style.colorScheme = scheme;
  const meta = document.querySelector('meta[name="theme-color"]');
  if (meta) meta.setAttribute('content', palette === 'minimal-light' ? '#f8fafc'
    : palette === 'tokyo-night' ? '#1a1b26'
    : palette === 'catppuccin' ? '#1e1e2e'
    : palette === 'linear' ? '#111111'
    : palette === 'cyber-neon' ? '#0a0a0f'
    : scheme === 'dark' ? '#11121b' : '#f6f7fb');
  try {
    const savedLocale = localStorage.getItem('exoroute.locale');
    if (savedLocale) {
      root.lang = savedLocale;
      if (savedLocale === 'ar') root.dir = 'rtl';
    }
  } catch {}
})();`.trim();

export function themeBootstrapCatalogIds(): string[] {
  return THEME_ATTRIBUTE_IDS.split(',');
}
