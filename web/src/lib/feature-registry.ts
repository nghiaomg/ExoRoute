import type { DashboardPage } from './navigation';

/**
 * The resolved component is typed by the caller: a single shared type cannot
 * describe every page's prop contract, and the pages differ by props only.
 */
type FeatureLoader = () => Promise<unknown>;

/**
 * Dashboard routes are code-split so the login and docs bundles never pull in
 * the feature panels. Keep every entry a literal dynamic import so the bundler
 * can still emit one chunk per page.
 */
const featureLoaders: Record<DashboardPage, FeatureLoader> = {
  overview: () => import('../features/overview/OverviewPage.svelte'),
  providers: () => import('../features/providers/ProvidersPage.svelte'),
  combos: () => import('../features/combos/CombosPage.svelte'),
  quota: () => import('../features/quota/QuotaPage.svelte'),
  requests: () => import('../features/requests/RequestsPage.svelte'),
  statistics: () => import('../features/statistics/StatisticsPage.svelte'),
  'api-keys': () => import('../features/api-keys/ApiKeysPage.svelte'),
  settings: () => import('../features/settings/SettingsPage.svelte'),
};

/** Lazy loader for one dashboard page's feature bundle. */
export function featureComponentLoader(page: DashboardPage): FeatureLoader {
  return featureLoaders[page];
}
