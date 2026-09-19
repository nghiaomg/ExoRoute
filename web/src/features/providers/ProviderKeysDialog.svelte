<script lang="ts">
  import { onDestroy } from 'svelte';
  import { AlertTriangle, LoaderCircle, Plus } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import ArkField from '../../components/ArkField.svelte';
  import ArkPasswordInput from '../../components/ArkPasswordInput.svelte';
  import { api } from '../../lib/api';
  import type { Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { Provider, ProviderKey } from '../../lib/types';
  import ProviderKeyList from './ProviderKeyList.svelte';
  import { resolveAuthPanel } from './auth-panel.registry';

  export let open = false;
  export let provider: Provider | null = null;
  export let tr: Translate;
  export let locale: Locale;
  export let onChanged: () => void;

  let keys: ProviderKey[] = [];
  let nextCursor: string | null = null;
  let pageCursors: Array<string | null> = [null];
  let keyName = '';
  let keyDraft = '';
  let notice = '';
  let noticeWarning = false;
  let errorMessage = '';
  let loading = false;
  let saving = false;
  let busyId = '';
  let deletingId = '';
  let keyToRemove: ProviderKey | null = null;
  let generation = 0;
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;
  let wasOpen = false;
  let loadedProviderId = '';
  let autoCloseTimer: ReturnType<typeof setTimeout> | undefined;

  $: authPanel = resolveAuthPanel(provider?.capabilities?.auth_panel);
  $: supportsLocalUsage = provider?.capabilities?.local_usage_meter === true;
  $: isFreebuff = provider?.id === 'freebuff' || provider?.adapter_id === 'freebuff';
  $: if (open && provider && (!wasOpen || loadedProviderId !== provider.id)) {
    generation += 1;
    wasOpen = true;
    loadedProviderId = provider.id;
    pageCursors = [null];
    keyName = '';
    keyDraft = '';
    keys = [];
    nextCursor = null;
    notice = '';
    errorMessage = '';
    void loadKeys();
  } else if (!open) {
    if (autoCloseTimer) {
      clearTimeout(autoCloseTimer);
      autoCloseTimer = undefined;
    }
    if (wasOpen) {
      generation += 1;
      loading = false;
    }
    wasOpen = false;
  }

  function isCurrentRequest(currentGeneration: number, providerId: string): boolean {
    return open && currentGeneration === generation && provider?.id === providerId;
  }

  onDestroy(() => {
    generation += 1;
    if (autoCloseTimer) clearTimeout(autoCloseTimer);
  });

  function keyTestWarning(message: string): string {
    const commandCodeStatus = message.match(/^Command Code API key could not be verified \(HTTP (\d+)\)$/);
    if (commandCodeStatus) return tr('Command Code API key could not be verified (HTTP {status}).', { status: commandCodeStatus[1] });
    const openRouterStatus = message.match(/^OpenRouter API key could not be verified \(HTTP (\d+)\)$/);
    if (openRouterStatus) return tr('OpenRouter API key could not be verified (HTTP {status}).', { status: openRouterStatus[1] });
    const freebuffStatus = message.match(/^Freebuff validation returned HTTP (\d+)$/);
    if (freebuffStatus) return tr('Freebuff validation returned HTTP {status}', { status: freebuffStatus[1] });
    return message;
  }

  async function loadKeys(): Promise<void> {
    if (!provider) return;
    const providerId = provider.id;
    const currentGeneration = ++generation;
    loading = true;
    errorMessage = '';
    try {
      const cursor = pageCursors[pageCursors.length - 1] ?? undefined;
      const result = await api.providerKeys(providerId, cursor);
      if (!isCurrentRequest(currentGeneration, providerId)) return;
      keys = result.keys;
      nextCursor = result.next_cursor ?? null;
    } catch (error) {
      if (isCurrentRequest(currentGeneration, providerId)) {
        errorMessage = localizedError(error, 'Could not load provider keys.', tr);
      }
    } finally {
      if (isCurrentRequest(currentGeneration, providerId)) loading = false;
    }
  }

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!provider) return;
    errorMessage = '';
    notice = '';
    saving = true;
    try {
      const result = await api.createProviderKey(provider.id, keyName.trim() || undefined, keyDraft.trim());
      noticeWarning = !result.test_passed;
      notice = result.test_passed
        ? tr('Key test passed and the key was saved.')
        : result.warning
          ? keyTestWarning(result.warning)
          : tr('The key was saved, but its test failed. Check the warning before relying on it.');
      keyName = '';
      keyDraft = '';
      await loadKeys();
      onChanged();
      if (result.test_passed) {
        if (autoCloseTimer) clearTimeout(autoCloseTimer);
        autoCloseTimer = setTimeout(() => {
          open = false;
          notice = '';
        }, 500);
      }
    } catch (error) {
      errorMessage = localizedError(error, 'Could not add this provider key.', tr);
    } finally {
      saving = false;
    }
  }

  function remove(key: ProviderKey): void {
    if (!provider || deletingId) return;
    keyToRemove = key;
  }

  async function confirmRemove(): Promise<void> {
    if (!provider || !keyToRemove || deletingId) return;
    const key = keyToRemove;
    deletingId = key.id;
    errorMessage = '';
    try {
      await api.deleteProviderKey(provider.id, key.id);
      keys = keys.filter((item) => item.id !== key.id);
      if (!keys.length && pageCursors.length > 1) {
        pageCursors = pageCursors.slice(0, -1);
        await loadKeys();
      } else {
        await loadKeys();
      }
      keyToRemove = null;
      onChanged();
    } catch (error) {
      errorMessage = localizedError(error, 'Could not remove this provider key.', tr);
    } finally {
      deletingId = '';
    }
  }

  async function nextPage(): Promise<void> {
    if (!nextCursor || loading) return;
    pageCursors = [...pageCursors, nextCursor];
    await loadKeys();
  }

  async function previousPage(): Promise<void> {
    if (pageCursors.length <= 1 || loading) return;
    pageCursors = pageCursors.slice(0, -1);
    await loadKeys();
  }

  async function authConnected(): Promise<void> {
    noticeWarning = false;
    notice = tr('Command Code API key saved.');
    await loadKeys();
    onChanged();
    if (autoCloseTimer) clearTimeout(autoCloseTimer);
    autoCloseTimer = setTimeout(() => {
      open = false;
      notice = '';
    }, 500);
  }
