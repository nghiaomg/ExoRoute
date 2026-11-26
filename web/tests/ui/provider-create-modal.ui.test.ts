import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import ProviderCreateDialog from '../../src/features/providers/ProviderCreateDialog.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';

const tr: Translate = (key, vars) => key.replace(/\{(\w+)\}/g, (_, n) => String(vars?.[n] ?? '{'+n+'}'));

afterEach(() => vi.restoreAllMocks());

function renderModal() {
  return render(ProviderCreateDialog, { props: { open: true, presets: [], tr, onSaved: () => {}, onClose: () => {} } });
}

describe('custom provider modal layout and validation', () => {
  test('required fields show inline validation without calling the API', async () => {
    const create = vi.spyOn(api, 'createProvider');
    renderModal();
    await fireEvent.click(screen.getByRole('button', { name: 'Add provider' }));
    await waitFor(() => expect(screen.getByText('provider name is required')).toBeTruthy());
    expect(screen.getByText('Base URL is required.')).toBeTruthy();
    expect(create).not.toHaveBeenCalled();
  });

  test('invalid base URL shows inline validation', async () => {
    const create = vi.spyOn(api, 'createProvider');
    renderModal();
    await fireEvent.input(screen.getByPlaceholderText('e.g. OpenAI compatible'), { target: { value: 'Test' } });
    await fireEvent.input(screen.getByPlaceholderText('https://api.example.com/v1'), { target: { value: 'not-a-url' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Add provider' }));
    await waitFor(() => expect(screen.getByText('Base URL must be a valid URL.')).toBeTruthy());
    expect(create).not.toHaveBeenCalled();
  });

  test('key and header rows can grow for scrollable entry', async () => {
    renderModal();
    await fireEvent.click(screen.getByRole('button', { name: 'Add another key' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Add header' }));
    expect(screen.getByPlaceholderText('API key 1')).toBeTruthy();
    expect(screen.getByPlaceholderText('API key 2')).toBeTruthy();
    expect(screen.getByPlaceholderText('Header name')).toBeTruthy();
    expect(screen.getAllByRole('button', { name: 'Remove key field' }).length).toBe(2);
  });
});
