import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardRequestsPage from '../../src/features/requests/DashboardRequestsPage.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type { RequestLog } from '../../src/lib/types';
import { statisticsFixture } from './support/statistics';

function source(name: string): string {
  return readFileSync(resolve(process.cwd(), 'src/features/requests', name), 'utf8');
}

const pageSource = source('DashboardRequestsPage.svelte');
const menuSource = source('RequestColumnVisibilityMenu.svelte');

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const row: RequestLog = {
  id: 'log-theme-1',
  model: 'openai/gpt-5',
  created_at: new Date(Date.now() - 60_000).toISOString(),
  status: 200,
  duration_ms: 120,
  input_tokens: 100,
  output_tokens: 20,
};

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

describe('requests design-system standardization', () => {
  test('neither component contains hardcoded hex or rgba colors', () => {
    for (const [name, text] of [
      ['DashboardRequestsPage', pageSource],
      ['RequestColumnVisibilityMenu', menuSource],
    ] as const) {
      expect(text.match(/#[0-9a-fA-F]{3,8}\b|rgba?\(/g), name).toBeNull();
    }
  });

  test('the detail view uses the shared Dialog primitive and canonical spacing', () => {
    expect(pageSource).toContain("import Dialog from \"../../components/ui/Dialog.svelte\"");
    expect(pageSource).toContain('<Dialog');
    expect(pageSource).not.toContain('<dialog');
    expect(pageSource).not.toContain('padding-top:14px');
  });

  test('the visibility menu uses shared Checkbox and theme tokens only', () => {
    expect(menuSource).toContain('Checkbox.svelte');
    expect(menuSource).toContain('<Checkbox');
    expect(menuSource).not.toContain('req-vis-checkbox');
    expect(menuSource).not.toContain("data-theme='dark'");
    for (const token of ['var(--line)', 'var(--surface)', 'var(--ink)', 'var(--shadow-popover)', 'var(--accent)']) {
      expect(menuSource, token).toContain(token);
    }
  });

  test('the migrated Dialog still opens from a row and preserves the label hook', async () => {
    vi.spyOn(api, 'requests').mockResolvedValue({ requests: [row], next_cursor: null, page_size: 50 });
    vi.spyOn(api, 'providers').mockResolvedValue([]);
    vi.spyOn(api, 'statistics').mockResolvedValue(statisticsFixture());
    vi.spyOn(api, 'streamRequestLive').mockImplementation(async () => {});

    render(DashboardRequestsPage, {
      props: { tr, locale: 'en', onConnectionChange: () => {} },
    });

    const tableRow = (await screen.findByText('openai/gpt-5')).closest('tr');
    expect(tableRow).not.toBeNull();
    await fireEvent.click(tableRow as HTMLElement);

    const dialog = await screen.findByRole('dialog');
    expect((dialog as HTMLDialogElement).open).toBe(true);
    expect(dialog.getAttribute('aria-labelledby')).toBe('req-dialog-title');
    expect(dialog.querySelector('.modal-title')?.textContent).toContain('Request detail');
    expect(dialog.textContent).toContain('Request info');
  });
});
