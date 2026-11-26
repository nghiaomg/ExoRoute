import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardCombosPage from '../../src/features/combos/DashboardCombosPage.svelte';
import { api } from '../../src/lib/api';
import { statisticsFixture } from './support/statistics';
import type { GatewayCombo, Translate } from '../../src/lib/types';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

function combo(name: string): GatewayCombo {
  return {
    id: `combo-${name}`,
    name,
    strategy: 'priority',
    accepted_protocols: ['chat_completions'],
    enabled: true,
    targets: [{ provider_id: 'provider-a', model: 'shared/model', priority: 1, enabled: true }],
  };
}

function renderPage() {
  return render(DashboardCombosPage, {
    props: {
      tr,
      locale: 'en',
      onNavigate: () => {},
      onConnectionChange: () => {},
      onProviderCountChange: () => {},
    },
  });
}

afterEach(() => {
  vi.restoreAllMocks();
  delete (HTMLDialogElement.prototype as unknown as { showModal?: () => void }).showModal;
  delete (HTMLDialogElement.prototype as unknown as { close?: () => void }).close;
});

/** StatsCard renders its heading as a direct child of the card root div. */
function statCardText(heading: HTMLElement): string {
  const text = heading.parentElement?.textContent;
  if (text === null || text === undefined) throw new Error('Stat card not found');
  return text;
}

/** StatsCard's KPI span immediately precedes its heading. */
function statCardValue(heading: HTMLElement): string {
  const value = heading.previousElementSibling?.textContent?.trim();
  if (value === null || value === undefined) throw new Error('Stat card value not found');
  return value;
}

/** Combo rows are article elements carrying their enablement state. */
function comboCardText(): string {
  const text = document.querySelector('article[data-status]')?.textContent;
  if (text === null || text === undefined) throw new Error('Combo row not found');
  return text;
}

