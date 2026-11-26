import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import ApiKeyScopeFields from '../../src/features/api-keys/ApiKeyScopeFields.svelte';
import DashboardApiKeysPage from '../../src/features/api-keys/DashboardApiKeysPage.svelte';
import DashboardCombosPage from '../../src/features/combos/DashboardCombosPage.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type { GatewayApiKey } from '../../src/lib/types';
import { statisticsFixture } from './support/statistics';

function source(feature: string, name: string): string {
  return readFileSync(resolve(process.cwd(), 'src/features', feature, name), 'utf8');
}

const scopeSource = source('api-keys', 'ApiKeyScopeFields.svelte');
const keysPageSource = source('api-keys', 'DashboardApiKeysPage.svelte');
const combosPageSource = source('combos', 'DashboardCombosPage.svelte');

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const keys: GatewayApiKey[] = [
  { id: 'key-scope', name: 'scoped', enabled: true, created_at: '2026-11-01 00:00:00', allowed_models: ['gpt-4*'] },
];

function renderKeysPage() {
  vi.spyOn(api, 'apiKeys').mockResolvedValue({ api_keys: keys, next_cursor: null });
  vi.spyOn(api, 'statistics').mockResolvedValue(statisticsFixture({ total_requests: 10 }) as never);
  vi.spyOn(api, 'providers').mockResolvedValue([{ id: 'provider-a', name: 'OpenAI' }] as never);
  vi.spyOn(api, 'apiKeyStatistics').mockResolvedValue(statisticsFixture({ total_requests: 10 }) as never);
  return render(DashboardApiKeysPage, { props: { tr, locale: 'en', onNavigate: () => {} } });
}

