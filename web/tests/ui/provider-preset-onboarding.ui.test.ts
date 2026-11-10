import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardProvidersPage from '../../src/features/providers/DashboardProvidersPage.svelte';
import ProviderPresetSetupPage from '../../src/features/providers/ProviderPresetSetupPage.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type { Provider, ProviderPreset } from '../../src/lib/types';

vi.mock('../../src/features/providers/ProviderDetailsPage.svelte', async () => ({
  default: (await import('./support/ProviderDetailsStub.svelte')).default,
}));

const tr: Translate = (key, vars) => key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));
const capabilities = {
  api_keys: true, oauth_accounts: false, local_usage_meter: false, local_quota_tracking: false,
  model_discovery: true, usage_limits: false, api_key_usage: false, api_key_usage_status: 'unsupported' as const,
  api_key_auth_assist: false, model_catalog_authoritative: false, event_stream_response: false, auth_panel: null,
};
const openAiPreset: ProviderPreset = {
  id: 'openai', adapter_id: 'openai', name: 'OpenAI', description: '', category: 'cloud_api', labels: [],
  default_base_url: 'https://api.openai.com/v1', default_logo_url: null, default_model_prefix: 'openai',
  default_auth_type: 'bearer', supported_auth_types: ['bearer'], default_preferred_protocol: 'chat_completions',
  default_supported_protocols: ['chat_completions', 'responses'], protocol_selectable: true, capabilities,
};
const vertexPreset: ProviderPreset = {
  ...openAiPreset, id: 'vertex_ai', adapter_id: 'vertex_ai', name: 'Google Vertex AI', default_base_url: null,
  default_model_prefix: 'vtx', default_auth_type: 'vertex_service_account', supported_auth_types: ['vertex_service_account'],
  default_preferred_protocol: 'google_generate_content', default_supported_protocols: ['google_generate_content'], protocol_selectable: false,
};
function provider(preset: ProviderPreset, id: string, baseUrl: string): Provider {
  return { id, adapter_id: preset.adapter_id, capabilities: preset.capabilities, name: preset.name, base_url: baseUrl,
    enabled: true, auth_type: preset.default_auth_type, api_key_count: 0, model_count: 0,
    preferred_protocol: preset.default_preferred_protocol, supported_protocols: preset.default_supported_protocols };
}
function renderPage(): void {
  render(DashboardProvidersPage, { props: { tr, locale: 'en', onConnectionChange: () => {}, onProviderCountChange: () => {} } });
}
afterEach(() => {
  vi.restoreAllMocks();
  window.history.pushState(null, '', '/providers');
});
describe('provider preset onboarding', () => {
  test('default-endpoint preset creates then opens detail', async () => {
    const createdProvider = provider(openAiPreset, 'openai', 'https://api.openai.com/v1');
    vi.spyOn(api, 'providers').mockResolvedValueOnce([]).mockResolvedValueOnce([createdProvider]);
    vi.spyOn(api, 'providerPresets').mockResolvedValue([openAiPreset]);
    const createProvider = vi.spyOn(api, 'createProvider').mockResolvedValue({ ok: true, id: 'openai', key_tests: [] });
    vi.spyOn(window, 'scrollTo').mockImplementation(() => {});
    renderPage();
    await fireEvent.click(await screen.findByRole('button', { name: 'Open OpenAI details' }));
    expect(screen.queryByRole('button', { name: 'Add provider' })).toBeNull();
    expect((await screen.findByTestId('provider-detail')).getAttribute('data-provider-id')).toBe('openai');
    expect(createProvider).toHaveBeenCalledWith(expect.objectContaining({ adapter_id: 'openai', base_url: 'https://api.openai.com/v1', api_keys: [] }));
    expect(new URL(window.location.href).searchParams.get('id')).toBe('openai');
  });
  test('endpoint-less preset opens the setup page without creating a provider', async () => {
    const existingProvider = provider(openAiPreset, 'openai', 'https://api.openai.com/v1');
    vi.spyOn(api, 'providers').mockResolvedValueOnce([existingProvider]);
    vi.spyOn(api, 'providerPresets').mockResolvedValue([openAiPreset, vertexPreset]);
    const createProvider = vi.spyOn(api, 'createProvider');
    renderPage();
    await fireEvent.click(await screen.findByRole('button', { name: 'Open Google Vertex AI details' }));
    expect(document.querySelector('.provider-preset-setup')).toBeTruthy();
    expect(createProvider).not.toHaveBeenCalled();
  });
  test('Vertex setup requires a service-account key and preserves auth protocol', async () => {
    const endpoint = 'https://us-central1-aiplatform.googleapis.com/v1/projects/demo';
    const createProvider = vi.spyOn(api, 'createProvider').mockResolvedValue({ ok: true, id: 'vertex_ai', key_tests: [] });
    const onCreated = vi.fn();
    render(ProviderPresetSetupPage, { props: { preset: vertexPreset, tr, onBack: vi.fn(), onCreated } });
    expect((screen.getByLabelText('Provider name') as HTMLInputElement).value).toBe('Google Vertex AI');
    await fireEvent.input(screen.getByLabelText('Base URL'), { target: { value: endpoint } });
    await fireEvent.click(screen.getByRole('button', { name: 'Save provider' }));
    expect(createProvider).not.toHaveBeenCalled();
    expect(screen.getByLabelText('Service account key').hasAttribute('required')).toBe(true);
    await fireEvent.input(screen.getByLabelText('Service account key'), { target: { value: 'test-service-account' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Save provider' }));
    await waitFor(() => expect(createProvider).toHaveBeenCalledOnce());
    expect(createProvider).toHaveBeenCalledWith(expect.objectContaining({ adapter_id: 'vertex_ai', base_url: endpoint, auth_type: 'vertex_service_account', api_keys: ['test-service-account'], preferred_protocol: 'google_generate_content' }));
    expect(onCreated).toHaveBeenCalledWith('vertex_ai');
  });
});
