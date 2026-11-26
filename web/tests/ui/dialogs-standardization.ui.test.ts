import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DashboardChatPage from '../../src/features/chat/DashboardChatPage.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';
import type { WorkspaceChatModelsResult } from '../../src/lib/types';

const SOURCES = [
  'src/features/providers/DashboardProvidersPage.svelte',
  'src/features/overview/DashboardOverviewPage.svelte',
  'src/features/chat/DashboardChatPage.svelte',
  'src/features/settings/DashboardSettingsPage.svelte',
  'src/features/quota/DashboardQuotaPage.svelte',
];

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const modelsResult: WorkspaceChatModelsResult = {
  models: [{ provider_id: 'openrouter', model: 'test-model', id: 'ocg/test-model' }],
  truncated: false,
};

afterEach(() => {
  vi.restoreAllMocks();
});

describe('dialog standardization', () => {
  test('no raw <dialog> tags or inline modal padding remain in migrated pages', () => {
    for (const relative of SOURCES) {
      const source = readFileSync(resolve(process.cwd(), relative), 'utf8');
      expect(source.match(/<dialog\b/g), `${relative}: raw <dialog>`).toBeNull();
      expect(source.match(/padding-top:14px/g), `${relative}: inline padding`).toBeNull();
      expect(source, `${relative}: shared Dialog import`).toContain('components/ui/Dialog.svelte');
    }
  });

  test('Parameters opens the shared Dialog with its content and closes cleanly', async () => {
    vi.spyOn(api, 'chatModels').mockResolvedValue(modelsResult);
    render(DashboardChatPage, { props: { tr, locale: 'en', onConnectionChange: () => {} } });

    await screen.findByPlaceholderText('Select a model');
    expect(document.querySelector('dialog[open]')).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Parameters' }));

    const dialog = await screen.findByRole('dialog');
    expect(dialog.textContent).toContain('Chat parameters');
    expect(dialog.querySelector('textarea[placeholder="You are a helpful routing assistant…"]')).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(document.querySelector('dialog[open]')).toBeNull());
  });
});
