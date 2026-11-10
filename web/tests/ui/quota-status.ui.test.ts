import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardQuotaPage from '../../src/features/quota/DashboardQuotaPage.svelte';
import { api } from '../../src/lib/api';
import type { Provider, ProviderAdapterCapabilities } from '../../src/lib/types';
import type { Translate } from '../../src/lib/format';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const quotaCapabilities: ProviderAdapterCapabilities = {
  api_keys: true,
  oauth_accounts: false,
  model_discovery: true,
  usage_limits: true,
  api_key_usage: false,
  api_key_auth_assist: false,
  model_catalog_authoritative: true,
  event_stream_response: false,
  auth_panel: null,
};

function provider(id: string, apiKeyCount = 1): Provider {
  return {
    id,
    adapter_id: id,
    name: id,
    base_url: `https://${id}.example.com`,
    enabled: true,
    api_key_count: apiKeyCount,
    auth_type: 'bearer',
    preferred_protocol: 'chat_completions',
    supported_protocols: ['chat_completions'],
    capabilities: quotaCapabilities,
  };
}

function renderPage() {
  return render(DashboardQuotaPage, {
    props: {
      tr,
      locale: 'en',
      onNavigate: () => {},
      onConnectionChange: () => {},
      onProviderCountChange: () => {},
    },
  });
}

afterEach(() => vi.restoreAllMocks());

describe('quota status truthfulness', () => {
  test('does not report healthy when no provider returns quota data', async () => {
    vi.spyOn(api, 'providers').mockResolvedValue([provider('provider-a')]);
    vi.spyOn(api, 'providerUsage').mockResolvedValue({ accounts: [], next_cursor: null });

    renderPage();

    expect((await screen.findAllByText('No quota data reported')).length).toBeGreaterThanOrEqual(1);
    expect(screen.queryByText('All providers healthy')).toBeNull();
  });

  test('does not report healthy when a quota account is stale', async () => {
    vi.spyOn(api, 'providers').mockResolvedValue([provider('provider-a')]);
    vi.spyOn(api, 'providerUsage').mockResolvedValue({
      accounts: [{
        key_id: 'key-a',
        name: 'Account A',
        status: 'stale',
        snapshot: {
          limit_reached: false,
          quotas: [{ id: 'daily', label: 'Daily', used_percent: 20, remaining_percent: 80 }],
        },
      }],
      next_cursor: null,
    });

    renderPage();

    expect((await screen.findAllByText('No quota data reported')).length).toBeGreaterThanOrEqual(1);
    expect(screen.queryByText('All providers healthy')).toBeNull();
  });
});
