import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import ProviderDetailHeader from '../../src/features/providers/ProviderDetailHeader.svelte';
import ProviderModelCatalog from '../../src/features/providers/ProviderModelCatalog.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type { Provider, ProviderKey, GatewayCombo } from '../../src/lib/types';
const tr: Translate = (key, vars) => key.replace(/\{(\w+)\}/g, (_, n) => String(vars?.[n] ?? `{${n}}`));
const provider: Provider = { id: 'alibaba-dashscope', adapter_id: 'alibaba-dashscope', name: 'Alibaba DashScope', base_url: 'https://dashscope.aliyuncs.com/compatible-mode/v1', enabled: true, auth_type: 'bearer', api_key_count: 0, preferred_protocol: 'chat_completions', supported_protocols: ['chat_completions'], model_prefix: 'dashscope', capabilities: { api_keys: true, oauth_accounts: false, model_discovery: true, usage_limits: false, api_key_usage: false, api_key_auth_assist: false, model_catalog_authoritative: true, event_stream_response: true } as never };
afterEach(() => vi.restoreAllMocks());
describe('provider detail workspace', () => {
  test('renders breadcrumb, toggle, test action and telemetry strip', () => {
    const combos: GatewayCombo[] = [{ id: 'c1', name: 'fallback', strategy: 'priority', accepted_protocols: [], targets: [{ provider_id: provider.id, model: 'qwen-max', priority: 0, enabled: true }] }];
    render(ProviderDetailHeader, { props: { provider, tr, locale: 'en', combos, stats: null, onBack: vi.fn(), onToggle: vi.fn(), onEdit: vi.fn(), onManageKeys: vi.fn(), onDelete: vi.fn(), onNavigateToCombo: vi.fn() } });
    const crumb = screen.getByRole('navigation');
    expect(crumb.textContent).toContain('Providers');
    expect(crumb.textContent).toContain('Alibaba DashScope');
    expect(crumb.querySelector('[aria-current="page"]')).toBeTruthy();
    expect(screen.getByRole('switch')).toBeTruthy();
    expect(screen.getByRole('button', { name: /Test connection/ })).toBeTruthy();
    expect(screen.getByText('fallback')).toBeTruthy();
  });
  test('shows inline add-key CTA in keys card', async () => {
    vi.spyOn(api, 'providerKeys').mockResolvedValue({ keys: [] as ProviderKey[], next_cursor: null, total: 0 });
    const { default: ProviderKeysCard } = await import('../../src/features/providers/ProviderKeysCard.svelte');
    render(ProviderKeysCard, { props: { provider, tr, locale: 'en', onProviderChanged: vi.fn(), onAddKey: vi.fn() } });
    await screen.findByText('Add a key to start sending requests');
    expect(screen.getAllByRole('button', { name: /\+ Add API key/ }).length).toBe(2);
  });
  test('model input shows prefix addon and strips duplicate prefix', async () => {
    vi.spyOn(api, 'providerModels').mockResolvedValue({ models: ['qwen-max'], manual_models: [], has_more: false });
    render(ProviderModelCatalog, { props: { provider, open: true, tr, onProviderChanged: vi.fn() } });
    await screen.findByText('qwen-max');
    const input = screen.getByLabelText('Model ID') as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'dashscope/qwen-plus' } });
    expect(input.value).toBe('qwen-plus');
    expect(screen.getByText(/dashscope\/qwen-plus/)).toBeTruthy();
  });
});
