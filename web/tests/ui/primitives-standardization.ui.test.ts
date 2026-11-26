import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DocsLayout from '../../src/features/docs/DocsLayout.svelte';
import OverviewRequests from '../../src/features/overview/OverviewRequests.svelte';
import OverviewOpsBar from '../../src/features/overview/OverviewOpsBar.svelte';
import ApiKeyBreakdown from '../../src/features/api-keys/ApiKeyBreakdown.svelte';
import ApiKeyDetailPage from '../../src/features/api-keys/ApiKeyDetailPage.svelte';
import type { Translate } from '../../src/lib/format';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const sourcePath = (relative: string) => resolve(process.cwd(), relative.replace(/^web\//, ''));

afterEach(() => vi.restoreAllMocks());

describe('shared UI primitive standardization', () => {
  test('DocsLayout search input and theme select use shared primitives', async () => {
    const preferences = {
      locale: 'en' as const,
      theme: 'light' as const,
      themePreference: 'light' as const,
      scheme: 'light' as const,
      setLocale: vi.fn(),
      setTheme: vi.fn(),
      setThemePreference: vi.fn(),
    };
    render(DocsLayout, {
      props: {
        page: 'docs-quickstart',
        locale: 'en',
        preferences,
        onNavigate: vi.fn(),
        onExitDocs: vi.fn(),
      },
    });
    const search = screen.getByPlaceholderText('Search documentation…') as HTMLInputElement;
    await fireEvent.input(search, { target: { value: 'theme' } });
    expect(search.value).toBe('theme');

    const themeSelect = screen.getByRole('combobox', { name: 'All themes' }) as HTMLSelectElement;
    await fireEvent.change(themeSelect, { target: { value: 'system' } });
    await waitFor(() => expect(preferences.setThemePreference).toHaveBeenCalledWith('system'));
  });

  test('OverviewRequests search uses the shared Input primitive', async () => {
    render(OverviewRequests, {
      props: {
        tr,
        locale: 'en',
        requests: [],
        onNavigate: vi.fn(),
      },
    });
    const input = screen.getByPlaceholderText('Filter by model') as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'gpt' } });
    expect(input.value).toBe('gpt');
  });

  test('OverviewOpsBar checkbox uses the shared Checkbox primitive', async () => {
    const onLiveChange = vi.fn();
    render(OverviewOpsBar, {
      props: {
        tr,
        locale: 'en',
        endpoint: '127.0.0.1:8686',
        range: '24H',
        live: false,
        loading: false,
        updatedAt: null,
        onRangeChange: vi.fn(),
        onRefresh: vi.fn(),
        onLiveChange,
      },
    });
    const toggle = screen.getByRole('switch', { name: 'Auto-refresh' });
    expect(toggle).toBeTruthy();
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Auto-refresh' }));
    await waitFor(() => expect(onLiveChange).toHaveBeenCalledWith(true));
  });

  test('ApiKeyBreakdown time window uses the shared SelectOptions primitive', async () => {
    const onRangeChange = vi.fn();
    render(ApiKeyBreakdown, {
      props: {
        tr,
        locale: 'en',
        keys: [],
        range: '1d',
        onRangeChange,
        onInspect: vi.fn(),
      },
    });
    const select = screen.getByRole('combobox', { name: 'Time window' }) as HTMLSelectElement;
    await fireEvent.change(select, { target: { value: '7d' } });
    await waitFor(() => expect(onRangeChange).toHaveBeenCalledWith('7d'));
  });

  test('ApiKeyDetailPage renders shared empty-state primitives with alert semantics', async () => {
    render(ApiKeyDetailPage, {
      props: {
        tr,
        locale: 'en',
        keyId: 'key-1',
        loading: false,
        error: 'Could not load key',
        onBack: vi.fn(),
        onEdit: vi.fn(),
        onToggle: vi.fn(),
        onNavigate: vi.fn(),
        onRetry: vi.fn(),
      },
    });
    expect(await screen.findByRole('alert')).toBeTruthy();
    expect(screen.getByText('Could not load key')).toBeTruthy();
  });

  test('owned files contain no raw controls or inline modal footer margins', () => {
    const owned = [
      'web/src/features/docs/DocsLayout.svelte',
      'web/src/features/overview/OverviewRequests.svelte',
      'web/src/features/overview/OverviewOpsBar.svelte',
      'web/src/features/providers/ProviderDetailHeader.svelte',
      'web/src/features/providers/ProviderConfigDialog.svelte',
      'web/src/features/providers/CodexAccountsPanel.svelte',
      'web/src/features/providers/ProviderKeysCard.svelte',
      'web/src/features/api-keys/ApiKeyBreakdown.svelte',
      'web/src/features/api-keys/ApiKeyDetailPage.svelte',
      'web/src/features/proxies/DashboardProxiesPage.svelte',
    ];
    for (const relative of owned) {
      const text = readFileSync(sourcePath(relative), 'utf8');
      expect(text).not.toMatch(/<input\s+type="checkbox"/);
      expect(text).not.toMatch(/<select\s/);
      expect(text).not.toContain('margin-top: 16px');
    }
  });
});
