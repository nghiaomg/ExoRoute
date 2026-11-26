import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, test, vi } from 'vitest';
import RangeDropdown from '../../src/features/overview/RangeDropdown.svelte';
import type { Translate } from '../../src/lib/format';
const tr: Translate = (k) => k;
const options = [
  { value: '1H', label: '1H', disabled: true, hint: 'Unavailable' },
  { value: '24H', label: '24H' },
  { value: '7D', label: '7D' },
  { value: '30D', label: '30D' },
  { value: '90D', label: '90D', disabled: true, hint: 'Unavailable' },
  { value: 'CUSTOM', label: 'Custom', disabled: true, hint: 'Unavailable' },
];
describe('custom range dropdown', () => {
  test('opens a themed listbox with disabled state and selection callback', async () => {
    const onChange = vi.fn();
    render(RangeDropdown, { props: { tr, value: '24H', options, label: 'Time range', onChange } });
    const trigger = screen.getByRole('button', { name: /Time range: 24H/ });
    expect(trigger.getAttribute('aria-haspopup')).toBe('listbox');
    await fireEvent.click(trigger);
    expect(screen.getByRole('listbox')).toBeTruthy();
    expect(screen.getAllByText('Unavailable').length).toBe(3);
    expect(screen.getByRole('button', { name: /30D/ })).toBeTruthy();
    expect(screen.getByRole('button', { name: /Custom/ })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: '7D' }));
    expect(onChange).toHaveBeenCalledWith('7D');
  });
});
