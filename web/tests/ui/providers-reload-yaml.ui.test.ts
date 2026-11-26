import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { get } from 'svelte/store';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardProvidersPage from '../../src/features/providers/DashboardProvidersPage.svelte';
import { dashboardToast } from '../../src/lib/dashboard-ui';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type { ProviderPreset } from '../../src/lib/types';

vi.mock('../../src/features/providers/ProviderDetailsPage.svelte', async () => ({
  default: (await import('./support/ProviderDetailsStub.svelte')).default,
}));

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const capabilities = {
  api_keys: true, oauth_accounts: false, local_usage_meter: false, local_quota_tracking: false,
  model_discovery: true, usage_limits: false, api_key_usage: false, api_key_usage_status: 'unsupported' as const,
  api_key_auth_assist: false, model_catalog_authoritative: false, event_stream_response: false, auth_panel: null,
};

const preset: ProviderPreset = {
  id: 'openai', adapter_id: 'openai', name: 'OpenAI', description: '', category: 'cloud_api', labels: [],
  default_base_url: 'https://api.openai.com/v1', default_logo_url: null, default_model_prefix: 'openai',
  default_auth_type: 'bearer', supported_auth_types: ['bearer'], default_preferred_protocol: 'chat_completions',
  default_supported_protocols: ['chat_completions', 'responses'], protocol_selectable: true, capabilities,
};

const reloadedPreset: ProviderPreset = { ...preset, id: 'reloaded', adapter_id: 'reloaded', name: 'Reloaded provider' };

function renderPage(): void {
  render(DashboardProvidersPage, {
    props: { tr, locale: 'en', onConnectionChange: () => {}, onProviderCountChange: () => {} },
  });
}

afterEach(() => {
  vi.restoreAllMocks();
  window.history.pushState(null, '', '/providers');
});

describe('providers.yaml reload action', () => {
  test('renders the Reload YAML action button', async () => {
    vi.spyOn(api, 'providers').mockResolvedValue([]);
    vi.spyOn(api, 'providerPresets').mockResolvedValue([preset]);
    renderPage();
    expect(await screen.findByRole('button', { name: 'Reload YAML' })).toBeTruthy();
  });

  test('calls the reload endpoint, refreshes presets and shows a success toast', async () => {
    vi.spyOn(api, 'providers').mockResolvedValue([]);
    vi.spyOn(api, 'providerPresets').mockResolvedValueOnce([preset]).mockResolvedValueOnce([reloadedPreset]);
    let resolveReload!: (value: { reloaded: boolean; provider_count: number; overlaid_providers: string[] }) => void;
    const reload = vi.spyOn(api, 'reloadProviderPresets').mockImplementation(
      () => new Promise((resolve) => { resolveReload = resolve; }),
    );
    renderPage();
    const button = await screen.findByRole('button', { name: 'Reload YAML' }) as HTMLButtonElement;

    await fireEvent.click(button);
    expect(button.disabled).toBe(true);
    expect(screen.getByRole('button', { name: 'Reloading…' })).toBeTruthy();

    resolveReload({ reloaded: true, provider_count: 7, overlaid_providers: ['private-provider'] });
    await waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByText('Reloaded provider')).toBeTruthy());
    await waitFor(() => {
      expect(get(dashboardToast)?.message).toContain('7');
      expect(get(dashboardToast)?.message).toContain('1');
    });
    expect(button.disabled).toBe(false);
  });

  test('shows the server error and re-enables the button when reload fails', async () => {
    vi.spyOn(api, 'providers').mockResolvedValue([]);
    vi.spyOn(api, 'providerPresets').mockResolvedValue([preset]);
    vi.spyOn(api, 'reloadProviderPresets').mockRejectedValue(new Error('invalid YAML at line 4'));
    renderPage();
    const button = await screen.findByRole('button', { name: 'Reload YAML' }) as HTMLButtonElement;

    await fireEvent.click(button);
    await waitFor(() => {
      expect(get(dashboardToast)?.message).toContain('invalid YAML at line 4');
    });
    expect(button.disabled).toBe(false);
    expect(screen.getByRole('button', { name: 'Reload YAML' })).toBeTruthy();
  });
});
