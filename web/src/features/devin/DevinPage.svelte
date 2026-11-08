<script lang="ts">
  import { Bot, LoaderCircle, RefreshCw, Send } from '@lucide/svelte';
  import { onDestroy, onMount } from 'svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import InlineLoading from '../../components/InlineLoading.svelte';
  import GatewayError from '../../components/GatewayError.svelte';
  import PageHeading from '../../components/PageHeading.svelte';
  import Toast from '../../components/Toast.svelte';
  import type { Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import { api } from '../../lib/api';
  import type { DevinSession, DevinSettingsResult } from '../../lib/types';

  export let tr: Translate;
  export let locale: Locale;
  export let onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;

  const SESSION_POLL_MS = 10_000;

  interface DevinToast { tone: 'success' | 'error'; title: string; message?: string }
  let toast: DevinToast | null = null;
  let toastTimer: ReturnType<typeof setTimeout> | undefined;

  function showToast(tone: DevinToast['tone'], title: string, message?: string): void {
    if (toastTimer) clearTimeout(toastTimer);
    toast = { tone, title, message };
    toastTimer = setTimeout(() => { toast = null; toastTimer = undefined; }, 4000);
  }

  // --- API key settings -------------------------------------------------
  let settings: DevinSettingsResult | null = null;
  let settingsError = '';
  let apiKeyInput = '';
  let savingKey = false;
  let removingKey = false;
  let verifying = false;
  let verifyOutcome: { verified: boolean; detail: string } | null = null;

  async function loadSettings(): Promise<void> {
    settingsError = '';
    try {
      settings = await api.devinSettings();
    } catch (error) {
      settingsError = errorMessage(error);
    }
  }

  async function saveKey(): Promise<void> {
    if (savingKey || !apiKeyInput.trim()) return;
    savingKey = true;
    verifyOutcome = null;
    try {
      await api.devinSaveSettings(apiKeyInput.trim());
      apiKeyInput = '';
      await loadSettings();
      showToast('success', tr('Devin API key saved.'));
    } catch (error) {
      showToast('error', tr('Could not save the Devin API key.'), errorMessage(error));
    } finally {
      savingKey = false;
    }
  }

  async function removeKey(): Promise<void> {
    if (removingKey) return;
    removingKey = true;
    verifyOutcome = null;
    try {
      await api.devinDeleteSettings();
      await loadSettings();
      sessions = [];
      showToast('success', tr('Devin API key removed.'));
    } catch (error) {
      showToast('error', tr('Could not remove the Devin API key.'), errorMessage(error));
    } finally {
      removingKey = false;
    }
  }

  async function verifyKey(): Promise<void> {
    if (verifying) return;
    verifying = true;
    verifyOutcome = null;
    try {
      const result = await api.devinVerify();
      verifyOutcome = {
        verified: result.verified,
        detail: typeof result.detail === 'string' && result.detail ? result.detail : '',
      };
    } catch (error) {
      verifyOutcome = { verified: false, detail: errorMessage(error) };
    } finally {
      verifying = false;
    }
  }

  // --- Sessions ---------------------------------------------------------
  let sessions: DevinSession[] = [];
  let sessionsLoading = true;
  let sessionsError = '';
  let pollTimer: ReturnType<typeof setInterval> | undefined;

  async function loadSessions(): Promise<void> {
    if (!settings?.configured) {
      sessionsLoading = false;
      return;
    }
    onConnectionChange('loading');
    try {
      const page = await api.devinListSessions(50, 0);
      sessions = Array.isArray(page) ? page : [];
      sessionsError = '';
      onConnectionChange('loaded');
    } catch (error) {
      sessionsError = errorMessage(error);
      onConnectionChange('error');
    } finally {
      sessionsLoading = false;
    }
  }

  function refreshAll(): Promise<void> {
    return loadSessions();
  }

  // --- Create session ---------------------------------------------------
  let prompt = '';
  let repository = '';
  let branch = '';
  let creating = false;

  async function createSession(): Promise<void> {
    if (creating || !prompt.trim()) return;
    creating = true;
    try {
      await api.devinCreateSession({
        prompt: prompt.trim(),
        ...(repository.trim() ? { repository: repository.trim() } : {}),
        ...(branch.trim() ? { branch: branch.trim() } : {}),
      });
      prompt = '';
      showToast('success', tr('Session created.'));
      await loadSessions();
    } catch (error) {
      showToast('error', tr('Could not create the session.'), errorMessage(error));
    } finally {
      creating = false;
    }
  }

  // --- Send message -----------------------------------------------------
  let messageTarget: DevinSession | null = null;
  let messageInput = '';
  let sendingMessage = false;

  function openMessageDialog(session: DevinSession): void {
    messageTarget = session;
    messageInput = '';
  }

  async function sendMessage(): Promise<void> {
    const target = messageTarget;
    if (!target || sendingMessage || !messageInput.trim()) return;
    sendingMessage = true;
    try {
      await api.devinSendMessage(sessionIdOf(target), messageInput.trim());
      messageTarget = null;
      showToast('success', tr('Message sent.'));
      await loadSessions();
    } catch (error) {
      showToast('error', tr('Could not send the message.'), errorMessage(error));
    } finally {
      sendingMessage = false;
    }
  }

  // --- Helpers ----------------------------------------------------------
  function errorMessage(error: unknown): string {
    return error instanceof Error && error.message ? error.message : tr('Request failed.');
  }

  function sessionIdOf(session: DevinSession): string {
    return session.session_id ?? session.id ?? '';
  }

  function sessionTitle(session: DevinSession): string {
    if (session.title) return session.title;
    const id = sessionIdOf(session);
    return id ? id.slice(0, 12) : tr('Untitled session');
  }

  function sessionPrUrl(session: DevinSession): string {
    if (typeof session.pull_request?.url === 'string') return session.pull_request.url;
    return '';
  }

  function sessionCreated(session: DevinSession): string {
    const value = session.created_at;
    if (typeof value !== 'string' || !value) return '';
    try {
      return new Intl.DateTimeFormat(locale === 'vi' ? 'vi-VN' : locale, {
        dateStyle: 'short',
        timeStyle: 'short',
      }).format(new Date(value));
    } catch {
      return value;
    }
  }

  onMount(() => {
    void loadSettings().then(() => {
      void loadSessions();
      pollTimer = setInterval(() => { void loadSessions(); }, SESSION_POLL_MS);
    });
  });

  onDestroy(() => {
    if (pollTimer !== undefined) clearInterval(pollTimer);
    if (toastTimer !== undefined) clearTimeout(toastTimer);
  });
</script>

<div class="devin-view">
  <PageHeading
    title={tr('Devin cloud agents')}
    subtitle={tr('Create and track Devin sessions with the official Cognition API.')}
    {tr}
  />

  {#if settingsError}
    <GatewayError message={settingsError} {tr} onRetry={loadSettings} />
  {/if}

  <section class="devin-panel">
    <h2>{tr('API key')}</h2>
    {#if settings && !settings.master_key_configured}
      <p class="devin-hint" role="status">{tr('Set EXOROUTE_MASTER_KEY to store the Devin API key.')}</p>
    {:else if settings?.configured}
      <p class="devin-hint">{tr('A Devin API key is stored. Enter a new key to replace it.')}</p>
    {:else}
      <p class="devin-hint">{tr('No Devin API key is stored yet.')}</p>
    {/if}
    <div class="devin-key-row">
      <input
        class="devin-key-input"
        type="password"
        autocomplete="off"
        placeholder="cog_…"
        bind:value={apiKeyInput}
        aria-label={tr('Devin API key')}
      />
      <button type="button" class="primary-button" disabled={savingKey || !apiKeyInput.trim()} onclick={saveKey}>
        {#if savingKey}<LoaderCircle size={14} class="spin" />{/if}
        {tr('Save key')}
      </button>
      <button type="button" class="secondary-button" disabled={removingKey || !settings?.configured} onclick={removeKey}>
        {#if removingKey}<LoaderCircle size={14} class="spin" />{/if}
        {tr('Remove key')}
      </button>
      <button type="button" class="secondary-button" disabled={verifying || !settings?.configured} onclick={verifyKey}>
        {#if verifying}<LoaderCircle size={14} class="spin" />{/if}
        {tr('Verify key')}
      </button>
    </div>
    {#if verifyOutcome}
      <p class="devin-verify-result" class:ok={verifyOutcome.verified} class:failed={!verifyOutcome.verified} role="status">
        {verifyOutcome.verified ? tr('Key verified. The Devin API is reachable.') : tr('Key check failed.')}
        {verifyOutcome.detail}
      </p>
    {/if}
  </section>

  <section class="devin-panel">
    <h2>{tr('Create session')}</h2>
    <div class="devin-form">
      <label class="devin-field">
        <span>{tr('Prompt')}</span>
        <textarea rows="4" bind:value={prompt} placeholder={tr('Describe the task for Devin.')}></textarea>
      </label>
      <div class="devin-form-row">
        <label class="devin-field">
          <span>{tr('Repository URL (optional)')}</span>
          <input type="url" bind:value={repository} placeholder="https://github.com/org/repo" />
        </label>
        <label class="devin-field">
          <span>{tr('Branch (optional)')}</span>
          <input type="text" bind:value={branch} />
        </label>
      </div>
      <div class="devin-form-actions">
        <button type="button" class="primary-button" disabled={creating || !prompt.trim()} onclick={createSession}>
          {#if creating}<LoaderCircle size={14} class="spin" />{/if}
          {tr('Create session')}
        </button>
      </div>
    </div>
  </section>

  <section class="devin-panel">
    <div class="devin-sessions-head">
      <h2>{tr('Devin sessions')}</h2>
      <button type="button" class="secondary-button" onclick={refreshAll} disabled={!settings?.configured}>
        <RefreshCw size={14} />
        {tr('Refresh')}
      </button>
    </div>
    <p class="devin-hint">{tr('Auto-refreshes every 10 seconds while this page is open.')}</p>
    {#if sessionsError}
      <GatewayError message={sessionsError} {tr} onRetry={refreshAll} />
    {:else if sessionsLoading}
      <InlineLoading label={'Loading {page}…'} {tr} vars={{ page: 'Devin' }} />
    {:else if sessions.length}
      <div class="devin-table-wrap">
        <table class="devin-session-table">
          <thead>
            <tr>
              <th scope="col">{tr('Status')}</th>
              <th scope="col">{tr('Title')}</th>
              <th scope="col">{tr('Created')}</th>
              <th scope="col">{tr('Pull request')}</th>
              <th scope="col">{tr('Actions')}</th>
            </tr>
          </thead>
          <tbody>
            {#each sessions as session (sessionIdOf(session) || sessionTitle(session))}
              <tr>
                <td><code class="devin-status">{session.status ?? '—'}</code></td>
                <td class="devin-title-cell" title={sessionIdOf(session)}>{sessionTitle(session)}</td>
                <td>{sessionCreated(session)}</td>
                <td>
                  {#if sessionPrUrl(session)}
                    <a href={sessionPrUrl(session)} target="_blank" rel="noopener noreferrer">{tr('Pull request')}</a>
                  {:else}
                    —
                  {/if}
                </td>
                <td>
                  <button type="button" class="secondary-button" onclick={() => openMessageDialog(session)}>
                    <Send size={13} />
                    {tr('Send message')}
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else if settings?.configured}
      <EmptyState
        icon="requests"
        title={tr('No Devin sessions yet. Create one to get started.')}
        description={tr('Create and track Devin sessions with the official Cognition API.')}
      />
    {:else}
      <p class="devin-hint"><Bot size={14} /> {tr('No Devin API key is stored yet.')}</p>
    {/if}
  </section>

  <ArkDialog
    open={messageTarget !== null}
    title={tr('Message to Devin')}
    closeLabel={tr('Close dialog')}
    onClose={() => { messageTarget = null; }}
  >
    <div class="modal-form">
      <p class="modal-description">{sessionTitle(messageTarget ?? {})}</p>
      <label class="devin-field">
        <span>{tr('Message')}</span>
        <textarea rows="4" bind:value={messageInput}></textarea>
      </label>
      <div class="modal-actions">
        <button type="button" class="secondary-button" onclick={() => { messageTarget = null; }}>
          {tr('Cancel')}
        </button>
        <button type="button" class="primary-button" disabled={sendingMessage || !messageInput.trim()} onclick={sendMessage}>
          {#if sendingMessage}<LoaderCircle size={14} class="spin" />{/if}
          {tr('Send')}
        </button>
      </div>
    </div>
  </ArkDialog>

  <Toast {toast} {tr} onDismiss={() => { toast = null; }} />
</div>
