import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { describe, expect, test, vi } from 'vitest';
import DashboardShell from '../../src/components/DashboardShell.svelte';
import type { Translate } from '../../src/lib/format';
import type { DashboardPage } from '../../src/lib/navigation';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

interface ShellOptions {
  collapsed?: boolean;
  onToggleCollapse?: () => void;
}

function shellProps(options: ShellOptions = {}) {
  return {
    currentPage: 'overview' as DashboardPage,
    title: 'Overview',
    tr,
    preferences: {
      locale: 'en' as const,
      theme: 'light' as const,
      setLocale: () => {},
      toggleTheme: () => {},
    },
    gateway: { state: 'loaded' as const, address: '127.0.0.1:8686' },
    providerCount: 3,
    collapsed: options.collapsed ?? false,
    onToggleCollapse: options.onToggleCollapse ?? (() => {}),
    onNavigate: (_page: DashboardPage) => {},
    onRefresh: () => {},
    onSignOut: async () => {},
  };
}

/** Scopes link queries to the sidebar rail: the mobile bottom nav reuses the same labels in jsdom. */
function sidebarNav(container: HTMLElement) {
  const nav = container.querySelector('.sidebar .primary-nav');
  if (!(nav instanceof HTMLElement)) {
    throw new Error('sidebar primary navigation not rendered');
  }
  return within(nav);
}

const destinations = [
  'Overview',
  'Providers',
  'Combos',
  'Quota',
  'API keys',
  'Requests',
  'Statistics',
  'Chat',
  'Settings',
  'Docs',
];

describe('sidebar collapse', () => {
  test('the toggle asks the app to collapse and reports its action', async () => {
    const onToggleCollapse = vi.fn();
    const { container } = render(DashboardShell, {
      props: shellProps({ onToggleCollapse }),
    });

    expect(container.querySelector('.sidebar')?.classList.contains('collapsed')).toBe(false);
    expect(
      container.querySelector('.app-shell')?.classList.contains('sidebar-collapsed'),
    ).toBe(false);

    await fireEvent.click(screen.getByRole('button', { name: 'Collapse sidebar' }));
    expect(onToggleCollapse).toHaveBeenCalledTimes(1);
  });

  test('a collapsed rail keeps every destination labelled for icon-only mode', () => {
    const { container } = render(DashboardShell, {
      props: shellProps({ collapsed: true }),
    });

    expect(container.querySelector('.sidebar')?.classList.contains('collapsed')).toBe(true);
    expect(
      container.querySelector('.app-shell')?.classList.contains('sidebar-collapsed'),
    ).toBe(true);
    expect(screen.getByRole('button', { name: 'Expand sidebar' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Collapse sidebar' })).toBeNull();

    for (const label of destinations) {
      const link = sidebarNav(container).getByRole('link', { name: label });
      expect(link.getAttribute('aria-label')).toBe(label);
      expect(link.getAttribute('title')).toBe(label);
    }
  });

  test('an expanded shell keeps the rail class off and the same labels', () => {
    const { container } = render(DashboardShell, {
      props: shellProps({ collapsed: false }),
    });

    expect(container.querySelector('.sidebar')?.classList.contains('collapsed')).toBe(false);
    expect(screen.getByRole('button', { name: 'Collapse sidebar' })).toBeTruthy();
    for (const label of destinations) {
      expect(sidebarNav(container).getByRole('link', { name: label }).getAttribute('title')).toBe(label);
      expect(sidebarNav(container).getByRole('link', { name: label }).getAttribute('aria-label')).toBe(label);
    }
  });
});
