import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, test, vi } from 'vitest';
import StatsCard from '../../src/components/StatsCard.svelte';

describe('shared StatsCard', () => {
  test('shows its value and invokes its action', async () => {
    const onAction = vi.fn();
    render(StatsCard, { props: { icon: 'i-link', eyebrow: 'Network', value: '42', title: 'Providers', trend: 'Configured', actionLabel: 'View', onAction } });
    expect(screen.getByText('Network')).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Providers' })).toBeTruthy();
    expect(screen.getByText('42')).toBeTruthy();
    expect(screen.getByText('Configured')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'View' }));
    expect(onAction).toHaveBeenCalledOnce();
  });

  test('loading placeholder does not expose stale data or actions', () => {
    render(StatsCard, { props: { skeleton: true, title: 'Providers', actionLabel: 'View' } });
    expect(screen.queryByRole('heading', { name: 'Providers' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'View' })).toBeNull();
  });
});
