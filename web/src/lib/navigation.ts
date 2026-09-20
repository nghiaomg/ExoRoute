export type DashboardPage = 'overview' | 'providers' | 'combos' | 'quota' | 'requests' | 'statistics' | 'api-keys' | 'settings';
export type DocsPage = 'docs' | 'docs-quickstart' | 'docs-integrations' | 'docs-reference';
export type Page = 'login' | DashboardPage | DocsPage;
export interface FeatureActionRequest { id: number; page: DashboardPage }

export const pagePaths: Record<Page, string> = {
  login: '/login',
  overview: '/overview',
  providers: '/providers',
  combos: '/combos',
  quota: '/quota',
  requests: '/requests',
  statistics: '/statistics',
  'api-keys': '/api-keys',
  settings: '/settings',
  docs: '/docs',
  'docs-quickstart': '/docs/quickstart',
  'docs-integrations': '/docs/integrations',
  'docs-reference': '/docs/reference',
};

export const docsPages: ReadonlyArray<DocsPage> = [
  'docs',
  'docs-quickstart',
  'docs-integrations',
  'docs-reference',
];

export const dashboardPages: ReadonlyArray<DashboardPage> = [
  'overview',
  'providers',
  'combos',
  'quota',
  'requests',
  'statistics',
  'api-keys',
  'settings',
];

export function isDocsPage(page: Page): page is DocsPage {
  return docsPages.includes(page as DocsPage);
}

export function pageFromPath(pathname: string): Page {
  const normalizedPath = pathname === '/' ? '/overview' : pathname.replace(/\/+$/, '');
  if (normalizedPath === '/routes') return 'combos';
  return (Object.entries(pagePaths).find(([, path]) => path === normalizedPath)?.[0] as Page | undefined) ?? 'overview';
}

export function pageAfterLogin(search: string): DashboardPage {
  const nextPath = new URLSearchParams(search).get('next');
  const found = dashboardPages.find((page) => pagePaths[page] === nextPath);
  return found ?? 'overview';
}
