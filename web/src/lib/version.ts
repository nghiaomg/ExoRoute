declare const __APP_VERSION__: string;

/**
 * The running dashboard version. vite.config.ts injects it from package.json
 * at build time (node --test runs without that define, hence the fallback), so
 * the version shown in the UI and the version the release check compares
 * against can never drift from the published npm/package version.
 */
export const DASHBOARD_VERSION: string =
  typeof __APP_VERSION__ === 'string' && __APP_VERSION__.length > 0 ? __APP_VERSION__ : '0.0.0-dev';
