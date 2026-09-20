<script lang="ts">
  import { Check, LoaderCircle, Moon, Sun } from '@lucide/svelte';
  import ArkPasswordInput from './ArkPasswordInput.svelte';
  import FlyingFishLogo from './FlyingFishLogo.svelte';
  import LanguageSelect from './LanguageSelect.svelte';
  import AdminPasswordChangeForm from './AdminPasswordChangeForm.svelte';
  import { api, type AdminAccessResult, type AdminLoginResult } from '../lib/api';
  import { type Translate } from '../lib/format';
import { localizedError } from '../lib/errors';
  import type { Locale } from '../lib/i18n';
  import { docsCopy } from '../features/docs/docsCopy';

  interface Preferences {
    locale: Locale;
    theme: 'light' | 'dark';
    setLocale: (locale: Locale) => void;
    toggleTheme: () => void;
  }

  export let tr: Translate;
  export let preferences: Preferences;
  export let mustChangePassword = false;
  export let currentPassword = '';
  export let onLogin: (result: AdminLoginResult, enteredPassword: string) => void;
  export let onPasswordChanged: (result: AdminAccessResult) => void;
  export let sessionCheckStatus: 'checking' | 'rate_limited' | 'unavailable' | null = null;
  export let sessionRetryAfterSeconds = 0;
  export let onRetrySessionCheck: () => Promise<void> = async () => {};

  let passwordDraft = '';
  let saving = false;
  let errorMessage = '';

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
        <span class="brand-version">0.1</span>
      </a>
      <div class="preference-controls" aria-label={tr('Language and appearance')}>
        <LanguageSelect locale={preferences.locale} {tr} onLocaleChange={preferences.setLocale} />
        <button class="preference-button theme-button" aria-label={tr(preferences.theme === 'light' ? 'Switch to dark mode' : 'Switch to light mode')} title={tr(preferences.theme === 'light' ? 'Switch to dark mode' : 'Switch to light mode')} aria-pressed={preferences.theme === 'dark'} onclick={preferences.toggleTheme}>{#if preferences.theme === 'light'}<Moon size={15} />{:else}<Sun size={15} />{/if}</button>
      </div>
    </div>

    <div class="login-heading">
      <h1 id="login-title">{tr(mustChangePassword ? 'Set new password' : 'Sign in')}</h1>
      <p>{tr(mustChangePassword ? 'Please set a new password to continue.' : 'Enter your admin password to continue.')}</p>
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
          <button class="session-check-retry" type="button" disabled={sessionRetryAfterSeconds > 0} onclick={onRetrySessionCheck}>
            {#if sessionRetryAfterSeconds > 0}
              {tr('Retry session check in {seconds}s', { seconds: sessionRetryAfterSeconds })}
            {:else}
              {tr('Retry session check')}
            {/if}
          </button>
        {/if}
      </section>
    {/if}
    {#if mustChangePassword}
      <AdminPasswordChangeForm {tr} forced initialCurrentPassword={currentPassword} onComplete={onPasswordChanged} />
    {:else}
      <form class="modal-form login-form" onsubmit={signIn}>
        <ArkPasswordInput label={tr('Admin password')} bind:value={passwordDraft} autocomplete="current-password" visibilityToggleLabel={tr('Toggle password visibility')} disabled={saving || sessionCheckStatus === 'checking'} required />
        {#if errorMessage}<div class="form-error" role="alert">{errorMessage}</div>{/if}
        <button class="primary-button login-submit" disabled={saving || sessionCheckStatus === 'checking'}>{#if saving}<LoaderCircle size={15} class="spin" />{:else}<Check size={15} />{/if}{tr('Sign in')}</button>
      </form>
    {/if}
    <a class="login-docs-link" href="/docs">{docsCopy(preferences.locale).shell.documentation} <span aria-hidden="true">→</span></a>
  </section>
</main>
