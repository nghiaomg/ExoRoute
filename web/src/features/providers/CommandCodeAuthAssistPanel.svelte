<script lang="ts">
  import { onDestroy } from 'svelte';
  import { ArrowUpRight, LoaderCircle, ShieldCheck } from '@lucide/svelte';
  import { api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';

  export let providerId: string;
  export let tr: Translate;
  export let onConnected: () => void;

  type AuthState = 'idle' | 'starting' | 'pending' | 'received';

  let state: AuthState = 'idle';
  let flowId = '';
  let errorMessage = '';
  let notice = '';
  let popup: Window | null = null;
  let pollTimer: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;

  function clearPoll(): void {
    if (pollTimer) clearTimeout(pollTimer);
    pollTimer = undefined;
  }

  function closePopup(): void {
    if (popup && !popup.closed) popup.close();
    popup = null;
  }

  async function connect(): Promise<void> {
    if (state === 'starting' || state === 'pending' || state === 'received') return;
    clearPoll();
    closePopup();
    errorMessage = '';
    notice = '';
    state = 'starting';
    const currentGeneration = ++generation;
    const authWindow = window.open('about:blank', '_blank', 'popup,width=560,height=760');
    if (!authWindow) {
      state = 'idle';
      errorMessage = tr('Allow pop-ups to connect with Command Code Studio.');
      return;
    }
    popup = authWindow;
    try {
      const result = await api.startProviderApiKeyAuth(providerId);
      if (currentGeneration !== generation) {
        authWindow.close();
        return;
      }
      flowId = result.flow_id;
      state = 'pending';
      notice = tr('Finish setting up in the new window. ExoRoute will save the key after the callback returns.');
      authWindow.location.assign(result.auth_url);
      schedulePoll(currentGeneration);
    } catch (error) {
      if (currentGeneration === generation) {
        state = 'idle';
        errorMessage = localizedError(error, 'Could not start Command Code Studio sign-in.', tr);
      }
      authWindow.close();
      popup = null;
    }
  }

  function schedulePoll(currentGeneration: number): void {
    clearPoll();
    pollTimer = setTimeout(() => void poll(currentGeneration), 1800);
  }

  async function poll(currentGeneration: number): Promise<void> {
    if (currentGeneration !== generation || !flowId) return;
    try {
      const result = await api.providerApiKeyAuthStatus(providerId, flowId);
      if (currentGeneration !== generation) return;
      if (result.status === 'received') {
        state = 'received';
        notice = tr('Command Code key received. Saving it securely…');
        await saveReceivedKey(currentGeneration);
        return;
      }
      if (result.status === 'applied') {
        finishConnected();
        return;
      }
      if (result.status === 'failed' || result.status === 'expired') {
        state = 'idle';
        errorMessage = tr('Studio sign-in expired. Add the API key manually.');
        closePopup();
        return;
      }
      if (popup?.closed) {
        state = 'idle';
        errorMessage = tr('The sign-in window was closed. You can retry or enter a key manually.');
        return;
      }
      schedulePoll(currentGeneration);
    } catch (error) {
      if (currentGeneration !== generation) return;
      errorMessage = localizedError(error, 'Could not check Command Code sign-in status.', tr);
      schedulePoll(currentGeneration);
    }
  }

  async function saveReceivedKey(currentGeneration = generation): Promise<void> {
    if (!flowId || currentGeneration !== generation) return;
    clearPoll();
    state = 'received';
    errorMessage = '';
    try {
      await api.applyProviderApiKeyAuth(providerId, flowId);
      if (currentGeneration === generation) finishConnected();
    } catch (error) {
      if (currentGeneration === generation) {
        errorMessage = localizedError(error, 'Could not save the returned Command Code key. Retry the save or use manual entry.', tr);
        notice = '';
      }
    }
  }

  function finishConnected(): void {
    clearPoll();
    state = 'idle';
    flowId = '';
    errorMessage = '';
    notice = tr('Command Code API key saved.');
    closePopup();
    onConnected();
  }

  function startAgain(): void {
    clearPoll();
    closePopup();
    flowId = '';
    state = 'idle';
    errorMessage = '';
    notice = '';
  }

  onDestroy(() => {
    generation += 1;
    clearPoll();
    closePopup();
  });
</script>

<section class="command-code-auth-panel">
  <div class="command-code-auth-copy">
    <span class="command-code-auth-icon"><ShieldCheck size={16} /></span>
    <div>
      <h3>{tr('Command Code Studio')}</h3>
      <p>{tr('Sign in to Command Code Studio to securely add a provider API key. The key is sent directly to this local ExoRoute instance.')}</p>
    </div>
  </div>
  {#if notice}<div class="command-code-auth-notice" role="status">{notice}</div>{/if}
  {#if errorMessage}<div class="command-code-auth-notice error" role="alert">{errorMessage}</div>{/if}
  <div class="command-code-auth-actions">
    {#if state === 'received'}
      <button class="primary-button" disabled onclick={() => void saveReceivedKey()}><LoaderCircle size={14} class="spin" />{tr('Saving provider key…')}</button>
      {#if errorMessage}<button class="secondary-button compact" onclick={() => void saveReceivedKey()}>{tr('Retry saving key')}</button>{/if}
    {:else if state === 'pending'}
      <button class="secondary-button compact" disabled><LoaderCircle size={14} class="spin" />{tr('Waiting for Command Code Studio…')}</button>
      <button class="row-icon" aria-label={tr('Start again')} title={tr('Start again')} onclick={startAgain}><ArrowUpRight size={14} /></button>
    {:else}
      <button class="secondary-button compact" disabled={state === 'starting'} onclick={() => void connect()}>
        {#if state === 'starting'}<LoaderCircle size={14} class="spin" />{tr('Starting Studio sign-in…')}
        {:else}<ArrowUpRight size={14} />{tr('Connect with Command Code Studio')}{/if}
      </button>
    {/if}
  </div>
</section>
