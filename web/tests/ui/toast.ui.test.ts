import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, test, vi } from 'vitest';
import Toast from '../../src/components/Toast.svelte';
import type { Translate } from '../../src/lib/format';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

describe('toast', () => {
  test('announces a success toast with its detail line', async () => {
    render(Toast, {
      props: {
        toast: {
          tone: 'success',
          title: 'Access scope saved.',
          message: 'Requests now follow the saved providers.',
        },
        tr,
        onDismiss: () => {},
      },
    });

    const toast = await screen.findByRole('status');
    expect(toast.classList.contains('app-toast')).toBe(true);
    expect(toast.classList.contains('success')).toBe(true);
    expect(toast.textContent).toContain('Access scope saved.');
    expect(toast.textContent).toContain('Requests now follow the saved providers.');
  });

  test('renders a title-only error toast without a detail line', async () => {
    const { container } = render(Toast, {
      props: { toast: { tone: 'error', title: 'Could not save.' }, tr, onDismiss: () => {} },
    });

    const toast = await screen.findByRole('status');
    expect(toast.classList.contains('error')).toBe(true);
    expect(toast.textContent).toContain('Could not save.');
    expect(container.querySelector('.app-toast-copy small')).toBeNull();
  });

  test('the dismiss button reports the dismissal to the owner', async () => {
    const onDismiss = vi.fn();
    render(Toast, {
      props: { toast: { tone: 'success', title: 'Saved.' }, tr, onDismiss },
    });

    await fireEvent.click(await screen.findByRole('button', { name: 'Dismiss notification' }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  test('renders nothing while no toast is active', () => {
    const { container } = render(Toast, { props: { toast: null, tr, onDismiss: () => {} } });
    expect(container.querySelector('.app-toast')).toBeNull();
  });
});
