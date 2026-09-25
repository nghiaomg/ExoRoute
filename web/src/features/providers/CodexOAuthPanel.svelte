<script lang="ts">
  import { onDestroy } from 'svelte';
  import { Check, Command, Copy, ExternalLink, LoaderCircle, X } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import PortalToast from './PortalToast.svelte';
  import { api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';

  type OAuthState = 'idle' | 'pending' | 'connected' | 'failed' | 'expired';
  type NoticeTone = 'info' | 'success' | 'error';

  interface OAuthToast {
    tone: 'success' | 'error';
    title: string;
    message: string;
  }

  export let providerId: string;
  export let providerAdapterId = '';
  export let providerName = 'OpenAI Codex';
  export let open = false;
  export let tr: Translate;
  export let onConnected: () => void;

  let state: OAuthState = 'idle';
  let flowId = '';
  let authorizationUrl = '';
  let oauthStateValue = '';
  let notice = '';
  let noticeTone: NoticeTone = 'info';
  let callbackDialogOpen = false;
  let callbackInput = '';
  let callbackSubmitting = false;
  let callbackError = '';
  let pollTimer: ReturnType<typeof setTimeout> | undefined;
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  let copyAuthTimer: ReturnType<typeof setTimeout> | undefined;
  let authUrlCopied = false;
  let toast: OAuthToast | null = null;
  let generation = 0;
  let wasOpen = false;
  let activeProviderId = '';
  let isAntigravityProvider = false;
  $: isAntigravityProvider = providerAdapterId.toLowerCase() === 'antigravity';
  const dashboardUsesRemoteHost = typeof window !== 'undefined'
    && !['localhost', '127.0.0.1', '::1'].includes(window.location.hostname.replace(/^\[|\]$/g, '').toLowerCase());

  $: if (open && (!wasOpen || activeProviderId !== providerId)) {
    wasOpen = true;
    activeProviderId = providerId;
    stopPolling();
    state = 'idle';
    flowId = '';
    authorizationUrl = '';
    oauthStateValue = '';
    notice = '';
    callbackDialogOpen = false;
    callbackInput = '';
    callbackError = '';
  } else if (!open) {
    wasOpen = false;
    callbackDialogOpen = false;
    callbackInput = '';
    callbackError = '';
    stopPolling();
  }

  function clearTimer(): void {
    if (pollTimer) clearTimeout(pollTimer);
    pollTimer = undefined;
  }

  function stopPolling(): void {
    generation += 1;
    clearTimer();
  }

  function showToast(tone: OAuthToast['tone'], title: string, message: string): void {
    if (toastTimer) clearTimeout(toastTimer);
    toast = { tone, title, message };
    toastTimer = setTimeout(() => {
      toast = null;
      toastTimer = undefined;
    }, 4000);
  }

  function dismissToast(): void {
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = undefined;
    toast = null;
  }

  async function copyAuthUrl(): Promise<void> {
    if (!authorizationUrl) return;
    try {
      await navigator.clipboard.writeText(authorizationUrl);
      authUrlCopied = true;
      if (copyAuthTimer) clearTimeout(copyAuthTimer);
      copyAuthTimer = setTimeout(() => {
        authUrlCopied = false;
      }, 2500);
    } catch {
      const textarea = document.createElement('textarea');
      textarea.value = authorizationUrl;
      textarea.style.position = 'fixed';
      textarea.style.opacity = '0';
      document.body.appendChild(textarea);
      textarea.select();
      document.execCommand('copy');
      document.body.removeChild(textarea);
      authUrlCopied = true;
      if (copyAuthTimer) clearTimeout(copyAuthTimer);
      copyAuthTimer = setTimeout(() => {
        authUrlCopied = false;
      }, 2500);
    }
  }

  function openCallbackDialog(): void {
    callbackInput = '';
    callbackError = '';
    callbackDialogOpen = true;
  }

  function closeCallbackDialog(): void {
    if (callbackSubmitting) return;
    callbackInput = '';
    callbackError = '';
    callbackDialogOpen = false;
  }

  function callbackUrlForSubmission(value: string): string {
    const trimmed = value.trim();
    if (trimmed.includes('://') || !oauthStateValue) return trimmed;
    const callbackUrl = new URL('http://localhost:1455/auth/callback');
    callbackUrl.searchParams.set('code', trimmed);
    callbackUrl.searchParams.set('state', oauthStateValue);
    return callbackUrl.toString();
  }

  async function submitCallback(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!providerId || !flowId || !callbackInput.trim() || callbackSubmitting) return;
    callbackSubmitting = true;
    callbackError = '';
    try {
      const result = await api.completeProviderAuthCallbackForFlow(providerId, flowId, callbackUrlForSubmission(callbackInput));
      callbackInput = '';
      if (result.status === 'connected') {
        state = 'connected';
        callbackDialogOpen = false;
        callbackError = '';
        notice = '';
        noticeTone = 'success';
        authorizationUrl = '';
        clearTimer();
        showToast(
          'success',
          tr('Successful'),
          tr('{provider} account connected successfully.', { provider: providerName })
        );
        onConnected();
        return;
      }
      callbackDialogOpen = false;
      notice = tr('{provider} callback was received. Finishing sign-in…', { provider: providerName });
      noticeTone = 'info';
    } catch (error) {
      callbackError = localizedError(error, 'This OAuth callback could not be completed. Start a new sign-in and try again.', tr);
    } finally {
      callbackSubmitting = false;
    }
  }

  async function connect(): Promise<void> {
    if (!providerId || state === 'pending') return;
    clearTimer();
    const currentGeneration = ++generation;
    state = 'pending';
    flowId = '';
    authorizationUrl = '';
    callbackInput = '';
    callbackError = '';
    callbackDialogOpen = true;
    notice = tr('{provider} sign-in is starting…', { provider: providerName });
    noticeTone = 'info';

    let popup: Window | null = null;
    try {
      popup = window.open('about:blank', `exoroute-${providerId}-oauth`, 'popup,width=560,height=740');
      if (popup && providerAdapterId.toLowerCase() !== 'antigravity') popup.opener = null;
    } catch {
      popup = null;
    }

    try {
      const flow = await api.startProviderAuth(providerId);
      if (currentGeneration !== generation) {
        popup?.close();
        return;
      }
      flowId = flow.flow_id;
      authorizationUrl = flow.authorization_url;
      try {
        oauthStateValue = new URL(flow.authorization_url).searchParams.get('state') ?? '';
      } catch {
        oauthStateValue = '';
      }
      if (popup && !popup.closed) popup.location.href = flow.authorization_url;
      else {
        notice = tr('The sign-in popup was blocked. Use the link below to continue.');
        noticeTone = 'info';
      }
      await pollStatus(providerId, flow.flow_id, currentGeneration, Date.now() + 5 * 60 * 1000);
    } catch (error) {
      popup?.close();
      if (currentGeneration !== generation) return;
      state = 'failed';
      callbackError = localizedError(error, `Could not start ${providerName} authorization.`, tr);
      notice = callbackError;
      noticeTone = 'error';
    }
  }

  async function pollStatus(id: string, currentFlowId: string, currentGeneration: number, deadline: number): Promise<void> {
    if (currentGeneration !== generation) return;
    if (Date.now() >= deadline) {
      state = 'expired';
      notice = tr('{provider} authorization expired. Start again to reconnect.', { provider: providerName });
      noticeTone = 'error';
      authorizationUrl = '';
      return;
    }

    try {
      const result = await api.providerAuthStatus(id, currentFlowId);
      if (currentGeneration !== generation) return;
      if (result.status === 'connected') {
        state = 'connected';
        callbackDialogOpen = false;
        callbackInput = '';
        callbackError = '';
        notice = '';
        noticeTone = 'success';
        authorizationUrl = '';
        clearTimer();
        showToast(
          'success',
          tr('Successful'),
          tr('{provider} account connected successfully.', { provider: providerName })
        );
        onConnected();
        return;
      }
      if (result.status === 'failed' || result.status === 'expired') {
        state = result.status;
        callbackDialogOpen = false;
        callbackInput = '';
        callbackError = '';
        notice = tr(result.status === 'failed' ? '{provider} authorization failed.' : '{provider} authorization expired. Start again to reconnect.', { provider: providerName });
        noticeTone = 'error';
        authorizationUrl = '';
        clearTimer();
        return;
      }
      state = 'pending';
      notice = tr('{provider} sign-in is still pending. Complete it in the popup or use the link below.', { provider: providerName });
      noticeTone = 'info';
      pollTimer = setTimeout(() => void pollStatus(id, currentFlowId, currentGeneration, deadline), 1500);
    } catch (error) {
      if (currentGeneration !== generation) return;
      state = 'failed';
      notice = localizedError(error, `Could not load ${providerName} status.`, tr);
      noticeTone = 'error';
      authorizationUrl = '';
      clearTimer();
    }
  }

  onDestroy(() => {
    stopPolling();
    if (toastTimer) clearTimeout(toastTimer);
    if (copyAuthTimer) clearTimeout(copyAuthTimer);
  });
