import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import ApiKeyScopeEditor from '../../src/features/api-keys/ApiKeyScopeEditor.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type { GatewayApiKey, Provider } from '../../src/lib/types';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

/** Types a provider into the add-picker and confirms the highlighted match. */
async function addProvider(query: string): Promise<void> {
  const input = screen.getByPlaceholderText('Search providers by name or ID');
  await fireEvent.click(input);
  await fireEvent.input(input, { target: { value: query } });
  await fireEvent.keyDown(input, { key: 'ArrowDown' });
  await fireEvent.keyDown(input, { key: 'Enter' });
}

function provider(id: string, name: string): Provider {
  return {
    id,
    name,
    adapter_id: 'generic',
    base_url: 'http://127.0.0.1:9/v1',
    enabled: true,
    auth_type: 'none',
    preferred_protocol: 'chat_completions',
    supported_protocols: ['chat_completions'],
  };
}

const providers = [provider('openai', 'OpenAI'), provider('anthropic', 'Anthropic')];

const apiKey: GatewayApiKey = {
  id: 'key-1',
  name: 'Scoped key',
  enabled: true,
  created_at: '2026-09-13 12:00:00',
  allowed_provider_ids: ['openai'],
  allowed_models: ['gpt-4*'],
  scope_restricted: true,
  scope_valid: true,
};

afterEach(() => {
  vi.restoreAllMocks();
});

describe('API key scope editor', () => {
  test('saves the selected providers and model rules through the dashboard client', async () => {
    vi.spyOn(api, 'providers').mockResolvedValue(providers);
    const update = vi.spyOn(api, 'updateApiKey').mockResolvedValue({
      ...apiKey,
      allowed_provider_ids: ['openai', 'anthropic'],
      allowed_models: ['gpt-4*', 'claude-3-5-sonnet'],
    });
    const onUpdated = vi.fn();

    render(ApiKeyScopeEditor, { props: { apiKey, tr, onUpdated } });

    // The stored scope shows as a removable chip; the combobox only adds.
    expect(await screen.findByRole('button', { name: 'Remove OpenAI' })).toBeTruthy();
    await addProvider('anthropic');

    const rules = screen.getByLabelText('Allowed models') as HTMLTextAreaElement;
    expect(rules.value).toBe('gpt-4*');
    await fireEvent.input(rules, { target: { value: 'gpt-4*\n claude-3-5-sonnet \n\n' } });

    await fireEvent.click(screen.getByRole('button', { name: 'Save access scope' }));

    await waitFor(() =>
      expect(update).toHaveBeenCalledWith('key-1', {
        allowed_provider_ids: ['openai', 'anthropic'],
        allowed_models: ['gpt-4*', 'claude-3-5-sonnet'],
      }),
    );
    await waitFor(() => expect(onUpdated).toHaveBeenCalled());
    const toast = await screen.findByRole('status');
    expect(toast.classList.contains('app-toast')).toBe(true);
    expect(toast.textContent).toContain('Access scope saved.');
    expect(screen.getByText('2 allowed providers')).toBeTruthy();
  });

  test('warns that an invalid stored scope rejects every request', async () => {
    vi.spyOn(api, 'providers').mockResolvedValue(providers);

    render(ApiKeyScopeEditor, {
      props: {
        apiKey: {
          ...apiKey,
          allowed_provider_ids: null,
          allowed_models: null,
          scope_valid: false,
        },
        tr,
        onUpdated: () => {},
      },
    });

    const warning = await screen.findByRole('alert');
    expect(warning.textContent).toContain('stored access scope is invalid');
    expect((screen.getByLabelText('Allowed models') as HTMLTextAreaElement).value).toBe('');
  });

  test('a scope that references a deleted provider can be cleared', async () => {
    vi.spyOn(api, 'providers').mockResolvedValue(providers);
    const update = vi.spyOn(api, 'updateApiKey').mockResolvedValue({
      ...apiKey,
      allowed_provider_ids: ['openai'],
    });

    render(ApiKeyScopeEditor, {
      props: {
        apiKey: { ...apiKey, allowed_provider_ids: ['openai', 'ghost-provider'] },
        tr,
        onUpdated: () => {},
      },
    });

    const removeGhost = await screen.findByRole('button', { name: 'Remove ghost-provider' });
    expect(screen.getByText('Deleted')).toBeTruthy();

    await fireEvent.click(removeGhost);
    expect(screen.queryByRole('button', { name: 'Remove ghost-provider' })).toBeNull();
    expect(screen.queryByText('Deleted')).toBeNull();

    await fireEvent.click(screen.getByRole('button', { name: 'Save access scope' }));
    await waitFor(() =>
      expect(update).toHaveBeenCalledWith('key-1', {
        allowed_provider_ids: ['openai'],
        allowed_models: ['gpt-4*'],
      }),
    );
  });

  test('reports a failed save without losing the draft', async () => {
    vi.spyOn(api, 'providers').mockResolvedValue(providers);
    vi.spyOn(api, 'updateApiKey').mockRejectedValue(new Error('offline'));
    const onUpdated = vi.fn();

    render(ApiKeyScopeEditor, { props: { apiKey, tr, onUpdated } });

    expect(await screen.findByRole('button', { name: 'Remove OpenAI' })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Save access scope' }));

    expect(
      await screen.findByText('Could not save this key’s access scope.'),
    ).toBeTruthy();
    expect(onUpdated).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Remove OpenAI' })).toBeTruthy();
    expect(screen.queryByRole('status')).toBeNull();
  });
});
