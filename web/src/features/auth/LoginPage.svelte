<script lang="ts">
  import Button from "../../components/ui/Button.svelte";
  import { Check, LoaderCircle, Moon, Sun } from '@lucide/svelte';
  import PasswordInput from '../../components/ui/PasswordInput.svelte';
  import Input from '../../components/ui/Input.svelte';
  import FlyingFishLogo from '../../components/layout/FlyingFishLogo.svelte';
  import LanguageSelect from '../../components/layout/LanguageSelect.svelte';
  import AdminPasswordChangeForm from './AdminPasswordChangeForm.svelte';
  import { api, type AdminAccessResult, type AdminLoginResult } from '../../lib/api';
  import { keyPortalApi, KeyPortalError, type KeyPortalSession } from '../../lib/api/keyPortal';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { Locale } from '../../lib/i18n';
  import type { ThemeId, ThemeScheme } from '../../lib/theme';
  import { DASHBOARD_VERSION } from '../../lib/version';
  import { docsCopy } from '../docs/docsCopy';

  interface Preferences {
    locale: Locale;
    theme: ThemeId;
    scheme: ThemeScheme;
    setLocale: (locale: Locale) => void;
    toggleTheme: () => void;
  }

  export let tr: Translate;
  export let preferences: Preferences;
  export let mustChangePassword = false;
  export let currentPassword = '';
  export let onLogin: (result: AdminLoginResult, enteredPassword: string) => void;
  export let onPasswordChanged: (result: AdminAccessResult) => void;
  export let onKeyPortalLogin: (session: KeyPortalSession) => void = () => {};
  export let sessionCheckStatus: 'checking' | 'rate_limited' | 'unavailable' | null = null;
  export let sessionRetryAfterSeconds = 0;
  export let onRetrySessionCheck: () => Promise<void> = async () => {};

  let passwordDraft = '';
  let apiKeyDraft = '';
  let loginMode: 'admin' | 'key' = 'admin';
  let saving = false;
  let errorMessage = '';

  function switchMode(mode: 'admin' | 'key'): void {
    loginMode = mode;
    errorMessage = '';
  }

  async function signInWithKey(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    errorMessage = '';
    saving = true;
    try {
      const session = await keyPortalApi.login(apiKeyDraft);
      onKeyPortalLogin(session);
    } catch (error) {
      errorMessage = error instanceof KeyPortalError && error.status === 401
        ? tr('The API key was not accepted. Check the key and try again.')
        : localizedError(error, 'Could not open the key portal. Try again.', tr);
    } finally {
      apiKeyDraft = '';
      saving = false;
    }
  }

  async function signIn(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    errorMessage = '';
    saving = true;
    try {
      const enteredPassword = passwordDraft;
      const result = await api.adminLogin(enteredPassword);
      passwordDraft = '';
      onLogin(result, enteredPassword);
    } catch (error) {
      errorMessage = localizedError(error, 'Admin access was denied. Check the password and try again.', tr);
    } finally {
      saving = false;
    }
  }

</script>