describe('combo usage attribution', () => {
  test('opens the target provider options from inside the combo dialog', async () => {
    vi.spyOn(api, 'combos').mockResolvedValue([]);
    vi.spyOn(api, 'comboProviderOptions').mockResolvedValue({ providers: [{ id: 'provider-a', name: 'OpenAI', enabled: true }], next_cursor: null });
    vi.spyOn(api, 'statistics').mockRejectedValue(new Error('unavailable'));
    Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: function (this: HTMLDialogElement) { this.open = true; } });
    Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: function (this: HTMLDialogElement) { this.open = false; this.dispatchEvent(new Event('close')); } });

    renderPage();
    await fireEvent.click(await screen.findByRole('button', { name: 'New combo' }));
    // The unified target picker is a searchable SelectSearch: the options are
    // listbox rows rendered inside the editor dialog, and provider entries
    // carry their id as the searchable keyword.
    const targetSelect = screen.getByRole('combobox', { name: 'Search providers and combos' }) as HTMLInputElement;
    await fireEvent.click(targetSelect);
    expect(await screen.findByRole('option', { name: 'OpenAI' })).toBeTruthy();
    expect(document.querySelector('dialog.modal')?.contains(targetSelect)).toBe(true);
  });

  test('submits a nested combo target chosen from the unified picker', async () => {
    vi.spyOn(api, 'combos').mockResolvedValue([combo('fallback-chain')]);
    vi.spyOn(api, 'comboProviderOptions').mockResolvedValue({ providers: [], next_cursor: null });
    vi.spyOn(api, 'statistics').mockRejectedValue(new Error('unavailable'));
    const createCombo = vi.spyOn(api, 'createCombo').mockResolvedValue({} as GatewayCombo);
    Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: function (this: HTMLDialogElement) { this.open = true; } });
    Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: function (this: HTMLDialogElement) { this.open = false; this.dispatchEvent(new Event('close')); } });

    renderPage();
    await fireEvent.click(await screen.findByRole('button', { name: 'New combo' }));
    const targetSelect = screen.getByRole('combobox', { name: 'Search providers and combos' });
    await fireEvent.click(targetSelect);
    // Picking a combo entry turns the row into a nested-combo target without
    // any model field. The row carries the muted "combo" hint after the name.
    await fireEvent.click(await screen.findByRole('option', { name: /fallback-chain/ }));
    const form = targetSelect.closest('form') as HTMLFormElement;
    expect(form).toBeTruthy();
    const nameField = form.querySelector('input') as HTMLInputElement;
    await fireEvent.input(nameField, { target: { value: 'nested-test' } });
    await fireEvent.submit(form);
    await waitFor(() => expect(createCombo).toHaveBeenCalledTimes(1));
    const body = createCombo.mock.calls[0][0];
    expect(body.targets).toHaveLength(1);
    expect(body.targets[0].combo_id).toBe('combo-fallback-chain');
    expect(body.targets[0].provider_id).toBe('');
    expect(body.targets[0].model).toBe('');
    expect(body.targets[0].priority).toBe(1);
  });

  test('uses the route alias breakdown instead of a same-named model breakdown', async () => {
    vi.spyOn(api, 'combos').mockResolvedValue([combo('shared/model')]);
    vi.spyOn(api, 'comboProviderOptions').mockResolvedValue({ providers: [], next_cursor: null });
    vi.spyOn(api, 'statistics').mockResolvedValue(statisticsFixture({
      model_breakdown: [{
        model: 'shared/model', provider: 'provider-a', requests: 99, successes: 0,
        success_rate: 0, input_tokens: 0, output_tokens: 0, cache_hit_rate: 0,
      }],
      route_breakdown: [{
        route_alias: 'shared/model', requests: 5, successes: 4, failures: 1, success_rate: 80,
      }],
      route_breakdown_truncated: false,
      route_breakdown_incomplete: false,
    }));

    renderPage();

    const requestsCard = await screen.findByRole('heading', { name: 'Requests Routed' });
    expect(statCardText(requestsCard)).toContain('5');
    expect(statCardText(requestsCard)).not.toContain('99');

    const comboCard = comboCardText();
    expect(comboCard).toContain('5 requests');
    expect(comboCard).toContain('80% success');
    // The per-row card now shows requests and success; the routed failure
    // count is reported by the aggregate Failures stat card.
    const failuresCard = await screen.findByRole('heading', { name: 'Failures' });
    expect(statCardValue(failuresCard)).toBe('1');
  });

  test('does not report zero traffic when statistics are unavailable or incomplete', async () => {
    vi.spyOn(api, 'combos').mockResolvedValue([combo('combo-with-old-history')]);
    vi.spyOn(api, 'comboProviderOptions').mockResolvedValue({ providers: [], next_cursor: null });
    vi.spyOn(api, 'statistics').mockResolvedValue(statisticsFixture({
      route_breakdown: [],
      route_breakdown_truncated: false,
      route_breakdown_incomplete: true,
    }));

    renderPage();

    const requestsCard = await screen.findByRole('heading', { name: 'Requests Routed' });
    expect(statCardText(requestsCard)).toContain('—');
    expect(statCardText(requestsCard)).toContain('Statistics are incomplete');
    // With an incomplete window the row must not claim zero traffic either.
    expect(comboCardText()).not.toContain('no traffic yet');
    expect(comboCardText()).not.toContain('0 failed');
  });

  test('labels an empty route breakdown as no traffic only when the range is complete', async () => {
    vi.spyOn(api, 'combos').mockResolvedValue([combo('unused-route')]);
    vi.spyOn(api, 'comboProviderOptions').mockResolvedValue({ providers: [], next_cursor: null });
    vi.spyOn(api, 'statistics').mockResolvedValue(statisticsFixture({
      route_breakdown: [],
      route_breakdown_truncated: false,
      route_breakdown_incomplete: false,
    }));

    renderPage();

    await waitFor(() => expect(comboCardText()).toContain('no traffic yet'));
    const failuresCard = await screen.findByRole('heading', { name: 'Failures' });
    expect(statCardValue(failuresCard)).toBe('0');
  });
});
