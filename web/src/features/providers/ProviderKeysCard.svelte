<script lang="ts">
  import { AlertTriangle, LoaderCircle, KeyRound, RefreshCw } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import { api } from '../../lib/api';
  import type { Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { Provider, ProviderKey } from '../../lib/types';
  import ProviderKeyList from './ProviderKeyList.svelte';

  const KEY_PAGE_SIZE = 50;

  export let provider: Provider;
  export let tr: Translate;
  export let locale: Locale;
  export let refreshToken = 0;
  export let onProviderChanged: () => void;

  let keys: ProviderKey[] = [];
  let keyToRemove: ProviderKey | null = null;
  let nextCursor: string | null = null;
  let pageCursors: Array<string | null> = [null];
  let loading = true;
  let errorMessage = '';
  let busyId = '';
  let deletingId = '';
  let generation = 0;
  let loadedProviderId = '';
  let lastRefreshToken = 0;
  let lastApiKeyCount: number | undefined = undefined;

  $: isFreebuff = provider?.id === 'freebuff' || provider?.adapter_id === 'freebuff';

  $: if (
    provider.id !== loadedProviderId ||
    (refreshToken !== 0 && refreshToken !== lastRefreshToken) ||
    (provider.api_key_count !== undefined && lastApiKeyCount !== undefined && provider.api_key_count !== lastApiKeyCount)
  ) {
    loadedProviderId = provider.id;
    lastRefreshToken = refreshToken;
    lastApiKeyCount = provider.api_key_count;
    pageCursors = [null];
    nextCursor = null;
    busyId = '';
    deletingId = '';
    loading = true;
    void load();
  } else if (lastApiKeyCount === undefined && provider.api_key_count !== undefined) {
    lastApiKeyCount = provider.api_key_count;
  }

  async function load(): Promise<void> {
    const providerId = provider.id;
    const currentGeneration = ++generation;
    const cursor = pageCursors[pageCursors.length - 1] ?? undefined;
    loading = true;
    try {
      const page = await api.providerKeys(providerId, cursor, KEY_PAGE_SIZE);
      if (currentGeneration !== generation) return;
      keys = page.keys;
      nextCursor = page.next_cursor ?? null;
      errorMessage = '';
    } catch (error) {
      if (currentGeneration === generation) {
        errorMessage = localizedError(error, 'Could not load provider keys.', tr);
      }
    } finally {
      if (currentGeneration === generation) loading = false;
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

  async function renameKey(key: ProviderKey, name: string): Promise<void> {
    if (busyId) return;
    busyId = key.id;
    errorMessage = '';
    try {
      await api.updateProviderKey(provider.id, key.id, { name });
      await load();
    } catch (error) {
      errorMessage = localizedError(error, 'Could not update this provider key.', tr);
    } finally {
      busyId = '';
    }
  }

  async function toggleKey(key: ProviderKey, enabled: boolean): Promise<void> {
    if (busyId) return;
    busyId = key.id;
    errorMessage = '';
    try {
      await api.updateProviderKey(provider.id, key.id, { enabled });
      await load();
    } catch (error) {
      errorMessage = localizedError(error, 'Could not update this provider key.', tr);
    } finally {
      busyId = '';
    }
  }

  async function moveKey(key: ProviderKey, direction: -1 | 1): Promise<void> {
    if (busyId) return;
    const index = keys.findIndex((item) => item.id === key.id);
    const neighbor = keys[index + direction];
    if (index < 0 || !neighbor) return;
    busyId = key.id;
    errorMessage = '';
    try {
      await api.swapProviderKeyOrder(provider.id, key.id, neighbor.id);
      await load();
    } catch (error) {
      errorMessage = localizedError(error, 'Could not change the fallback order.', tr);
    } finally {
      busyId = '';
    }
  }

  function remove(key: ProviderKey): void {
    if (deletingId) return;
    keyToRemove = key;
  }

  async function confirmRemove(): Promise<void> {
    if (!keyToRemove || deletingId) return;
    const key = keyToRemove;
    deletingId = key.id;
    errorMessage = '';
    try {
      await api.deleteProviderKey(provider.id, key.id);
      keys = keys.filter((item) => item.id !== key.id);
      if (!keys.length && pageCursors.length > 1) pageCursors = pageCursors.slice(0, -1);
      keyToRemove = null;
      await load();
      onProviderChanged();
    } catch (error) {
      errorMessage = localizedError(error, 'Could not remove this provider key.', tr);
    } finally {
      deletingId = '';
    }
  }
</script>

<section class="provider-model-section provider-keys-section">
  <div class="provider-model-section-heading">
    <div>
      <h3>{tr('API keys')}</h3>
      <p>{tr(provider.key_strategy === 'round_robin'
        ? 'Each request starts at the next enabled key, then falls back to the following keys.'
        : 'Keys are tried in this order. The first enabled key serves requests and the following keys are fallbacks.')}</p>
    </div>
    <button class="secondary-button compact" disabled={loading} onclick={load}><RefreshCw size={13} />{tr('Refresh keys')}</button>
  </div>

  {#if isFreebuff}
    <div class="provider-key-ban-notice" role="alert" style="margin-bottom: 12px;">
      <AlertTriangle size={15} />
      <span>{tr('High ban risk: Codebuff accounts may be banned when used with Freebuff. Use a secondary account.')}</span>
    </div>
  {/if}

  {#if errorMessage}
    <div class="provider-model-empty" role="alert"><p>{errorMessage}</p><button class="secondary-button compact" onclick={load}><RefreshCw size={13} />{tr('Retry')}</button></div>
  {:else if loading}
    <div class="provider-keys-loading"><LoaderCircle size={15} class="spin" />{tr('Loading provider keys…')}</div>
  {:else if keys.length}
    <ProviderKeyList
      {keys}
      {tr}
      {locale}
      {busyId}
      {deletingId}
      positionOffset={(pageCursors.length - 1) * KEY_PAGE_SIZE}
      hasPreviousPage={pageCursors.length > 1}
      hasNextPage={nextCursor !== null}
      onRemove={remove}
      onRename={renameKey}
      onToggle={toggleKey}
      onMove={moveKey}
    />
  {:else}
    <div class="provider-model-empty"><KeyRound size={19} /><p>{tr('No provider keys yet. Add one from Manage keys.')}</p></div>
  {/if}

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

