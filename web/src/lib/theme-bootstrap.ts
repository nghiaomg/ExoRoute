const THEME_ATTRIBUTE_IDS = 'light,dark,tokyo-night,catppuccin,linear,cyber-neon,minimal-light';

/**
 * Theme IDs used by the first-paint bootstrap in `index.html`. The inline
 * script is the single source for first-paint behavior; this shared catalog
 * lets tests verify it stays in lockstep with the runtime theme catalog in
 * `theme.ts` without keeping a second copy of the script in TypeScript.
 */
export function themeBootstrapCatalogIds(): string[] {
  return THEME_ATTRIBUTE_IDS.split(',');
}