<main class="login-screen">
  <section class="login-card" aria-labelledby="login-title">
    <div class="login-topline">
      <a class="login-brand" href="/login" aria-label={tr('ExoRoute')}>
        <span class="brand-mark"><FlyingFishLogo size={24} variant="mark" /></span>
        <span class="brand-word">exo<span>route</span></span>
        <span class="brand-version">{DASHBOARD_VERSION}</span>
      </a>
      <div class="preference-controls" aria-label={tr('Language and appearance')}>
        <LanguageSelect locale={preferences.locale} {tr} onLocaleChange={preferences.setLocale} />
        <Button class="preference-button theme-button" aria-label={tr(preferences.scheme === 'light' ? 'Switch to dark mode' : 'Switch to light mode')} title={tr(preferences.scheme === 'light' ? 'Switch to dark mode' : 'Switch to light mode')} aria-pressed={preferences.scheme === 'dark'} onclick={preferences.toggleTheme}>{#if preferences.scheme === 'light'}<Moon size={15} />{:else}<Sun size={15} />{/if}</Button>
      </div>
    </div>

    <div class="login-heading">
      <h1 id="login-title">{tr(mustChangePassword ? 'Set new password' : 'Sign in')}</h1>
      <p>{tr(mustChangePassword ? 'Please set a new password to continue.' : 'Enter your admin password to continue.')}</p>
    </div>
    <div class="inline-flex w-full gap-1 rounded-lg border border-line bg-surface-sunken p-1" role="group" aria-label={tr('Sign in method')}>
      <Button type="button" aria-pressed={loginMode === 'admin'} class="flex-1 rounded-md px-3 py-1.5 text-xs font-semibold {loginMode === 'admin' ? 'bg-accent text-accent-contrast' : 'bg-transparent text-muted'}" onclick={() => switchMode('admin')}>{tr('Admin')}</Button>
      <Button type="button" aria-pressed={loginMode === 'key'} class="flex-1 rounded-md px-3 py-1.5 text-xs font-semibold {loginMode === 'key' ? 'bg-accent text-accent-contrast' : 'bg-transparent text-muted'}" onclick={() => switchMode('key')}>{tr('API key')}</Button>
    </div>

    {#if sessionCheckStatus}
      <section class="session-check-notice" class:rate-limited={sessionCheckStatus === 'rate_limited'} role={sessionCheckStatus === 'rate_limited' ? 'alert' : 'status'} aria-live={sessionCheckStatus === 'rate_limited' ? 'assertive' : 'polite'}>
        <p>
          {#if sessionCheckStatus === 'checking'}
            {tr('Checking admin session…')}
          {:else if sessionCheckStatus === 'rate_limited'}
            {tr('Session restoration is temporarily rate limited. You can still sign in. Try again in {seconds} seconds.', { seconds: sessionRetryAfterSeconds })}
          {:else}
            {tr('Could not restore a saved session. You can still sign in or retry the check.')}
          {/if}
        </p>
        {#if sessionCheckStatus !== 'checking'}
          <Button class="session-check-retry" type="button" disabled={sessionRetryAfterSeconds > 0} onclick={onRetrySessionCheck}>
            {#if sessionRetryAfterSeconds > 0}
              {tr('Retry session check in {seconds}s', { seconds: sessionRetryAfterSeconds })}
            {:else}
              {tr('Retry session check')}
            {/if}
          </Button>
        {/if}
      </section>
    {/if}
    {#if mustChangePassword}
      <AdminPasswordChangeForm {tr} forced initialCurrentPassword={currentPassword} onComplete={onPasswordChanged} />
    {:else}
      {#if loginMode === 'key'}
        <form class="modal-form login-form" onsubmit={signInWithKey}>
          <label class="field"><span>{tr('API key')}</span><Input type="password" placeholder="exo_..." autocomplete="off" bind:value={apiKeyDraft} disabled={saving} ariaLabel={tr('API key')} required /></label>
          <p class="form-help">{tr('Sign in with your API key to view its usage and stats.')}</p>
          {#if errorMessage}<div class="form-error" role="alert">{errorMessage}</div>{/if}
          <Button class="primary-button login-submit" disabled={saving}>{#if saving}<LoaderCircle size={15} class="spin" />{:else}<Check size={15} />{/if}{tr('View stats')}</Button>
        </form>
      {:else}
        <form class="modal-form login-form" onsubmit={signIn}>
          <PasswordInput label={tr('Admin password')} bind:value={passwordDraft} autocomplete="current-password" visibilityToggleLabel={tr('Toggle password visibility')} disabled={saving || sessionCheckStatus === 'checking'} required />
          {#if errorMessage}<div class="form-error" role="alert">{errorMessage}</div>{/if}
          <Button class="primary-button login-submit" disabled={saving || sessionCheckStatus === 'checking'}>{#if saving}<LoaderCircle size={15} class="spin" />{:else}<Check size={15} />{/if}{tr('Sign in')}</Button>
        </form>
      {/if}
    {/if}
    <a class="login-docs-link" href="/docs">{docsCopy(preferences.locale).shell.documentation} <span aria-hidden="true">→</span></a>
  </section>
</main>
