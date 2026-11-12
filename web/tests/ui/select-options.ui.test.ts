import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, test, vi } from 'vitest';
import SelectOptions from '../../src/components/SelectOptions.svelte';

/**
 * SelectOptions is a native `<select>`: the accessible name comes from the
 * aria-label, the options are plain `<option>` elements, and choosing one is
 * a normal change event instead of a portal-rendered Ark listbox.
 */
describe('shared SelectOptions', () => {
  test('renders a labelled native select and reports the chosen option', async () => {
    const onValueChange = vi.fn();
    render(SelectOptions, { props: {
      ariaLabel: 'Provider', value: 'a', onValueChange,
      items: [{ value: 'a', label: 'Alpha' }, { value: 'b', label: 'Beta' }],
    } });
    const select = screen.getByRole('combobox', { name: 'Provider' }) as HTMLSelectElement;
    expect(select.tagName).toBe('SELECT');
    expect(select.value).toBe('a');
    await fireEvent.change(select, { target: { value: 'b' } });
    expect(onValueChange).toHaveBeenCalledWith('b');
    expect(select.value).toBe('b');
  });

  test('renders the placeholder as the leading empty option', () => {
    render(SelectOptions, { props: { ariaLabel: 'Provider', placeholder: 'Pick one', items: [{ value: 'a', label: 'Alpha' }] } });
    const select = screen.getByRole('combobox', { name: 'Provider' }) as HTMLSelectElement;
    const first = select.options[0];
    expect(first.value).toBe('');
    expect(first.textContent).toBe('Pick one');
    expect(select.value).toBe('');
  });

  test('does not accept input when disabled', () => {
    render(SelectOptions, { props: { ariaLabel: 'Provider', disabled: true, items: [{ value: 'a', label: 'Alpha' }] } });
    const select = screen.getByRole('combobox', { name: 'Provider' });
    expect(select.hasAttribute('disabled')).toBe(true);
  });

  test('clears a selection with a named control', async () => {
    const onValueChange = vi.fn();
    render(SelectOptions, { props: {
      ariaLabel: 'Provider', clearLabel: 'Bỏ chọn', clearable: true, value: 'a', onValueChange,
      items: [{ value: 'a', label: 'Alpha' }],
    } });
    await fireEvent.click(screen.getByRole('button', { name: 'Bỏ chọn' }));
    expect(onValueChange).toHaveBeenCalledWith('');
    expect((screen.getByRole('combobox', { name: 'Provider' }) as HTMLSelectElement).value).toBe('');
  });

  test('renders no clear control until a value is chosen', () => {
    render(SelectOptions, { props: {
      ariaLabel: 'Provider', clearLabel: 'Bỏ chọn', clearable: true,
      items: [{ value: 'a', label: 'Alpha' }],
    } });
    expect(screen.queryByRole('button', { name: 'Bỏ chọn' })).toBeNull();
  });

  test('disables the clear control when the select is disabled', () => {
    render(SelectOptions, { props: {
      ariaLabel: 'Provider', clearLabel: 'Bỏ chọn', clearable: true, disabled: true, value: 'a',
      items: [{ value: 'a', label: 'Alpha' }],
    } });
    expect(screen.getByRole('button', { name: 'Bỏ chọn' }).hasAttribute('disabled')).toBe(true);
  });

  test('marks disabled options with the disabled attribute', () => {
    render(SelectOptions, { props: {
      ariaLabel: 'Provider',
      items: [{ value: 'a', label: 'Alpha' }, { value: 'b', label: 'Beta', disabled: true }],
    } });
    const disabledOption = screen.getByRole('option', { name: 'Beta' });
    expect(disabledOption.hasAttribute('disabled')).toBe(true);
    expect(disabledOption.getAttribute('aria-disabled')).toBeNull();
  });

  test('groups options under the group label', () => {
    render(SelectOptions, { props: {
      ariaLabel: 'Provider', groupLabel: 'Targets', items: [{ value: 'a', label: 'Alpha' }],
    } });
    expect(screen.getByRole('group', { name: 'Targets' })).toBeTruthy();
    expect(screen.getByRole('option', { name: 'Alpha' })).toBeTruthy();
  });
});
