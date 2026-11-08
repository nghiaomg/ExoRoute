/**
 * Theme catalog and application.
 *
 * A theme id is persisted in localStorage under `exoroute.theme`. Rendering
 * is driven by two attributes on <html>: `data-theme` holds the base scheme
 * ('light' | 'dark') so every scheme-scoped stylesheet keeps applying, and
 * `data-palette` selects the palette layer that overrides the color tokens
 * (styles/palettes.css). Classic light and dark share their id with their
 * scheme; the other palettes map onto one of the two schemes.
 */

export type ThemeScheme = 'light' | 'dark';

export interface ThemeDefinition {
  id: ThemeId;
  scheme: ThemeScheme;
  /** Proper noun shown in the Appearance panel; never translated. */
  name: string;
  /** Design system style archetype name. */
  styleTag: string;
  /** i18n key of the one-line description shown under the name. */
  descriptionKey: string;
  /** Browser UI color reported through the theme-color meta tag. */
  metaColor: string;
  /** Preview swatches: canvas, surface, accent, text. */
  swatches: readonly [string, string, string, string];
}

export type ThemePreference = ThemeId | 'system';

export type ThemeId =
  | 'light'
  | 'dark'
  | 'tokyo-night'
  | 'catppuccin'
  | 'linear'
  | 'cyber-neon'
  | 'minimal-light';

export const THEMES: readonly ThemeDefinition[] = [
  {
    id: 'light',
    scheme: 'light',
    name: 'Classic Light',
    styleTag: 'Classic',
    descriptionKey: 'The default light dashboard with warm orange accents.',
    metaColor: '#f6f7fb',
    swatches: ['#f6f7fb', '#ffffff', '#f97316', '#222538'],
  },
  {
    id: 'dark',
    scheme: 'dark',
    name: 'Classic Dark',
    styleTag: 'Glassmorphic',
    descriptionKey: 'Glassy dark surfaces with the signature orange accent.',
    metaColor: '#11121b',
    swatches: ['#07080c', '#111320', '#f97316', '#f0f2f8'],
  },
  {
    id: 'tokyo-night',
    scheme: 'dark',
    name: 'Tokyo Night',
    styleTag: 'IDE Terminal',
    descriptionKey: 'Cool Tokyo-night blues for long developer sessions.',
    metaColor: '#1a1b26',
    swatches: ['#1a1b26', '#24283b', '#7aa2f7', '#c0caf5'],
  },
  {
    id: 'catppuccin',
    scheme: 'dark',
    name: 'Catppuccin Mocha',
    styleTag: 'Cozy Pastels',
    descriptionKey: 'Soft dark pastels that stay easy on the eyes.',
    metaColor: '#1e1e2e',
    swatches: ['#1e1e2e', '#313244', '#89b4fa', '#cdd6f4'],
  },
  {
    id: 'linear',
    scheme: 'dark',
    name: 'Linear',
    styleTag: 'Precision SaaS',
    descriptionKey: 'Quiet greys with a single indigo accent.',
    metaColor: '#111111',
    swatches: ['#111111', '#1a1a1c', '#5e6ad2', '#e5e5e5'],
  },
  {
    id: 'cyber-neon',
    scheme: 'dark',
    name: 'Cyber Neon',
    styleTag: 'Cyberpunk HUD',
    descriptionKey: 'High-energy neon glow on deep space black.',
    metaColor: '#0a0a0f',
    swatches: ['#0a0a0f', '#12121a', '#00f5d5', '#e0e0ff'],
  },
  {
    id: 'minimal-light',
    scheme: 'light',
    name: 'Minimal Light',
    styleTag: 'Swiss Minimal',
    descriptionKey: 'A clean light theme for bright rooms and screenshots.',
    metaColor: '#f8fafc',
    swatches: ['#f8fafc', '#ffffff', '#3b82f6', '#0f172a'],
  },
];

const THEME_BY_ID = new Map(THEMES.map((theme) => [theme.id, theme]));

export function themeById(id: ThemeId): ThemeDefinition {
  const theme = THEME_BY_ID.get(id);
  // Only reachable through a programming error: every caller passes an id
  // from this catalog or getStoredTheme, both of which validate it.
  if (!theme) throw new Error(`Unknown theme id: ${id}`);
  return theme;
}

export function themeScheme(id: ThemeId): ThemeScheme {
  return themeById(id).scheme;
}

export const THEME_STORAGE_KEY = 'exoroute.theme';

export function isThemeId(value: string | null): value is ThemeId {
  return value !== null && THEME_BY_ID.has(value as ThemeId);
}

export function isThemePreference(value: string | null): value is ThemePreference {
  return value === 'system' || isThemeId(value);
}

function systemThemeId(systemScheme: ThemeScheme): ThemeId {
  return systemScheme === 'dark' ? 'dark' : 'light';
}

export function currentSystemScheme(): ThemeScheme {
  return typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches
    ? 'dark'
    : 'light';
}

export function getStoredThemePreference(): ThemePreference {
  try {
    const saved = localStorage.getItem(THEME_STORAGE_KEY);
    if (isThemePreference(saved)) return saved;
  } catch {
    // Storage may be unavailable; use the live system preference instead.
  }
  return 'system';
}

export function resolveTheme(
  preference: ThemePreference,
  systemScheme: ThemeScheme = currentSystemScheme(),
): ThemeId {
  return preference === 'system' ? systemThemeId(systemScheme) : preference;
}

/**
 * Compatibility helper for callers that only need the effective visual theme.
 * Unlike applyTheme(), this never persists the system-derived resolution.
 */
export function getStoredTheme(): ThemeId {
  return resolveTheme(getStoredThemePreference());
}

export function setStoredThemePreference(preference: ThemePreference): void {
  try {
    if (preference === 'system') localStorage.removeItem(THEME_STORAGE_KEY);
    else localStorage.setItem(THEME_STORAGE_KEY, preference);
  } catch {
    // The selected theme still applies for this page if storage is unavailable.
  }
}

export function applyTheme(id: ThemeId): void {
  const theme = themeById(id);
  if (typeof document !== 'undefined') {
    const root = document.documentElement;
    root.dataset.theme = theme.scheme;
    root.dataset.palette = theme.id;
    root.dataset.themeStyle = theme.id;
    root.style.colorScheme = theme.scheme;
    const metaThemeColor = document.querySelector('meta[name="theme-color"]');
    if (metaThemeColor) {
      metaThemeColor.setAttribute('content', theme.metaColor);
    }
  }
}

export function applyThemePreference(preference: ThemePreference, systemScheme?: ThemeScheme): ThemeId {
  const resolved = resolveTheme(preference, systemScheme);
  applyTheme(resolved);
  return resolved;
}
