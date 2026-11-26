import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import ProviderModelCatalog from '../../src/features/providers/ProviderModelCatalog.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type { Provider } from '../../src/lib/types';

const tr: Translate = (key) => key;
const provider: Provider = {
  id: 'example', adapter_id: 'generic', name: 'Example', base_url: 'https://example.com',
  enabled: true, auth_type: 'bearer', preferred_protocol: 'chat_completions',
  supported_protocols: ['chat_completions'],
};

afterEach(() => vi.restoreAllMocks());

describe('provider manual model field', () => {
  test('validates an empty model and submits the typed ID', async () => {
    vi.spyOn(api, 'providerModels').mockResolvedValue({ models: [], manual_models: [] });
    const addModel = vi.spyOn(api, 'addManualProviderModel').mockResolvedValue({ model: 'provider/model', saved: true });
    const onProviderChanged = vi.fn();
    render(ProviderModelCatalog, { props: { provider, open: true, tr, onProviderChanged } });

    await fireEvent.click(screen.getByRole('button', { name: 'Save model' }));
    expect((await screen.findByRole('alert')).textContent).toContain('Provider model ID is required.');
    expect(addModel).not.toHaveBeenCalled();
    await fireEvent.input(screen.getByRole('textbox', { name: 'Model ID' }), { target: { value: 'provider/model' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Save model' }));
    await waitFor(() => expect(addModel).toHaveBeenCalledWith('example', 'provider/model'));
    expect(onProviderChanged).toHaveBeenCalledOnce();
  });
});