function renderCombosPage() {
  vi.spyOn(api, 'combos').mockResolvedValue([]);
  vi.spyOn(api, 'comboProviderOptions').mockResolvedValue({ providers: [{ id: 'provider-a', name: 'OpenAI', enabled: true }], next_cursor: null });
  vi.spyOn(api, 'statistics').mockResolvedValue(statisticsFixture({ total_requests: 10 }) as never);
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

async function closeOpenDialog(): Promise<void> {
  const dialog = document.querySelector('dialog.modal');
  expect(dialog).toBeTruthy();
  await fireEvent(dialog as Element, new Event('close'));
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('api-keys and combos design-system standardization', () => {
  test('the three components contain no hardcoded colors or raw dialogs', () => {
    for (const [name, text] of [
      ['ApiKeyScopeFields', scopeSource],
      ['DashboardApiKeysPage', keysPageSource],
      ['DashboardCombosPage', combosPageSource],
    ] as const) {
      expect(text.match(/#[0-9a-fA-F]{3,8}\b|rgba?\(/g), name).toBeNull();
    }
    for (const [name, text] of [
      ['DashboardApiKeysPage', keysPageSource],
      ['DashboardCombosPage', combosPageSource],
    ] as const) {
      expect(text, name).not.toContain('<dialog');
      expect(text, name).toContain('<Dialog');
    }
    expect(combosPageSource).not.toContain('padding-top:14px');
  });

  test('scope fields use semantic tokens for warning and popover surfaces', () => {
    for (const token of [
      'color-mix(in srgb, var(--accent) 8%, var(--surface))',
      'var(--accent-strong)',
      'var(--accent-deep)',
      'var(--shadow-popover)',
    ]) {
      expect(scopeSource, token).toContain(token);
    }
  });

  test('the combo retry row migrates surface, border, and text together', () => {
    const row = combosPageSource.slice(
      combosPageSource.indexOf('.combo-retry-setting {'),
      combosPageSource.indexOf('.combo-retry-setting small'),
    );
    for (const token of ['var(--line)', 'var(--surface)', 'var(--ink)']) {
      expect(row, token).toContain(token);
    }
  });

  test('the combo retry row keeps a compact checkbox and a wrapping text column', () => {
    function rule(selector: string): string {
      const start = combosPageSource.indexOf(`${selector} {`);
      expect(start, selector).toBeGreaterThanOrEqual(0);
      return combosPageSource.slice(start, combosPageSource.indexOf('}', start) + 1);
    }

    const container = rule('.combo-retry-setting');
    for (const declaration of [
      'display: flex',
      'align-items: flex-start',
      'gap: 12px',
      'width: 100%',
      'box-sizing: border-box',
      'box-shadow: none',
    ]) {
      expect(container, declaration).toContain(declaration);
    }

    const checkbox = rule('.combo-retry-setting :global(input[type="checkbox"])');
    for (const declaration of ['box-sizing: border-box', 'width: 16px', 'height: 16px', 'flex: 0 0 auto']) {
      expect(checkbox, declaration).toContain(declaration);
    }
    // The shared `.modal-form input` skin must not stretch this control.
    expect(checkbox).not.toContain('width: 100%');

    const copy = rule('.combo-retry-setting span');
    for (const declaration of ['flex: 1 1 auto', 'min-width: 0']) {
      expect(copy, declaration).toContain(declaration);
    }
  });

  test('scope chips render known providers and flag deleted ones', () => {
    const { unmount } = render(ApiKeyScopeFields, {
      props: {
        tr,
        providers: [{ id: 'provider-a', name: 'OpenAI' }],
        selectedProviderIds: ['provider-a', 'missing-id'],
      },
    });
    expect(screen.getByText('OpenAI')).toBeTruthy();
    expect(screen.getByText('Deleted')).toBeTruthy();
    expect(document.querySelectorAll('.scope-chip.missing')).toHaveLength(1);
    unmount();
  });

  test('empty scope selection reports all providers', () => {
    render(ApiKeyScopeFields, { props: { tr, providers: [], selectedProviderIds: [] } });
    expect(screen.getByText('All providers')).toBeTruthy();
    expect(screen.getByText('All models')).toBeTruthy();
    expect(document.querySelector('textarea')).toBeTruthy();
  });

  test('API-key create dialog opens from the page and closes on the native close event', async () => {
    renderKeysPage();
    await fireEvent.click(await screen.findByRole('button', { name: 'New API key' }));
    expect(await screen.findByRole('heading', { name: 'New API key' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeTruthy();
    await closeOpenDialog();
    await waitFor(() => expect(document.querySelector('dialog.modal')).toBeNull());
  });

  test('API-key delete confirmation opens and closes on the native close event', async () => {
    renderKeysPage();
    await fireEvent.click(await screen.findByRole('button', { name: 'Delete permanently' }));
    expect(await screen.findByRole('heading', { name: 'Delete API key?' })).toBeTruthy();
    await closeOpenDialog();
    await waitFor(() => expect(document.querySelector('dialog.modal')).toBeNull());
  });

  test('combo dialog opens with a working retry-until-success control', async () => {
    renderCombosPage();
    await fireEvent.click(await screen.findByRole('button', { name: 'New combo' }));
    expect(await screen.findByRole('heading', { name: 'New combo' })).toBeTruthy();
    const retry = await screen.findByRole('checkbox', { name: /Retry until success/ });
    const setting = retry.closest('.combo-retry-setting') as HTMLElement;
    expect(setting).toBeTruthy();
    // A single text wrapper sits next to the checkbox and holds both the title
    // and the description, so the copy can wrap inside the card.
    expect(retry.parentElement).toBe(setting);
    const copy = setting.querySelector('span');
    expect(copy?.parentElement).toBe(setting);
    expect(copy?.querySelector('strong')?.textContent).toContain('Retry until success');
    expect(copy?.querySelector('small')?.textContent).toContain('After all targets fail');
    expect((retry as HTMLInputElement).checked).toBe(false);
    await fireEvent.click(retry);
    expect((retry as HTMLInputElement).checked).toBe(true);
    await closeOpenDialog();
    await waitFor(() => expect(document.querySelector('dialog.modal')).toBeNull());
  });
});
