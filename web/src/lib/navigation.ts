export type Page = 'login' | 'overview' | 'providers' | 'combos' | 'quota' | 'requests' | 'statistics' | 'api-keys' | 'settings';
export type DashboardPage = Exclude<Page, 'login'>;
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
};

export function pageFromPath(pathname: string): Page {
  const normalizedPath = pathname === '/' ? '/overview' : pathname.replace(/\/+$/, '');
  if (normalizedPath === '/routes') return 'combos';
  return (Object.entries(pagePaths).find(([, path]) => path === normalizedPath)?.[0] as Page | undefined) ?? 'overview';
}

export function pageAfterLogin(search: string): DashboardPage {
  const nextPath = new URLSearchParams(search).get('next');
  return (Object.entries(pagePaths).find(([page, path]) => page !== 'login' && path === nextPath)?.[0] as DashboardPage | undefined) ?? 'overview';
}
