import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import DevinPage from '../../src/features/devin/DevinPage.svelte';
import type { Translate } from '../../src/lib/format';

const tr: Translate = (key, vars) => key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const devinSettings = vi.fn();
const devinSaveSettings = vi.fn();
const devinDeleteSettings = vi.fn();
const devinVerify = vi.fn();
const devinListSessions = vi.fn();

vi.mock('../../src/lib/api', () => ({
  api: {
    devinSettings: (...args: unknown[]) => devinSettings(...args),
    devinSaveSettings: (...args: unknown[]) => devinSaveSettings(...args),
    devinDeleteSettings: (...args: unknown[]) => devinDeleteSettings(...args),
    devinVerify: (...args: unknown[]) => devinVerify(...args),
    devinListSessions: (...args: unknown[]) => devinListSessions(...args),
    devinCreateSession: vi.fn(),
    devinGetSession: vi.fn(),
    devinSendMessage: vi.fn(),
  },
}));

function sessionFixture(overrides: Record<string, unknown> = {}) {
  return {
    session_id: 'devin-abc123',
    status: 'running',
    title: 'Fix the login flow',
    created_at: '2026-01-02T03:04:05Z',
    pull_request: { url: 'https://github.com/org/repo/pull/7' },
    ...overrides,
  };
}

function renderDevin() {
  return render(DevinPage, { props: { tr, locale: 'en', onConnectionChange: vi.fn() } });
}

beforeEach(() => {
  vi.clearAllMocks();
  devinSettings.mockResolvedValue({ configured: true, master_key_configured: true });
  devinListSessions.mockResolvedValue([sessionFixture()]);
});

afterEach(() => {
  // The page schedules a poll interval on mount; stop the clock so the
  // unmounted component never fires it.
  vi.useRealTimers();
});

describe('devin panel', () => {
  test('renders the stored-key hint and the session table', async () => {
    renderDevin();

    expect(await screen.findByText('A Devin API key is stored. Enter a new key to replace it.')).toBeTruthy();
    const table = await screen.findByRole('table');
    expect(table.textContent).toContain('running');
    expect(table.textContent).toContain('Fix the login flow');
    const prLink = screen.getByRole('link', { name: 'Pull request' }) as HTMLAnchorElement;
    expect(prLink.href).toBe('https://github.com/org/repo/pull/7');
    expect(devinListSessions).toHaveBeenCalledWith(50, 0);
  });

  test('the verify button surfaces the live key check result', async () => {
    devinVerify.mockResolvedValue({ verified: true, status: 200, detail: null });
    renderDevin();

    await screen.findByRole('table');
    await fireEvent.click(screen.getByRole('button', { name: 'Verify key' }));

    await waitFor(() => {
      expect(screen.getByText('Key verified. The Devin API is reachable.')).toBeTruthy();
    });
    expect(devinVerify).toHaveBeenCalledTimes(1);
  });

  test('create stays disabled until the prompt is filled and the message dialog opens from a row', async () => {
    renderDevin();
    await screen.findByRole('table');

    const createButton = screen.getByRole('button', { name: 'Create session' }) as HTMLButtonElement;
    expect(createButton.disabled).toBe(true);

    await fireEvent.click(screen.getByRole('button', { name: 'Send message' }));
    expect(await screen.findByText('Message to Devin')).toBeTruthy();

    const cancelButton = screen.getByRole('button', { name: 'Cancel' });
    await fireEvent.click(cancelButton);
    await waitFor(() => {
      expect(screen.queryByText('Message to Devin')).toBeNull();
    });
  });

  test('without a configured key the empty state explains what to do', async () => {
    devinSettings.mockResolvedValue({ configured: false, master_key_configured: false });
    devinListSessions.mockResolvedValue([]);
    renderDevin();

    expect(await screen.findByText('Set EXOROUTE_MASTER_KEY to store the Devin API key.')).toBeTruthy();
    expect(await screen.findByText('No Devin API key is stored yet.')).toBeTruthy();
    expect((screen.getByRole('button', { name: 'Verify key' }) as HTMLButtonElement).disabled).toBe(true);
  });
});
