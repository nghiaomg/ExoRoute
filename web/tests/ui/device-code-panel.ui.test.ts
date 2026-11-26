import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import DeviceCodeOAuthPanel from '../../src/features/providers/DeviceCodeOAuthPanel.svelte';
import { api } from '../../src/lib/api';
import type { Translate } from '../../src/lib/format';

const tr: Translate = (key, vars) => key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const PROVIDER_ID = 'kilocode-provider';

function stubPopup(): { closed: boolean; close: () => void; location: { href: string } } {
  const popup = { closed: false, close: vi.fn(), location: { href: 'about:blank' } };
  vi.spyOn(window, 'open').mockImplementation(() => popup as unknown as Window);
  return popup;
}

function stubClipboard(): ReturnType<typeof vi.fn> {
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText },
  });
  return writeText;
}

function renderPanel(onConnected: () => void) {
  render(DeviceCodeOAuthPanel, {
    props: {
      providerId: PROVIDER_ID,
      providerAdapterId: 'kilocode',
      providerName: 'Kilo Code',
      open: true,
      tr,
      onConnected,
    },
  });
}

function startDeviceFlow(intervalSeconds: number) {
  return vi.spyOn(api, 'startProviderAuth').mockResolvedValue({
    flow_id: 'flow-1',
    authorization_url: 'https://app.kilo.ai/device',
    method: 'device_code',
    user_code: 'code-1',
    expires_in: 300,
    interval: intervalSeconds,
  });
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('DeviceCodeOAuthPanel', () => {
  test('shows the approval code, keeps polling, and reports the connected account', async () => {
    startDeviceFlow(1);
    vi.spyOn(api, 'pollProviderAuth')
      .mockResolvedValueOnce({ status: 'pending', message: 'Waiting for approval' })
      .mockResolvedValue({ status: 'connected' });
    stubPopup();
    const onConnected = vi.fn();

    renderPanel(onConnected);

    await fireEvent.click(await screen.findByRole('button', { name: 'Connect account' }));

    // The operator needs the code and a link even when the popup is blocked.
    expect(
      await screen.findByText('Enter this code on the Kilo Code page to finish sign-in.'),
    ).toBeTruthy();
    expect(screen.getByText('code-1')).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Open authorization page' }).getAttribute('href')).toBe(
      'https://app.kilo.ai/device',
    );

    await waitFor(
      () => {
        expect(onConnected).toHaveBeenCalledTimes(1);
      },
      { timeout: 6000 },
    );
    expect(
      screen.getByText('Provider sign-in completed and the account is connected.'),
    ).toBeTruthy();
    // The code and link are cleared once the account is connected.
    expect(screen.queryByText('code-1')).toBeNull();
    expect(api.pollProviderAuth).toHaveBeenCalledWith(PROVIDER_ID, 'flow-1');
  }, 15000);

  test('surfaces a denied sign-in and offers to start again', async () => {
    startDeviceFlow(1);
    vi.spyOn(api, 'pollProviderAuth').mockResolvedValue({
      status: 'failed',
      message: 'The Kilo Code request was denied; start sign-in again',
    });
    stubPopup();

    renderPanel(vi.fn());

    await fireEvent.click(await screen.findByRole('button', { name: 'Connect account' }));

    expect(
      await screen.findByText('The Kilo Code request was denied; start sign-in again', {}, { timeout: 6000 }),
    ).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Start again' })).toBeTruthy();
  }, 15000);

  test('copies the approval code from the dashboard', async () => {
    startDeviceFlow(3000);
    vi.spyOn(api, 'pollProviderAuth').mockResolvedValue({ status: 'pending' });
    stubPopup();
    const writeText = stubClipboard();

    renderPanel(vi.fn());

    await fireEvent.click(await screen.findByRole('button', { name: 'Connect account' }));
    await screen.findByText('code-1');
    await fireEvent.click(screen.getByRole('button', { name: 'Copy code' }));

    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith('code-1');
    });
    expect(await screen.findByRole('button', { name: 'Code copied' })).toBeTruthy();
  }, 15000);
});
