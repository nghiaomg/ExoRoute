export type DashboardPage = 'overview' | 'providers' | 'combos' | 'proxies' | 'quota' | 'requests' | 'api-keys' | 'api-key-detail' | 'settings' | 'chat' | 'help';
export type DashboardPageRoute = DashboardPage | `api-key-detail:${string}`;
export type DocsPage = 'docs' | 'docs-overview' | 'docs-quickstart' | 'docs-integrations' | 'docs-reference';
export type Page = 'login' | DashboardPage | DocsPage;
export interface FeatureActionRequest { id: number; page: DashboardPage }

export const pagePaths: Record<Page, string> = {
  login: '/login',
  overview: '/overview',
  providers: '/providers',
  combos: '/combos',
  proxies: '/proxies',
  quota: '/quota',
  requests: '/requests',
  'api-keys': '/api-keys',
  'api-key-detail': '/api-keys/:id',
  settings: '/settings',
  chat: '/chat',
  help: '/help',
  docs: '/docs',
  'docs-overview': '/docs/overview',
  'docs-quickstart': '/docs/quickstart',
  'docs-integrations': '/docs/integrations',
  'docs-reference': '/docs/reference',
};

export const docsPages: ReadonlyArray<DocsPage> = [
  'docs',
  'docs-overview',
  'docs-quickstart',
  'docs-integrations',
  'docs-reference',
];

export const dashboardPages: ReadonlyArray<DashboardPage> = [
  'overview',
  'providers',
  'combos',
  'proxies',
  'quota',
  'requests',
  'api-keys',
  'api-key-detail',
  'settings',
  'chat',
  'help',
];

export function isDocsPage(page: Page): page is DocsPage {
  return docsPages.includes(page as DocsPage);
}

export function pageFromPath(pathname: string): Page | DashboardPageRoute {
  const normalizedPath = pathname === '/' ? '/overview' : pathname.replace(/\/+$/, '');
  if (normalizedPath === '/docs/api-reference') return 'docs-reference';
  if (normalizedPath === '/routes') return 'combos';
  if (normalizedPath.startsWith('/api-keys/')) {
    const id = normalizedPath.slice('/api-keys/'.length);
    if (id) return `api-key-detail:${id}`;
  }
  return (Object.entries(pagePaths).find(([, path]) => path === normalizedPath)?.[0] as Page | undefined) ?? 'overview';
}

export function pageAfterLogin(search: string): DashboardPage {
  const nextPath = new URLSearchParams(search).get('next');
  const found = dashboardPages.find((page) => pagePaths[page] === nextPath);
  return found ?? 'overview';
}
