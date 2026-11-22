import { render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardHelpPage from '../../src/features/help/DashboardHelpPage.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type { GatewayApiKey, Provider } from '../../src/lib/types';
import { statisticsFixture } from './support/statistics';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

function provider(id: string, keyCount: number): Provider {
  return {
    id,
    adapter_id: 'generic',
    name: id,
    base_url: 'http://127.0.0.1:9/v1',
    enabled: true,
    auth_type: 'none',
    api_key_count: keyCount,
    preferred_protocol: 'chat_completions',
    supported_protocols: ['chat_completions'],
  };
}

function apiKey(id: string, enabled: boolean): GatewayApiKey {
  return {
    id,
    name: id,
    enabled,
    created_at: '2026-11-10 17:43:00',
    request_count: 1,
  };
}

function setupApi(providers: Provider[], keys: GatewayApiKey[]): void {
  vi.spyOn(api, 'overview').mockResolvedValue({
    provider_count: providers.length,
    combo_count: 1,
    route_count: 1,
    request_count: 10,
    dropped_request_logs: 0,
    uptime_seconds: 100,
  });
  vi.spyOn(api, 'statistics').mockResolvedValue(statisticsFixture());
  vi.spyOn(api, 'settings').mockResolvedValue({ host: '127.0.0.1', port: 8686 });
  vi.spyOn(api, 'providers').mockResolvedValue(providers);
  vi.spyOn(api, 'apiKeys').mockResolvedValue({ api_keys: keys, next_cursor: null });
}

function renderPage() {
  return render(DashboardHelpPage, {
    props: { tr, locale: 'en', onNavigate: () => {} },
  });
}

function healthTile(label: string): HTMLElement {
  const section = document.querySelector('.help-health');
  const button = Array.from(section?.querySelectorAll('button') ?? []).find((item) =>
    item.textContent?.includes(label),
  );
  if (!button) throw new Error(`health tile not rendered: ${label}`);
  return button as HTMLElement;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('Help page configuration health', () => {
  test('renders real provider/key metrics and never fabricates a cooldown count', async () => {
    // Two enabled providers without keys and one enabled key.
    setupApi(
      [provider('with-key', 1), provider('keyless-a', 0), provider('keyless-b', 0)],
      [apiKey('active', true), apiKey('disabled-a', false), apiKey('disabled-b', false)],
    );
    renderPage();

    await waitFor(() => {
      expect(healthTile('Providers without API keys').querySelector('strong')?.textContent).toBe('2');
      expect(healthTile('Active API keys').querySelector('strong')?.textContent).toBe('1');
    });

    expect(screen.queryByText('Providers in cooldown')).toBeNull();
    expect(document.querySelector('.help-health')?.textContent).not.toContain('Providers in cooldown');
  });

  test('a zero keyless count is a real metric, not a fabricated cooldown tile', async () => {
    setupApi([provider('with-key', 1)], [apiKey('active', true)]);
    renderPage();

    await waitFor(() => {
      expect(healthTile('Providers without API keys').querySelector('strong')?.textContent).toBe('0');
    });
    expect(document.querySelectorAll('.help-health-items button').length).toBe(3);
    expect(screen.queryByText('Providers in cooldown')).toBeNull();
  });
});