</script>

<section class="provider-model-section">
  <div class="provider-model-section-heading">
    <div>
      <h3>{tr('{provider} account', { provider: providerName })}</h3>
    </div>
    <div class="codex-oauth-actions">
      <button class="primary-button compact" disabled={state === 'pending'} onclick={connect}>
        {#if state === 'pending'}
          <LoaderCircle size={13} class="spin" />
          {tr('Waiting for authorization…')}
        {:else}
          <Command size={14} />
          {tr('Connect account')}
        {/if}
      </button>
      {#if state === 'pending' && flowId}
        <button type="button" class="secondary-button compact" onclick={openCallbackDialog}>
          {tr('Paste callback URL or authorization code')}
        </button>
        {#if authorizationUrl}
          <a class="codex-auth-link" href={authorizationUrl} target="_blank" rel="noopener noreferrer">
            <ExternalLink size={13} />
            {tr('Open authorization page')}
          </a>
        {/if}
      {/if}
    </div>
  </div>

  {#if notice && noticeTone !== 'success'}
    <div class="codex-oauth-notice" class:error={noticeTone === 'error'} role={noticeTone === 'error' ? 'alert' : 'status'}>
      {notice}
    </div>
  {/if}
</section>

{#if toast}
  <PortalToast {toast} {tr} onDismiss={dismissToast} />
{/if}

<ArkDialog
  bind:open={callbackDialogOpen}
  closeLabel={tr('Close dialog')}
  title={tr('Connect account')}
  kicker={tr('{provider} sign-in', { provider: providerName })}
  preventClose={callbackSubmitting}
  onClose={closeCallbackDialog}
>
  <form class="modal-form" onsubmit={submitCallback}>
    <div class="oauth-manual-steps" aria-label={tr('OAuth steps')}>
      <div class="oauth-manual-step">
        <strong>{tr('Step 1: Open this URL in your browser')}</strong>
        {#if authorizationUrl}
          <div class="oauth-step-actions">
            <a class="primary-button compact" href={authorizationUrl} target="_blank" rel="noopener noreferrer">
              <ExternalLink size={13} />
              <span>{tr('Open authorization page')}</span>
            </a>
            <button
              type="button"
              class="secondary-button compact"
              onclick={copyAuthUrl}
            >
              {#if authUrlCopied}
                <Check size={13} />
                <span>{tr('Copied!')}</span>
              {:else}
                <Copy size={13} />
                <span>{tr('Copy URL')}</span>
              {/if}
            </button>
          </div>
        {:else}
          <div class="inline-loading">
            <LoaderCircle size={14} class="spin" />
            <span>{tr('Waiting for authorization…')}</span>
          </div>
        {/if}
      </div>
      <div class="oauth-manual-step">
        <label class="oauth-callback-field" for="provider-oauth-callback-url">
          <strong>{tr('Step 2: Paste callback URL or authorization code here')}</strong>
          <div class="oauth-format-guide">
            <span class="oauth-format-label">{tr('OAuth callback URL')}:</span>
            <code>http://localhost:1455/auth/callback?code=...</code>
          </div>
          <textarea
            id="provider-oauth-callback-url"
            class="oauth-callback-textarea"
            bind:value={callbackInput}
            rows="3"
            maxlength="16384"
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
            required
            disabled={callbackSubmitting}
            placeholder={tr('Paste the callback URL or authorization code')}
          ></textarea>
        </label>
      </div>
    </div>
    {#if callbackError}<div class="form-error" role="alert">{callbackError}</div>{/if}
    <div class="modal-actions">
      <button type="button" class="secondary-button" disabled={callbackSubmitting} onclick={closeCallbackDialog}>{tr('Cancel')}</button>
      <button type="submit" class="primary-button" disabled={callbackSubmitting || !callbackInput.trim()}>
        {#if callbackSubmitting}
          <LoaderCircle size={14} class="spin" />
          {tr('Completing sign-in…')}
        {:else}
          {tr('Complete sign-in')}
        {/if}
      </button>
    </div>
  </form>
</ArkDialog>
