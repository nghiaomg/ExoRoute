import { strict as assert } from 'node:assert/strict';
import test from 'node:test';
import { isDocsPage, pageAfterLogin as resolveAfterLogin, pageFromPath as resolveAppPage } from '../src/lib/navigation';

export type DashboardPage =
  | 'overview'
  | 'providers'
  | 'combos'
  | 'quota'
  | 'requests'
  | 'statistics'
  | 'api-keys'
  | 'settings';

export const pagePaths: Record<DashboardPage | 'login', string> = {
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

const KNOWN_PATHS = new Set(Object.values(pagePaths));

export function pageFromPath(pathname: string): DashboardPage | 'login' | 'overview' {
  const normalizedPath = pathname === '/' ? '/overview' : pathname.replace(/\/+$/, '');
  if (normalizedPath === '/routes') return 'combos';
  const found = (Object.entries(pagePaths) as Array<[DashboardPage | 'login', string]>).find(
    ([, path]) => path === normalizedPath,
  )?.[0];
  return (found as DashboardPage | 'login' | undefined) ?? 'overview';
}

export function pageAfterLogin(search: string): DashboardPage {
  const nextPath = new URLSearchParams(search).get('next');
  const found = (Object.entries(pagePaths) as Array<[DashboardPage | 'login', string]>).find(
    ([page, path]) => page !== 'login' && path === nextPath,
  )?.[0];
  return (found as DashboardPage | undefined) ?? 'overview';
}

export function safeRedirectTarget(next: string | null): DashboardPage | null {
  if (!next || !KNOWN_PATHS.has(next) || next === '/login') return null;
  return next.slice(1) as DashboardPage;
}

test('post-login next target cannot become an external redirect', () => {
  assert.equal(pageAfterLogin('?next=/providers'), 'providers');
  assert.equal(pageAfterLogin('?next=https://evil.example/phish'), 'overview');
  assert.equal(pageAfterLogin('?next=//evil.example/phish'), 'overview');
  assert.equal(pageAfterLogin('?next=/login'), 'overview');
});

test('safe redirect target rejects protocol-relative and external URLs', () => {
  assert.equal(safeRedirectTarget('/providers'), 'providers');
  assert.equal(safeRedirectTarget('https://evil.example/'), null);
  assert.equal(safeRedirectTarget('//evil.example/'), null);
  assert.equal(safeRedirectTarget('/login'), null);
  assert.equal(safeRedirectTarget(null), null);
});

test('legacy /routes path resolves to the combos page', () => {
  assert.equal(pageFromPath('/routes'), 'combos');
  assert.equal(pageFromPath('/'), 'overview');
  assert.equal(pageFromPath('/unknown'), 'overview');
});

test('public documentation routes resolve without becoming admin redirect targets', () => {
  const docsPages = [
    ['/docs', 'docs'],
    ['/docs/quickstart', 'docs-quickstart'],
    ['/docs/integrations/', 'docs-integrations'],
    ['/docs/reference', 'docs-reference'],
  ] as const;

  for (const [path, page] of docsPages) {
    const resolved = resolveAppPage(path);
    assert.equal(resolved, page);
    assert.equal(isDocsPage(resolved), true);
  }

  assert.equal(resolveAfterLogin('?next=/docs'), 'overview');
  assert.equal(resolveAfterLogin('?next=/docs/reference'), 'overview');
});

test('formatGatewayEndpoint displays domain/v1 when window host is present and falls back to host:port/v1', async () => {
  const { formatGatewayEndpoint } = await import('../src/lib/format');

  // When loaded on remote domain (e.g. https://exo.qwencoder.cloud)
  assert.equal(
    formatGatewayEndpoint('127.0.0.1', 8686, 'exo.qwencoder.cloud'),
    'exo.qwencoder.cloud/v1',
  );

  // When loaded on local address with port (e.g. http://127.0.0.1:8686)
  assert.equal(
    formatGatewayEndpoint('127.0.0.1', 8686, '127.0.0.1:8686'),
    '127.0.0.1:8686/v1',
  );

  // When loaded on domain with custom port (e.g. exo.qwencoder.cloud:8443)
  assert.equal(
    formatGatewayEndpoint('127.0.0.1', 8686, 'exo.qwencoder.cloud:8443'),
    'exo.qwencoder.cloud:8443/v1',
  );

  // When window host is missing or empty (fallback to settings host:port)
  assert.equal(
    formatGatewayEndpoint('127.0.0.1', 8686, null),
    '127.0.0.1:8686/v1',
  );
  assert.equal(
    formatGatewayEndpoint('0.0.0.0', 9000, ''),
    '0.0.0.0:9000/v1',
  );
  assert.equal(
    formatGatewayEndpoint(null, null, null),
    '127.0.0.1:8686/v1',
  );
});
