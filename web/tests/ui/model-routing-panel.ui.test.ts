import { render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import ProviderModelRouting from '../../src/features/providers/ProviderModelRouting.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type { Provider, ProviderModelRoutingPage } from '../../src/lib/types';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

/**
 * The gateway refuses a model whose upstream protocol cannot be resolved and
 * tells the operator to set it in the provider model settings. OpenCode Go is
 * exactly such an adapter, so its saved models must expose this panel even
 * though the adapter ships a default model mapping.
 */
const opencodeGoProvider = {
  id: 'opencode-go',
  adapter_id: 'opencode_go',
  capabilities: {
    model_protocol_routing: true,
    supported_upstream_protocols: ['chat_completions', 'responses', 'messages'],
  },
} as unknown as Provider;

const UNMAPPED_MODEL = 'longcat-2.5-preview-free';

function routingPage(overrideProtocol: 'chat_completions' | null): ProviderModelRoutingPage {
  return {
    supported_protocols: ['chat_completions', 'responses', 'messages'],
    next_cursor: null,
    models: [
      {
        model: 'minimax-m3',
        effective_upstream_protocol: 'messages',
        override_protocol: null,
        routing_configured: true,
      },
      {
        model: UNMAPPED_MODEL,
        effective_upstream_protocol: overrideProtocol,
        override_protocol: overrideProtocol,
        routing_configured: overrideProtocol !== null,
      },
    ],
  };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('ProviderModelRouting', () => {
  test('flags an OpenCode provider model the default mapping does not know', async () => {
    const load = vi.spyOn(api, 'providerModelRouting').mockResolvedValue(routingPage(null));

    render(ProviderModelRouting, { props: { provider: opencodeGoProvider, tr } });

    // The panel is available for the adapter that needs it, and the unmapped
    // model is called out instead of only failing at request time.
    expect(await screen.findByText('Model protocol routing')).toBeTruthy();
    expect(screen.getByText(UNMAPPED_MODEL)).toBeTruthy();
    expect(screen.getByText('Protocol unknown')).toBeTruthy();
    expect(screen.getByText('Choose a protocol to enable this model.')).toBeTruthy();
    // The mapped model keeps reporting its adapter-owned default mapping.
    expect(screen.getByText('Default model mapping')).toBeTruthy();
    expect(load).toHaveBeenCalledWith('opencode-go', expect.objectContaining({ limit: 50 }));

    // With no override drafted the Save action stays disabled.
    const saveButtons = screen.getAllByRole('button', { name: 'Save' });
    expect((saveButtons[saveButtons.length - 1] as HTMLButtonElement).disabled).toBe(true);
  });

  test('does not request routing data for an adapter that routes per model only by default', async () => {
    const load = vi.spyOn(api, 'providerModelRouting');
    const plainProvider = {
      id: 'openrouter',
      adapter_id: 'openrouter',
      capabilities: { model_protocol_routing: false, supported_upstream_protocols: ['chat_completions'] },
    } as unknown as Provider;

    render(ProviderModelRouting, { props: { provider: plainProvider, tr } });

    await waitFor(() => {
      expect(load).not.toHaveBeenCalled();
    });
    expect(screen.queryByText('Model protocol routing')).toBeNull();
  });
});
