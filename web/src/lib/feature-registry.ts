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
  overview: () => import('../features/overview/SketchOverviewPage.svelte'),
  providers: () => import('../features/providers/SketchProvidersPage.svelte'),
  combos: () => import('../features/combos/SketchCombosPage.svelte'),
  quota: () => import('../features/quota/SketchQuotaPage.svelte'),
  requests: () => import('../features/requests/SketchRequestsPage.svelte'),
  'api-keys': () => import('../features/api-keys/SketchApiKeysPage.svelte'),
  settings: () => import('../features/settings/SketchSettingsPage.svelte'),
  chat: () => import('../features/chat/SketchChatPage.svelte'),
  help: () => import('../features/help/SketchHelpPage.svelte'),
};

/** Lazy loader for one dashboard page's feature bundle. */
export function featureComponentLoader(page: DashboardPage): FeatureLoader {
  return featureLoaders[page];
}