</script>

<ArkDialog bind:open closeLabel={tr('Close dialog')} title={tr('Manage provider keys')} kicker={tr('Provider details')} wide>
  <div class="modal-form">
    {#if provider}<p class="modal-description">{provider.name}</p>{/if}
    {#if notice}<div class:key-test-warning={noticeWarning} class:key-test-success={!noticeWarning} role={noticeWarning ? 'alert' : 'status'}>{notice}</div>{/if}
    {#if errorMessage}<div class="form-error" role="alert">{errorMessage}</div>{/if}
    {#if open && provider?.capabilities?.api_key_auth_assist && authPanel?.apiKey}
      <svelte:component this={authPanel.apiKey} providerId={provider.id} {tr} onConnected={authConnected} />
    {/if}
    {#if loading}<div class="provider-keys-loading"><LoaderCircle size={15} class="spin" />{tr('Loading provider keys…')}</div>
    {:else if keys.length}<ProviderKeyList {keys} {tr} {locale} {deletingId} onRemove={remove} />
    {:else}<div class="provider-keys-empty">{tr('No provider keys yet.')}</div>{/if}
    {#if pageCursors.length > 1 || nextCursor}
      <div class="provider-key-pagination">
        <button class="secondary-button compact" disabled={loading || pageCursors.length <= 1} onclick={previousPage}>{tr('Previous keys')}</button>
        <span>{tr('Page {current}', { current: pageCursors.length })}</span>
        <button class="secondary-button compact" disabled={loading || !nextCursor} onclick={nextPage}>{tr('Next keys')}</button>
      </div>
    {/if}
    <form class="provider-key-add-form" onsubmit={submit}>
      {#if isFreebuff}
        <div class="provider-key-ban-notice" role="alert">
          <AlertTriangle size={15} />
          <span>{tr('High ban risk: Codebuff accounts may be banned when used with Freebuff. Use a secondary account.')}</span>
        </div>
      {/if}
      <div class="form-section-label">{tr('Add API key')} <span>{tr(isFreebuff ? 'Freebuff uses a CodeBuff auth token; the session endpoint is checked before saving.' : supportsLocalUsage ? 'Cline API keys are saved without an automatic inference test; use Test model explicitly.' : 'Each key is tested before it is saved')}</span></div>
      <ArkField label={`${tr('Key name')} (${tr('optional')})`} bind:value={keyName} placeholder={tr('e.g. production key')} />
      <ArkPasswordInput label={tr('API key')} bind:value={keyDraft} autocomplete="new-password" required placeholder={tr(isFreebuff ? 'Freebuff / CodeBuff auth token' : 'Secret is sent directly to the gateway')} visibilityToggleLabel={tr('Toggle password visibility')} />
      <div class="modal-actions"><button class="primary-button" disabled={saving || loading}>{#if saving}<LoaderCircle size={15} class="spin" />{:else}<Plus size={15} />{/if}{tr(supportsLocalUsage ? 'Save API key' : 'Test and save key')}</button></div>
    </form>
  </div>
</ArkDialog>

<ArkDialog
  open={keyToRemove !== null}
  role="alertdialog"
  closeLabel={tr('Close dialog')}
  title={tr('Are you absolutely sure?')}
  kicker={tr('EXOROUTE CONTROL PLANE')}
  onClose={() => { keyToRemove = null; }}
>
  <div class="modal-form">
    <p class="modal-description">
      {tr('Remove provider key “{name}”?', { name: keyToRemove?.name ?? '' })}
    </p>
    <div class="modal-actions" style="margin-top: 16px;">
      <button type="button" class="secondary-button" disabled={Boolean(deletingId)} onclick={() => { keyToRemove = null; }}>
        {tr('Cancel')}
      </button>
      <button
        type="button"
        class="primary-button danger"
        disabled={Boolean(deletingId)}
        onclick={confirmRemove}
      >
        {#if deletingId}<LoaderCircle size={14} class="spin" />{tr('Removing…')}{:else}{tr('Remove key')}{/if}
      </button>
    </div>
  </div>
</ArkDialog>

