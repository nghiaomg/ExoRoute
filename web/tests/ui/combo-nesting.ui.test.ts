import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import ComboCard from '../../src/features/combos/ComboCard.svelte';
import ComboEditorDialog from '../../src/features/combos/ComboEditorDialog.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type { ComboProviderOption, GatewayCombo } from '../../src/lib/types';

const tr: Translate = (key, vars) => key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const comboProviders: ComboProviderOption[] = [{ id: 'openai', name: 'OpenAI', enabled: true }];

function combo(id: string, name: string, targets: GatewayCombo['targets']): GatewayCombo {
  return { id, name, strategy: 'priority', accepted_protocols: ['chat_completions'], targets };
}

const childCombo = combo('child', 'Child', [
  { provider_id: 'openai', model: 'gpt-4o', priority: 1, enabled: true },
]);

afterEach(() => {
  vi.restoreAllMocks();
});

describe('nested combo targets', () => {
  test('the editor submits a nested target as a combo_id', async () => {
    const createCombo = vi.spyOn(api, 'createCombo').mockResolvedValue({ ok: true, id: 'parent', combo_id: 'parent' });

    render(ComboEditorDialog, {
      props: {
        open: true,
        comboToEdit: null,
        tr,
        onClose: vi.fn(),
        onSaved: vi.fn(),
        onNavigate: vi.fn(),
        providers: comboProviders,
        combos: [childCombo],
      },
    });

    const name = await screen.findByLabelText('Combo name') as HTMLInputElement;
    await fireEvent.input(name, { target: { value: 'parent' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Combo target' }));
    const input = screen.getByPlaceholderText('Search combos by name or ID');
    await fireEvent.click(input);
    await fireEvent.input(input, { target: { value: 'Child' } });
    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    await fireEvent.keyDown(input, { key: 'Enter' });
    const saveButton = screen.getByRole('button', { name: 'Create combo' }) as HTMLButtonElement;
    expect(input.value).toBe('Child');
    expect(saveButton.disabled).toBe(false);
    await fireEvent.click(saveButton);

    await waitFor(() => {
      expect(createCombo).toHaveBeenCalledWith(expect.objectContaining({
        name: 'parent',
        targets: [{ provider_id: '', model: '', combo_id: 'child', priority: 1, enabled: true }],
      }));
    });
  });

  test('the editor does not offer the combo being edited or one that reaches it', async () => {
    // Editing `parent`, which already nests `child`. `root` nests `parent`, so
    // nesting it here would close a cycle; `child` stays selectable.
    const parent = combo('parent', 'Parent', [
      { provider_id: 'openai', model: 'gpt-4o', priority: 1, enabled: true },
      { provider_id: '', model: '', combo_id: 'child', priority: 2, enabled: true },
    ]);
    const root = combo('root', 'Root', [
      { provider_id: '', model: '', combo_id: 'parent', priority: 1, enabled: true },
    ]);

    render(ComboEditorDialog, {
      props: {
        open: true,
        comboToEdit: parent,
        tr,
        onClose: vi.fn(),
        onSaved: vi.fn(),
        onNavigate: vi.fn(),
        providers: comboProviders,
        combos: [parent, childCombo, root],
      },
    });

    const [firstKindToggle] = await screen.findAllByRole('button', { name: 'Combo target' });
    await fireEvent.click(firstKindToggle);
    // The second row already nests a combo, so its picker is on screen too.
    const [firstPicker] = screen.getAllByPlaceholderText('Search combos by name or ID');
    await fireEvent.click(firstPicker);

    expect(await screen.findByRole('option', { name: 'Child' })).toBeTruthy();
    expect(screen.queryByRole('option', { name: 'Parent' })).toBeNull();
    expect(screen.queryByRole('option', { name: 'Root' })).toBeNull();
  });

  test('the card labels a nested target with the combo it names', async () => {
    const parent = combo('parent', 'Parent', [
      { provider_id: 'openai', model: 'gpt-4o', priority: 1, enabled: true },
      { provider_id: '', model: '', combo_id: 'child', priority: 2, enabled: true },
    ]);

    const { unmount } = render(ComboCard, {
      props: {
        combo: parent,
        providerNames: { openai: 'OpenAI' },
        comboNames: { child: 'Child' },
        tr,
        onDelete: vi.fn(),
      },
    });

    expect(await screen.findByText('Nested combo')).toBeTruthy();
    expect(screen.getByText('Child')).toBeTruthy();
    unmount();

    render(ComboCard, {
      props: {
        combo: parent,
        providerNames: { openai: 'OpenAI' },
        comboNames: {},
        tr,
        onDelete: vi.fn(),
      },
    });

    expect(await screen.findByText('Missing combo')).toBeTruthy();
    expect(screen.getByText('child')).toBeTruthy();
  });
});
