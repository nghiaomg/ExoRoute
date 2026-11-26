import { render, screen } from '@testing-library/svelte';
import { describe, expect, test } from 'vitest';
import StatsCard from '../../src/components/ui/StatsCard.svelte';

describe('shared StatsCard', () => {
  test('renders the indexed eyebrow, badge, value, title and trend', () => {
    const { container } = render(StatsCard, {
      props: {
        eyebrow: 'Network',
        index: 2,
        badge: 'Live',
        badgeTone: 'pill',
        value: '42',
        unit: 'ms',
        title: 'Providers',
        trend: 'Configured',
      },
    });

    expect(screen.getByText('02 / Network')).toBeTruthy();
    expect(screen.getByText('Live')).toBeTruthy();
    const kpi = screen.getByText('42');
    expect(screen.getByText('ms')).toBeTruthy();
    expect(kpi.textContent).toContain('ms');
    expect(screen.getByRole('heading', { name: 'Providers', level: 2 })).toBeTruthy();
    expect(screen.getByText('Configured')).toBeTruthy();
    expect(container.querySelector('.stat-title')?.textContent).toBe('Providers');
    // The details-link action and icon were removed by design; no button remains.
    expect(screen.queryByRole('button')).toBeNull();
  });

  test('renders a status-dot badge as a titled dot with no badge text', () => {
    render(StatsCard, { props: { eyebrow: 'Health', badge: 'Healthy', badgeTone: 'dot' } });
    expect(screen.getByTitle('Healthy')).toBeTruthy();
    expect(screen.queryByText('Healthy')).toBeNull();
  });

  test('loading placeholder hides all content and keeps its requested height', () => {
    const { container } = render(StatsCard, {
      props: {
        skeleton: true,
        skeletonHeight: 180,
        eyebrow: 'Network',
        index: 2,
        badge: 'Live',
        value: '42',
        title: 'Providers',
        trend: 'Configured',
      },
    });

    const placeholder = container.firstElementChild as HTMLElement;
    expect(placeholder.getAttribute('aria-hidden')).toBe('true');
    expect(placeholder.style.minHeight).toBe('180px');
    expect(screen.queryByText('02 / Network')).toBeNull();
    expect(screen.queryByText('42')).toBeNull();
    expect(screen.queryByRole('heading', { name: 'Providers' })).toBeNull();
    expect(screen.queryByText('Configured')).toBeNull();
    expect(screen.queryByRole('button')).toBeNull();
  });
});
