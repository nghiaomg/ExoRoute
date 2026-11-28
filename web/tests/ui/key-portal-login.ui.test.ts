import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, test, vi } from 'vitest';
import LoginPage from '../../src/features/auth/LoginPage.svelte';
import KeyPortalPage from '../../src/features/key-portal/KeyPortalPage.svelte';
import { KeyPortalError, keyPortalApi, type KeyPortalSession, type KeyPortalStats } from '../../src/lib/api/keyPortal';
import type { Translate } from '../../src/lib/format';

const tr: Translate = (key, vars) =>
  key.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? `{${name}}`));

const preferences = {
  locale: 'en' as const,
  theme: 'light' as const,
  scheme: 'light' as const,
  setLocale: () => {},
  toggleTheme: () => {},
};

const key = {
  id: 'key-a',
  name: 'Staging app',
  prefix: 'exo_key-a',
  enabled: true,
  allowed_providers: ['openai'],
  allowed_models: ['gpt-4o', 'claude-*'],
  created_at: '2026-11-01 00:00:00',
  last_used_at: '2026-11-27 00:00:00',
};

const session: KeyPortalSession = { authenticated: true, key };

const stats: KeyPortalStats = {
  key,
  summary: {
    total_requests: 128,
    success_requests: 124,
    failed_requests: 4,
    success_rate: 0.969,
    input_tokens: 12000,
    output_tokens: 3000,
    cached_tokens: 2000,
    total_tokens: 17000,
    total_cost_micro_usd: 125000,
    avg_latency_ms: 220,
  },
  models: [{
    model: 'gpt-4o', provider: 'openai', requests: 128, failed: 4,
    total_tokens: 17000, cost_micro_usd: 125000, avg_latency_ms: 220,
  }],
  recent_requests: [{
    id: 'req-1', timestamp: '2026-11-27T00:00:00Z', model: 'gpt-4o', provider: 'openai',
    status_code: 200, input_tokens: 100, output_tokens: 20, cached_tokens: 0,
    latency_ms: 180, cost_micro_usd: 1000,
  }],
};

function renderLogin(onKeyPortalLogin = vi.fn()) {
  const result = render(LoginPage, {
    props: {
      tr,
      preferences,
      mustChangePassword: false,
      currentPassword: '',
      onLogin: vi.fn(),
      onPasswordChanged: vi.fn(),
      onKeyPortalLogin,
    },
  });
  return { ...result, onKeyPortalLogin };
}

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  sessionStorage.clear();
});

describe('API key login and read-only key portal', () => {
  test('segmented switch exposes both modes and preserves the admin form by default', () => {
    renderLogin();
    const group = screen.getByRole('group', { name: 'Sign in method' });
    const [admin, apiKey] = Array.from(group.querySelectorAll('button'));
    expect(admin.getAttribute('aria-pressed')).toBe('true');
    expect(apiKey.getAttribute('aria-pressed')).toBe('false');
    expect(screen.getByLabelText('Admin password')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeTruthy();
  });

  test('API key mode submits the key once and never stores it', async () => {
    const login = vi.spyOn(keyPortalApi, 'login').mockResolvedValue(session);
    const onKeyPortalLogin = vi.fn();
    renderLogin(onKeyPortalLogin);
    await fireEvent.click(screen.getByRole('button', { name: 'API key' }));
    const input = screen.getByPlaceholderText('exo_...') as HTMLInputElement;
    expect(input.getAttribute('autocomplete')).toBe('off');
    await fireEvent.input(input, { target: { value: 'exo_secret_value' } });
    await fireEvent.click(screen.getByRole('button', { name: 'View stats' }));
    await waitFor(() => expect(login).toHaveBeenCalledWith('exo_secret_value'));
    expect(onKeyPortalLogin).toHaveBeenCalledWith(session);
    expect(JSON.stringify(localStorage)).not.toContain('exo_secret_value');
    expect(JSON.stringify(sessionStorage)).not.toContain('exo_secret_value');
  });

  test('401 login failure surfaces an inline alert', async () => {
    vi.spyOn(keyPortalApi, 'login').mockRejectedValue(new KeyPortalError('bad key', 401));
    renderLogin();
    await fireEvent.click(screen.getByRole('button', { name: 'API key' }));
    await fireEvent.input(screen.getByPlaceholderText('exo_...'), { target: { value: 'exo_bad' } });
    await fireEvent.click(screen.getByRole('button', { name: 'View stats' }));
    expect(await screen.findByRole('alert')).toBeTruthy();
    expect((screen.getByPlaceholderText('exo_...') as HTMLInputElement).value).toBe('');
  });

  test('portal renders KPIs, breakdown, recent requests and sign out', async () => {
    const load = vi.spyOn(keyPortalApi, 'stats').mockResolvedValue(stats);
    const onSignOut = vi.fn();
    render(KeyPortalPage, { props: { tr, locale: 'en', session, onSignOut } });
    expect(await screen.findByText('128')).toBeTruthy();
    expect(screen.getByLabelText('Time range')).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Model breakdown' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Recent requests' })).toBeTruthy();
    expect(screen.getAllByText('gpt-4o').length).toBeGreaterThan(0);
    expect(screen.getByText('200')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: '7d' }));
    await waitFor(() => expect(load).toHaveBeenCalledWith('7d'));
    await fireEvent.click(screen.getByRole('button', { name: 'Sign out' }));
    expect(onSignOut).toHaveBeenCalledTimes(1);
  });
});
