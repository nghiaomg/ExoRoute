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
 * The shared SelectOptions control is a native `<select>`: picking an option
 * is a normal change event, matched by the option's visible label.
 */
async function chooseOption(select: HTMLSelectElement, label: string): Promise<void> {
  const option = Array.from(select.options).find((candidate) => candidate.textContent?.trim() === label);
  if (!option) throw new Error(`the shared select has no option labelled "${label}"`);
  await fireEvent.change(select, { target: { value: option.value } });
}

function loadedLimits(): ReturnType<typeof vi.spyOn> {
  return vi.spyOn(api, 'gatewayResourceLimits').mockResolvedValue(limits(savedLimits));
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('the gateway resource limits panel', () => {
  test('the body-processing limit is the shared native select', async () => {
    loadedLimits();

    render(GatewayResourceLimitsPanel, { props: { tr } });

    const select = await screen.findByRole('combobox', { name: 'Concurrent bodies being processed' }) as HTMLSelectElement;
    expect(select.tagName).toBe('SELECT');
    expect(select.value).toBe('4');
    expect(select.options[select.selectedIndex]?.textContent).toContain('4 requests');
  });

  test('the chosen concurrency is what the panel saves', async () => {
    loadedLimits();
    const update = vi.spyOn(api, 'updateGatewayResourceLimits').mockResolvedValue(limits({ ...savedLimits, gateway_body_processing_concurrency: 8 }));

    render(GatewayResourceLimitsPanel, { props: { tr } });

    const select = await screen.findByRole('combobox', { name: 'Concurrent bodies being processed' }) as HTMLSelectElement;
    await chooseOption(select, '8 requests');
    await fireEvent.click(screen.getByRole('button', { name: 'Save settings' }));

    await waitFor(() => expect(update).toHaveBeenCalled());
    expect(update.mock.calls[0][0]).toMatchObject({ gateway_body_processing_concurrency: 8 });
    expect(update.mock.calls[0][1]).toMatchObject({ gateway_body_processing_concurrency: 4 });
  });

  test('a saved value of zero reads as unlimited', async () => {
    vi.spyOn(api, 'gatewayResourceLimits').mockResolvedValue(limits({ ...savedLimits, gateway_body_processing_concurrency: 0 }));

    render(GatewayResourceLimitsPanel, { props: { tr } });

    const select = await screen.findByRole('combobox', { name: 'Concurrent bodies being processed' }) as HTMLSelectElement;
    expect(select.value).toBe('0');
    expect(select.options[select.selectedIndex]?.textContent).toContain('Unlimited');
  });

  test('stream continuity is the shared checkbox and saves what it reports', async () => {
    loadedLimits();
    const update = vi.spyOn(api, 'updateGatewayResourceLimits').mockResolvedValue(limits({ ...savedLimits, stream_continuity_enabled: true }));

    render(GatewayResourceLimitsPanel, { props: { tr } });

    const input = await screen.findByRole('checkbox', { name: 'Stream continuity' }) as HTMLInputElement;
    expect(input.tagName).toBe('INPUT');
    expect(input.checked).toBe(false);
    expect(screen.getByText('Disabled')).toBeTruthy();

    await fireEvent.click(input);

    expect(input.checked).toBe(true);
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

    const select = await screen.findByRole('combobox', { name: 'Terse prose style level' }) as HTMLSelectElement;
    expect(select.tagName).toBe('SELECT');
    expect(select.hasAttribute('disabled')).toBe(true);

    await fireEvent.click(await screen.findByRole('checkbox', { name: 'Terse prose' }));

    expect(select.hasAttribute('disabled')).toBe(false);
    await chooseOption(select, 'Ultra');
    await fireEvent.click(screen.getByRole('button', { name: 'Save output styles' }));

    await waitFor(() => expect(update).toHaveBeenCalled());
    expect(update.mock.calls[0][0]).toEqual([{ id: 'terse-prose', level: 'ultra' }]);
    expect(update.mock.calls[0][1]).toBe(7);
  });
});
