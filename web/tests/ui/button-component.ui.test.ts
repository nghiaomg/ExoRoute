import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, test, vi } from 'vitest';
import ButtonHarness from './support/ButtonHarness.svelte';

describe('shared Button', () => {
  test('forwards native type, accessibility, disabled state and click handler', async () => {
    const onClick = vi.fn();
    render(ButtonHarness, { props: { onclick: onClick } });
    const button = screen.getByRole('button', { name: 'Save' });
    expect(button.classList.contains('button-secondary')).toBe(true);
    expect(button.getAttribute('type')).toBe('button');
    await fireEvent.click(button);
    expect(onClick).toHaveBeenCalledTimes(1);
  });
});
