import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, test, vi } from 'vitest';
import SelectSearch from '../../src/components/SelectSearch.svelte';

describe('shared SelectSearch', () => {
  test('disables the localized clear action with the input', async () => {
    const onValueChange = vi.fn();
    render(SelectSearch, { props: {
      items: [{ label: 'Alpha', value: 'a' }], value: 'a', disabled: true,
      ariaLabel: 'Provider', clearLabel: 'Bỏ chọn', onValueChange,
    } });
    expect(screen.getByRole('combobox', { name: 'Provider' }).hasAttribute('disabled')).toBe(true);
    const clear = screen.getByRole('button', { name: 'Bỏ chọn' });
    expect(clear.hasAttribute('disabled')).toBe(true);
    await fireEvent.click(clear);
    expect(onValueChange).not.toHaveBeenCalled();
  });
});
