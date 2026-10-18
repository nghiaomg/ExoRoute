import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import GatewayResourceLimitsPanel from '../../src/features/settings/GatewayResourceLimitsPanel.svelte';
import OutputStylesPanel from '../../src/features/settings/OutputStylesPanel.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type {
  GatewayResourceLimitValues,
  GatewayResourceLimits,
  OutputStylesSnapshot,
} from '../../src/lib/types';

const tr: Translate = (key, vars) => key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const savedLimits: GatewayResourceLimitValues = {
  gateway_body_limit_mib: 16,
  gateway_body_processing_concurrency: 4,
  sse_frame_limit_kib: 1024,
  sse_buffer_limit_kib: 2048,
  provider_max_concurrency: 32,
  stream_continuity_enabled: false,
  stream_continuity_max_concurrency: 16,
};

function limits(values: GatewayResourceLimitValues): GatewayResourceLimits {
  return { active: { ...values }, saved: { ...values }, restart_required: false };
}

/**
 * The shared controls are Ark widgets. Their hidden native element carries the
 * accessible name, so a test clicks the box the user sees rather than that
 * hidden input, which real pointers cannot reach.
 */
function arkControl(input: HTMLElement): Element {
  const control = input.closest('[data-scope="checkbox"]')?.querySelector('[data-part="control"]');
  if (!control) throw new Error('the shared checkbox did not render its control');
  return control;
}

async function toggle(input: HTMLElement): Promise<void> {
  await fireEvent.click(arkControl(input));
}

/** Opens a shared select and picks one of its options by visible label. */
async function chooseOption(trigger: HTMLElement, label: string): Promise<void> {
  await fireEvent.click(trigger);
  await fireEvent.click(await screen.findByRole('option', { name: label }));
}

function loadedLimits(): ReturnType<typeof vi.spyOn> {
  return vi.spyOn(api, 'gatewayResourceLimits').mockResolvedValue(limits(savedLimits));
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('the gateway resource limits panel', () => {
  test('the body-processing limit is the shared select, not a native one', async () => {
    loadedLimits();

    render(GatewayResourceLimitsPanel, { props: { tr } });

    const trigger = await screen.findByRole('combobox', { name: 'Concurrent bodies being processed' });
    // A native `<select>` also answers to the combobox role, so the tag is what
    // tells the shared control apart from the element it replaced.
    expect(trigger.tagName).toBe('BUTTON');
    expect(trigger.closest('.ark-select-root')).not.toBeNull();
    expect(trigger.textContent).toContain('4 requests');
  });

  test('the chosen concurrency is what the panel saves', async () => {
    loadedLimits();
    const update = vi.spyOn(api, 'updateGatewayResourceLimits').mockResolvedValue(limits({ ...savedLimits, gateway_body_processing_concurrency: 8 }));

    render(GatewayResourceLimitsPanel, { props: { tr } });

    const trigger = await screen.findByRole('combobox', { name: 'Concurrent bodies being processed' });
    await chooseOption(trigger, '8 requests');
    await fireEvent.click(screen.getByRole('button', { name: 'Save settings' }));

    await waitFor(() => expect(update).toHaveBeenCalled());
    expect(update.mock.calls[0][0]).toMatchObject({ gateway_body_processing_concurrency: 8 });
    expect(update.mock.calls[0][1]).toMatchObject({ gateway_body_processing_concurrency: 4 });
  });

  test('a saved value of zero reads as unlimited', async () => {
    vi.spyOn(api, 'gatewayResourceLimits').mockResolvedValue(limits({ ...savedLimits, gateway_body_processing_concurrency: 0 }));

    render(GatewayResourceLimitsPanel, { props: { tr } });

    const trigger = await screen.findByRole('combobox', { name: 'Concurrent bodies being processed' });
    expect(trigger.textContent).toContain('Unlimited');
  });

  test('stream continuity is the shared checkbox and saves what it reports', async () => {
    loadedLimits();
    const update = vi.spyOn(api, 'updateGatewayResourceLimits').mockResolvedValue(limits({ ...savedLimits, stream_continuity_enabled: true }));

    render(GatewayResourceLimitsPanel, { props: { tr } });

    const input = await screen.findByRole('checkbox', { name: 'Stream continuity' });
    expect(input.tagName).toBe('INPUT');
    expect(input.closest('.ark-checkbox-root')).not.toBeNull();
    expect(screen.getByText('Disabled')).toBeTruthy();

    await toggle(input);

    expect(screen.getByText('Enabled')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Save settings' }));

    await waitFor(() => expect(update).toHaveBeenCalled());
    expect(update.mock.calls[0][0]).toMatchObject({ stream_continuity_enabled: true });
  });
});

describe('the output styles panel', () => {
  const snapshot: OutputStylesSnapshot = { styles: [], revision: 7, overridden: false, source: 'default' };

  test('a style level is chosen through the shared select, which waits for its style', async () => {
    vi.spyOn(api, 'outputStyles').mockResolvedValue(snapshot);
    const update = vi.spyOn(api, 'updateOutputStyles').mockResolvedValue({
      ...snapshot,
      styles: [{ id: 'terse-prose', level: 'ultra' }],
      revision: 8,
      overridden: true,
      source: 'database',
    });

    render(OutputStylesPanel, { props: { tr } });

    const trigger = await screen.findByRole('combobox', { name: 'Terse prose style level' });
    expect(trigger.tagName).toBe('BUTTON');
    expect(trigger.closest('.ark-select-root')).not.toBeNull();
    expect(trigger.hasAttribute('data-disabled')).toBe(true);

    await toggle(await screen.findByRole('checkbox', { name: 'Terse prose' }));

    expect(trigger.hasAttribute('data-disabled')).toBe(false);
    await chooseOption(trigger, 'Ultra');
    await fireEvent.click(screen.getByRole('button', { name: 'Save output styles' }));

    await waitFor(() => expect(update).toHaveBeenCalled());
    expect(update.mock.calls[0][0]).toEqual([{ id: 'terse-prose', level: 'ultra' }]);
    expect(update.mock.calls[0][1]).toBe(7);
  });
});
