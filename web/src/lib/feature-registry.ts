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
  overview: () => import('../features/overview/DashboardOverviewPage.svelte'),
  providers: () => import('../features/providers/DashboardProvidersPage.svelte'),
  combos: () => import('../features/combos/DashboardCombosPage.svelte'),
  quota: () => import('../features/quota/DashboardQuotaPage.svelte'),
  requests: () => import('../features/requests/DashboardRequestsPage.svelte'),
  'api-keys': () => import('../features/api-keys/DashboardApiKeysPage.svelte'),
  settings: () => import('../features/settings/DashboardSettingsPage.svelte'),
  chat: () => import('../features/chat/DashboardChatPage.svelte'),
  help: () => import('../features/help/DashboardHelpPage.svelte'),
};

/** Lazy loader for one dashboard page's feature bundle. */
export function featureComponentLoader(page: DashboardPage): FeatureLoader {
  return featureLoaders[page];
}
