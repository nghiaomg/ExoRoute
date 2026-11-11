import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, test, vi } from 'vitest';
import SelectOptions from '../../src/components/SelectOptions.svelte';

describe('shared SelectOptions', () => {
  test('opens a labelled list and reports the chosen option', async () => {
    const onValueChange = vi.fn();
    render(SelectOptions, { props: {
      ariaLabel: 'Provider', value: 'a', onValueChange,
      items: [{ value: 'a', label: 'Alpha' }, { value: 'b', label: 'Beta' }],
    } });
    const trigger = screen.getByRole('combobox', { name: 'Provider' });
    expect(trigger.textContent).toContain('Alpha');
    await fireEvent.click(trigger);
    const option = await screen.findByRole('option', { name: 'Beta' });
    await fireEvent.click(option);
    await waitFor(() => expect(onValueChange).toHaveBeenCalledWith('b'));
    expect(trigger.textContent).toContain('Beta');
  });

  test('does not open a disabled select', async () => {
    render(SelectOptions, { props: { ariaLabel: 'Provider', disabled: true, items: [{ value: 'a', label: 'Alpha' }] } });
    const trigger = screen.getByRole('combobox', { name: 'Provider' });
    expect(trigger.hasAttribute('disabled')).toBe(true);
    await fireEvent.click(trigger);
    expect(screen.queryByRole('option')).toBeNull();
  });

  test('clears a selection with a named control', async () => {
    const onValueChange = vi.fn();
    render(SelectOptions, { props: {
      ariaLabel: 'Provider', clearLabel: 'Bỏ chọn', clearable: true, value: 'a', onValueChange,
      items: [{ value: 'a', label: 'Alpha' }],
    } });
    await fireEvent.click(screen.getByRole('button', { name: 'Bỏ chọn' }));
    await waitFor(() => expect(onValueChange).toHaveBeenCalledWith(''));
    expect(screen.getByRole('combobox', { name: 'Provider' }).textContent).toContain('Select…');
  });

  test('disables the clear control when the select is disabled', () => {
    render(SelectOptions, { props: {
      ariaLabel: 'Provider', clearLabel: 'Bỏ chọn', clearable: true, disabled: true, value: 'a',
      items: [{ value: 'a', label: 'Alpha' }],
    } });
    expect(screen.getByRole('button', { name: 'Bỏ chọn' }).hasAttribute('disabled')).toBe(true);
  });

  test('opens with the keyboard and refuses disabled options', async () => {
    const onValueChange = vi.fn();
    render(SelectOptions, { props: {
      ariaLabel: 'Provider', value: 'a', onValueChange,
      items: [{ value: 'a', label: 'Alpha' }, { value: 'b', label: 'Beta', disabled: true }],
    } });
    const trigger = screen.getByRole('combobox', { name: 'Provider' });
    trigger.focus();
    await fireEvent.keyDown(trigger, { key: 'ArrowDown' });
    const disabledOption = await screen.findByRole('option', { name: 'Beta' });
    expect(disabledOption.getAttribute('aria-disabled')).toBe('true');
    await fireEvent.click(disabledOption);
    expect(onValueChange).not.toHaveBeenCalled();
  });
});
