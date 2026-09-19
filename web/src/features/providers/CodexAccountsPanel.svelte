<script lang="ts">
  import { LoaderCircle, RefreshCw } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import type { Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { ProviderKey } from '../../lib/types';
  import { api } from '../../lib/api';
  import ProviderKeyList from './ProviderKeyList.svelte';

  export let providerId: string;
  export let providerName = 'OpenAI Codex';
  export let tr: Translate;
  export let locale: Locale;
  export let refreshToken = 0;
  export let onChanged: () => void;

  let keys: ProviderKey[] = [];
  let keyToRemove: ProviderKey | null = null;
  let nextCursor: string | null = null;
  let pageCursors: Array<string | null> = [null];
  let loading = true;
  let errorMessage = '';
  let deletingId = '';
  let generation = 0;
  let loadedProviderId = '';
  let lastRefreshToken = -1;

  $: if (providerId && (providerId !== loadedProviderId || refreshToken !== lastRefreshToken)) {
    if (providerId !== loadedProviderId) pageCursors = [null];
    loadedProviderId = providerId;
    lastRefreshToken = refreshToken;
    void load();
  }

  async function load(): Promise<void> {
    const currentGeneration = ++generation;
    const cursor = pageCursors[pageCursors.length - 1] ?? undefined;
    loading = true;
    try {
      const keyPage = await api.providerKeys(providerId, cursor);
      if (currentGeneration !== generation) return;
      keys = keyPage.keys;
      nextCursor = keyPage.next_cursor ?? null;
      errorMessage = '';
    } catch (error) {
      if (currentGeneration === generation) {
        errorMessage = localizedError(error, 'Could not load connected accounts.', tr);
      }
    } finally {
      if (currentGeneration === generation) {
        loading = false;
      }
    }
  }

  async function nextPage(): Promise<void> {
    if (!nextCursor || loading) return;
    pageCursors = [...pageCursors, nextCursor];
    await load();
  }

  async function previousPage(): Promise<void> {
    if (pageCursors.length <= 1 || loading) return;
    pageCursors = pageCursors.slice(0, -1);
    await load();
  }

  function remove(key: ProviderKey): void {
    if (deletingId) return;
    keyToRemove = key;
  }

  async function confirmRemove(): Promise<void> {
    if (!keyToRemove || deletingId) return;
    const key = keyToRemove;
    deletingId = key.id;
    try {
      await api.deleteProviderKey(providerId, key.id);
      keys = keys.filter((item) => item.id !== key.id);
      if (!keys.length && pageCursors.length > 1) pageCursors = pageCursors.slice(0, -1);
      keyToRemove = null;
      await load();
      onChanged();
    } catch (error) {
      errorMessage = localizedError(error, 'Could not remove this provider key.', tr);
    } finally {
      deletingId = '';
    }
  }
</script>

<section class="provider-model-section codex-accounts-section">
  <div class="provider-model-section-heading">
    <div>
      <h3>{tr('Connected accounts')}</h3>
    </div>
  </div>
  {#if errorMessage}<div class="provider-model-empty" role="alert"><p>{errorMessage}</p><button class="secondary-button compact" onclick={load}><RefreshCw size={13} />{tr('Retry')}</button></div>
  {:else if loading}<div class="provider-keys-loading"><LoaderCircle size={15} class="spin" />{tr('Loading connected accounts…')}</div>
  {:else if keys.length}<ProviderKeyList {keys} {tr} {locale} {deletingId} onRemove={remove} />
  {:else}<div class="provider-keys-empty">{tr('No {provider} accounts connected yet.', { provider: providerName })}</div>{/if}
  {#if pageCursors.length > 1 || nextCursor}
    <div class="provider-key-pagination">
      <button class="secondary-button compact" disabled={loading || pageCursors.length <= 1} onclick={previousPage}>{tr('Previous keys')}</button>
      <span>{tr('Page {current}', { current: pageCursors.length })}</span>
      <button class="secondary-button compact" disabled={loading || !nextCursor} onclick={nextPage}>{tr('Next keys')}</button>
    </div>
  {/if}
</section>

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
