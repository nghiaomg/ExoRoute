<script lang="ts">
  import Button from "../../components/Button.svelte";
  import { onDestroy } from 'svelte';
  import { Check, Copy, ExternalLink, LoaderCircle, Link } from '@lucide/svelte';
  import { api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
  import { localizedError } from '../../lib/errors';

  type DeviceState = 'idle' | 'pending' | 'connected' | 'failed' | 'expired';
  type NoticeTone = 'info' | 'success' | 'error';

  export let providerId: string;
  /** Shared auth-panel prop contract; exposed on the panel for selectors. */
  export let providerAdapterId = '';
  export let providerName = 'Kilo Code';
  export let open = false;
  export let tr: Translate;
  export let onConnected: () => void;

  let state: DeviceState = 'idle';
  let flowId = '';
  let verificationUrl = '';
  let userCode = '';
  let notice = '';
  let noticeTone: NoticeTone = 'info';
  let codeCopied = false;
  let isPending = false;
  let pollTimer: ReturnType<typeof setTimeout> | undefined;
  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;
  let wasOpen = false;
  let activeProviderId = '';
  let pollDeadline = 0;
  let kiroToken = '';
  let kiroImporting = false;

  $: isPending = state === 'pending';

  $: if (open && (!wasOpen || activeProviderId !== providerId)) {
    wasOpen = true;
    activeProviderId = providerId;
    stopPolling();
    state = 'idle';
    flowId = '';
    verificationUrl = '';
    userCode = '';
    notice = '';
    noticeTone = 'info';
    kiroToken = '';
    kiroImporting = false;
  } else if (!open) {
    wasOpen = false;
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

  async function copyCode(): Promise<void> {
    if (!userCode) return;
    try {
      await navigator.clipboard.writeText(userCode);
    } catch {
      const textarea = document.createElement('textarea');
      textarea.value = userCode;
      textarea.style.position = 'fixed';
      textarea.style.opacity = '0';
      document.body.appendChild(textarea);
      textarea.select();
      document.execCommand('copy');
      document.body.removeChild(textarea);
    }
    codeCopied = true;
    if (copyTimer) clearTimeout(copyTimer);
    copyTimer = setTimeout(() => {
      codeCopied = false;
    }, 2500);
  }

  async function connect(): Promise<void> {
    if (!providerId || isPending) return;
    clearTimer();
    const currentGeneration = ++generation;
    state = 'pending';
    flowId = '';
    verificationUrl = '';
    userCode = '';
    notice = tr('Waiting for authorization…');
    noticeTone = 'info';
    let popup: Window | null = null;
    try {
      popup = window.open(
        'about:blank',
        `exoroute-device-${providerId}`,
        'popup,width=560,height=740',
      );
      if (popup) popup.opener = null;
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
      verificationUrl = flow.authorization_url;
      userCode = flow.user_code ?? '';
      if (popup && !popup.closed) popup.location.href = flow.authorization_url;
      else {
        notice = tr('The sign-in popup was blocked. Use the link below to continue.');
        noticeTone = 'info';
      }
      const interval = Math.max(1, flow.interval ?? 3);
      pollDeadline = Date.now() + Math.max(60, flow.expires_in ?? 300) * 1000;
      schedulePoll(currentGeneration, interval);
    } catch (error) {
      popup?.close();
      if (currentGeneration !== generation) return;
      state = 'failed';
      notice = localizedError(error, 'Could not start device sign-in.', tr);
      noticeTone = 'error';
    }
  }

  function schedulePoll(currentGeneration: number, intervalSeconds: number): void {
    clearTimer();
    pollTimer = setTimeout(() => {
      if (currentGeneration !== generation) return;
      void pollOnce(currentGeneration, intervalSeconds);
    }, intervalSeconds * 1000);
  }

  async function pollOnce(currentGeneration: number, intervalSeconds: number): Promise<void> {
    if (currentGeneration !== generation || !flowId) return;
    if (Date.now() >= pollDeadline) {
      expireSignIn();
      return;
    }
    try {
      const result = await api.pollProviderAuth(providerId, flowId);
      if (currentGeneration !== generation) return;
      if (result.status === 'connected') {
        stopPolling();
        state = 'connected';
        notice = tr('Provider sign-in completed and the account is connected.');
        noticeTone = 'success';
        verificationUrl = '';
        userCode = '';
        onConnected();
        return;
      }
      if (result.status === 'failed') {
        state = 'failed';
        notice = result.message || tr('Device sign-in was denied. Start again.');
        noticeTone = 'error';
        return;
      }
      if (result.status === 'expired') {
        expireSignIn();
        return;
      }
      if (result.message) {
        notice = result.message;
        noticeTone = 'info';
      }
    } catch (error) {
      if (currentGeneration !== generation) return;
      notice = localizedError(error, 'Could not check the sign-in status.', tr);
      noticeTone = 'error';
    }
    schedulePoll(currentGeneration, intervalSeconds);
  }

  async function importKiroToken(): Promise<void> {
    const token = kiroToken.trim();
    if (!token || kiroImporting || !providerId) return;
    kiroImporting = true;
    try {
      await api.importKiroLogin(providerId, token);
      kiroToken = '';
      state = 'connected';
      notice = tr('Kiro login imported; the account is connected.');
      noticeTone = 'success';
      onConnected();
    } catch (error) {
      notice = localizedError(error, 'Could not import the Kiro login.', tr);
      noticeTone = 'error';
    } finally {
      kiroImporting = false;
    }
  }

  function expireSignIn(): void {
    clearTimer();
    state = 'expired';
    notice = tr('Device sign-in expired. Start again.');
    noticeTone = 'error';
    verificationUrl = '';
    userCode = '';
  }

  onDestroy(() => {
    generation += 1;
    clearTimer();
    if (copyTimer) clearTimeout(copyTimer);
  });
</script>

<section class="provider-model-section" data-adapter={providerAdapterId}>
  <div class="provider-model-section-heading">
    <div>
      <h3>{tr('{provider} account', { provider: providerName })}</h3>
    </div>
    <div class="codex-oauth-actions">
      {#if state === 'idle' || state === 'connected'}
        <Button class="primary-button compact" disabled={!providerId} onclick={() => void connect()}>
          <Link size={14} />
          {tr('Connect account')}
        </Button>
      {:else if state === 'pending'}
        <Button class="secondary-button compact" disabled>
          <LoaderCircle size={13} class="spin" />
          {tr('Waiting for authorization…')}
        </Button>
      {:else}
        <Button class="primary-button compact" disabled={!providerId} onclick={() => void connect()}>
          <Link size={14} />
          {tr('Start again')}
        </Button>
      {/if}
    </div>
  </div>

  <div class="codex-oauth-panel">
    {#if userCode}
      <p class="device-code-hint">
        {tr('Enter this code on the {provider} page to finish sign-in.', { provider: providerName })}
      </p>
      <div class="device-code-value">
        <code>{userCode}</code>
        <Button class="secondary-button compact" type="button" onclick={() => void copyCode()}>
          {#if codeCopied}
            <Check size={13} />
            {tr('Code copied')}
          {:else}
            <Copy size={13} />
            {tr('Copy code')}
          {/if}
        </Button>
      </div>
    {/if}
    {#if verificationUrl}
      <a class="codex-auth-link" href={verificationUrl} target="_blank" rel="noopener noreferrer">
        <ExternalLink size={13} />
        {tr('Open authorization page')}
      </a>
    {/if}
    {#if providerAdapterId === 'kiro'}
      <div class="agy-cli-import">
        <p class="agy-cli-hint">
          {tr('Paste a Kiro IDE or social refresh token to connect without the device flow.')}
        </p>
        <textarea
          class="agy-cli-textarea"
          bind:value={kiroToken}
          rows="3"
          placeholder="Kiro refresh token"
        ></textarea>
        <div class="agy-cli-actions">
          <Button
            class="secondary-button compact"
            type="button"
            disabled={!kiroToken.trim() || kiroImporting || !providerId}
            onclick={() => void importKiroToken()}
          >
            {#if kiroImporting}
              <LoaderCircle size={13} class="spin" />
            {/if}
            {tr('Import token')}
          </Button>
        </div>
      </div>
    {/if}
    {#if notice}
      <div
        class="codex-oauth-notice"
        class:error={noticeTone === 'error'}
        class:success={noticeTone === 'success'}
        role={noticeTone === 'error' ? 'alert' : 'status'}
      >
        {notice}
      </div>
    {/if}
  </div>
</section>
