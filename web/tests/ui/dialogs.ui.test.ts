import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import ProviderConfigDialog from '../../src/features/providers/ProviderConfigDialog.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type { Provider } from '../../src/lib/types';

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
    await fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    await waitFor(() => {
      expect(updateProvider).toHaveBeenCalledWith(
        'openai',
        expect.objectContaining({ model_prefix: 'custom' }),
      );
      expect(onProviderUpdated).toHaveBeenCalledWith(expect.objectContaining({ model_prefix: 'custom' }));
    });
  });
});
