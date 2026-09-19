import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import ComboEditorDialog from '../../src/features/combos/ComboEditorDialog.svelte';
import ProviderConfigDialog from '../../src/features/providers/ProviderConfigDialog.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type { ComboProviderOption, Provider } from '../../src/lib/types';

const tr: Translate = (key, vars) => key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const provider: Provider = {
  id: 'openai',
  adapter_id: 'openai',
  name: 'OpenAI',
  base_url: 'https://api.openai.com/v1',
  model_prefix: 'openai',
  enabled: true,
  auth_type: 'bearer',
  preferred_protocol: 'chat_completions',
  supported_protocols: ['chat_completions'],
  custom_headers: [],
};

const comboProviders: ComboProviderOption[] = [
  { id: provider.id, name: provider.name, enabled: true },
];

afterEach(() => {
  vi.restoreAllMocks();
});

describe('complex admin dialogs', () => {
  test('ProviderConfigDialog saves the edited model prefix through the API', async () => {
    const updateProvider = vi.spyOn(api, 'updateProvider').mockResolvedValue({ ok: true, key_tests: [] });
    const onProviderUpdated = vi.fn();

    render(ProviderConfigDialog, {
      props: { open: true, provider, tr, onProviderUpdated },
    });

    expect(await screen.findByText('Provider Configurations')).toBeTruthy();
    const input = await screen.findByLabelText('Model prefix') as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'custom' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Save model prefix' }));

    await waitFor(() => expect(updateProvider).toHaveBeenCalledWith(
      'openai',
      expect.objectContaining({ model_prefix: 'custom' }),
    ));
    expect(onProviderUpdated).toHaveBeenCalledWith(expect.objectContaining({ model_prefix: 'custom' }));
  });

  test('ComboEditorDialog renders a primary target and adds a fallback target', async () => {
    render(ComboEditorDialog, {
      props: {
        open: true,
        comboToEdit: null,
        tr,
        onClose: vi.fn(),
        onSaved: vi.fn(),
        onNavigate: vi.fn(),
        providers: comboProviders,
      },
    });

    expect(await screen.findByText('Create a combo')).toBeTruthy();
    expect(await screen.findByText('Primary target')).toBeTruthy();
    expect(screen.getByText('1 / 32')).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'Add fallback target' }));

    expect(screen.getByText('2 / 32')).toBeTruthy();
    expect(screen.getByText('Fallback 1')).toBeTruthy();
  });
});
