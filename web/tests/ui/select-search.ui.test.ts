import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, test, vi } from 'vitest';
import SelectSearch from '../../src/components/SelectSearch.svelte';

/**
 * SelectSearch is a hand-built searchable combobox: a closed selection shows
 * as a trigger button, opening swaps it for a text input plus an in-flow
 * `role="listbox"` panel whose rows are `role="option"` buttons.
 */
describe('shared SelectSearch', () => {
  test('shows the selection as a trigger and picks a filtered option from the listbox', async () => {
    const onValueChange = vi.fn();
    render(SelectSearch, { props: {
      items: [{ label: 'Alpha', value: 'a' }, { label: 'Beta', value: 'b', hint: 'prov-b' }],
      value: 'a', ariaLabel: 'Provider', onValueChange,
    } });
    // Closed with a selection the combobox is a single input showing the
    // selected label; opening swaps to the live query.
    const trigger = screen.getByRole('combobox', { name: 'Provider' }) as HTMLInputElement;
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
    expect(trigger.value).toBe('Alpha');

    await fireEvent.click(trigger);
    expect(trigger.getAttribute('aria-expanded')).toBe('true');
    const listbox = await screen.findByRole('listbox');
    expect(listbox.getAttribute('id')).toBe(trigger.getAttribute('aria-controls'));

    // The hint text is searchable even though only the label is displayed.
    await fireEvent.input(trigger, { target: { value: 'prov-b' } });
    expect(screen.getByRole('option', { name: /Beta/ }).getAttribute('aria-selected')).toBe('false');

    await fireEvent.keyDown(trigger, { key: 'Enter' });
    expect(onValueChange).toHaveBeenCalledWith('b');
    expect(screen.queryByRole('listbox')).toBeNull();
    expect((screen.getByRole('combobox', { name: 'Provider' }) as HTMLInputElement).value).toBe('Beta');
  });

  test('forwards search queries to a caller-owned data source', async () => {
    const onSearch = vi.fn();
    render(SelectSearch, { props: {
      items: [{ label: 'Alpha', value: 'a' }], ariaLabel: 'Provider', onSearch,
    } });
    // With no selection the search input renders directly, closed.
    const input = screen.getByRole('combobox', { name: 'Provider' });
    expect(input.getAttribute('aria-expanded')).toBe('false');

    await fireEvent.click(input);
    expect(onSearch).toHaveBeenLastCalledWith('');
    await fireEvent.input(input, { target: { value: 'al' } });
    expect(onSearch).toHaveBeenLastCalledWith('al');
    expect(screen.getByRole('option', { name: 'Alpha' }).getAttribute('aria-selected')).toBe('false');

    await fireEvent.keyDown(input, { key: 'Escape' });
    expect(screen.queryByRole('listbox')).toBeNull();
    expect(input.getAttribute('aria-expanded')).toBe('false');
  });

  test('shows the empty text row when the filter matches nothing', async () => {
    render(SelectSearch, { props: {
      items: [{ label: 'Alpha', value: 'a' }], ariaLabel: 'Provider', noOptionsText: 'Không có kết quả',
    } });
    const input = screen.getByRole('combobox', { name: 'Provider' });
    await fireEvent.click(input);
    await fireEvent.input(input, { target: { value: 'zzz' } });
    expect(screen.getByText('Không có kết quả')).toBeTruthy();
    expect(screen.queryByRole('option')).toBeNull();
  });

  test('clears the selection with the named clear action', async () => {
    const onValueChange = vi.fn();
    render(SelectSearch, { props: {
      items: [{ label: 'Alpha', value: 'a' }], value: 'a', ariaLabel: 'Provider',
      clearLabel: 'Bỏ chọn', onValueChange,
    } });
    await fireEvent.click(screen.getByRole('button', { name: 'Bỏ chọn' }));
    expect(onValueChange).toHaveBeenCalledWith('');
    // The clear affordance disappears once there is nothing to clear.
    expect(screen.queryByRole('button', { name: 'Bỏ chọn' })).toBeNull();
  });

  test('disables the localized clear action with the input', async () => {
    const onValueChange = vi.fn();
    render(SelectSearch, { props: {
      items: [{ label: 'Alpha', value: 'a' }], value: 'a', disabled: true,
      ariaLabel: 'Provider', clearLabel: 'Bỏ chọn', onValueChange,
    } });
    const trigger = screen.getByRole('combobox', { name: 'Provider' }) as HTMLInputElement;
    expect(trigger.hasAttribute('disabled')).toBe(true);
    const clear = screen.getByRole('button', { name: 'Bỏ chọn' });
    expect(clear.hasAttribute('disabled')).toBe(true);
    await fireEvent.click(clear);
    expect(onValueChange).not.toHaveBeenCalled();
  });
});
