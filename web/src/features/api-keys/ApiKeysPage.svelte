<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { KeyRound, LoaderCircle, Plus } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import GatewayError from '../../components/GatewayError.svelte';
  import InlineLoading from '../../components/InlineLoading.svelte';
  import PageHeading from '../../components/PageHeading.svelte';
  import PageSearch from '../../components/PageSearch.svelte';
  import { api } from '../../lib/api';
  import type { FeatureActionRequest } from '../../lib/navigation';
  import type { Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { GatewayApiKey } from '../../lib/types';
  import GatewayApiKeyCard from './GatewayApiKeyCard.svelte';
  import ApiKeyDetailsPage from './ApiKeyDetailsPage.svelte';
  import GatewayApiKeyDialog from './GatewayApiKeyDialog.svelte';

  export let tr: Translate;
  export let locale: Locale;
  export let actionRequest: FeatureActionRequest | null = null;
  export let onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;

  let apiKeys: GatewayApiKey[] = [];
  let keyToRevoke: GatewayApiKey | null = null;
  let viewingApiKeyId: string | null = null;
  let query = '';
  let nextCursor: string | null = null;
  let pageCursors: Array<string | null> = [null];
  let loading = true;
  let errorMessage = '';
  let deletingId = '';
  let dialogOpen = false;
  let generation = 0;
  let handledActionId = 0;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  $: viewingApiKey = viewingApiKeyId
    ? apiKeys.find((k) => k.id === viewingApiKeyId) ?? null
    : null;

  $: if (actionRequest?.page === 'api-keys' && actionRequest.id !== handledActionId) {
    handledActionId = actionRequest.id;
    dialogOpen = true;
  }

  function currentCursor(): string | null {
    return pageCursors[pageCursors.length - 1] ?? null;
  }

  function checkUrlApiKey(): void {
    if (typeof window === 'undefined') return;
    const params = new URLSearchParams(window.location.search);
    const id = params.get('id');
    if (id) {
      const match = apiKeys.find((k) => k.id === id);
      if (match) {
        viewingApiKeyId = match.id;
        window.scrollTo({ top: 0, behavior: 'instant' });
      } else {
        viewingApiKeyId = null;
      }
    } else {
      viewingApiKeyId = null;
    }
  }

  async function load(cursor: string | null = currentCursor(), search = query): Promise<void> {
    const requestGeneration = ++generation;
    loading = true;
    errorMessage = '';
    onConnectionChange('loading');
    try {
      const page = await api.apiKeys({ cursor, q: search });
      if (requestGeneration !== generation) return;
      apiKeys = page.api_keys;
      nextCursor = page.next_cursor ?? null;
      checkUrlApiKey();
      onConnectionChange('loaded');
    } catch (error) {
      if (requestGeneration !== generation) return;
      errorMessage = localizedError(error, 'Something went wrong while loading this page.', tr);
      onConnectionChange('error');
    } finally {
      if (requestGeneration === generation) loading = false;
    }
  }

  function searchChanged(value: string): void {
    query = value;
    pageCursors = [null];
    nextCursor = null;
    generation += 1;
    loading = true;
    errorMessage = '';
    onConnectionChange('loading');
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => {
      searchTimer = undefined;
      void load(null, value);
    }, 250);
  }

  function revoke(key: GatewayApiKey): void {
    keyToRevoke = key;
  }

  function showDetails(key: GatewayApiKey): void {
    viewingApiKeyId = key.id;
    if (typeof window !== 'undefined') {
      const url = new URL(window.location.href);
      url.searchParams.set('id', key.id);
      window.history.pushState({ keyId: key.id }, '', url.toString());
      window.scrollTo({ top: 0, behavior: 'instant' });
    }
  }

  function closeDetails(): void {
    viewingApiKeyId = null;
    if (typeof window !== 'undefined') {
      const url = new URL(window.location.href);
      url.searchParams.delete('id');
      window.history.pushState({}, '', url.pathname + (url.search ? url.search : ''));
      window.scrollTo({ top: 0, behavior: 'instant' });
    }
  }

  async function confirmRevoke(): Promise<void> {
    if (!keyToRevoke) return;
    const key = keyToRevoke;
    errorMessage = '';
    deletingId = key.id;
    try {
      await api.deleteApiKey(key.id);
      if (apiKeys.length === 1 && pageCursors.length > 1) pageCursors = pageCursors.slice(0, -1);
      if (viewingApiKeyId === key.id) closeDetails();
      keyToRevoke = null;
      await load();
    } catch (error) {
      errorMessage = localizedError(error, 'Could not revoke this API key.', tr);
    } finally {
      deletingId = '';
    }
  }

  function openDialog(): void {
    errorMessage = '';
    dialogOpen = true;
  }

  async function refreshToFirstPage(): Promise<void> {
    pageCursors = [null];
    await load(null, query);
  }

  async function nextPage(): Promise<void> {
    if (!nextCursor || loading) return;
    pageCursors = [...pageCursors, nextCursor];
    await load(nextCursor);
  }

  async function previousPage(): Promise<void> {
    if (pageCursors.length <= 1 || loading) return;
    pageCursors = pageCursors.slice(0, -1);
    await load(currentCursor());
  }

  onMount(() => {
    void load(null, query);
    const handlePopState = () => { checkUrlApiKey(); };
    window.addEventListener('popstate', handlePopState);
    return () => {
      window.removeEventListener('popstate', handlePopState);
    };
  });
  onDestroy(() => {
    generation += 1;
    if (searchTimer) clearTimeout(searchTimer);
  });
</script>

<div class="api-keys-view">
  {#if viewingApiKey}
    <ApiKeyDetailsPage
      apiKey={viewingApiKey}
      {tr}
      {locale}
      onBack={closeDetails}
      onRevoke={revoke}
    />
  {:else}
    <PageHeading title={tr('Gateway API keys')} subtitle={tr('Manage client keys for requests to /v1.')} {tr}>
      <PageSearch bind:value={query} pageLabel={tr('API keys')} {tr} onInput={searchChanged} />
      <button class="primary-button" onclick={openDialog}><Plus size={17} />{tr('Create key')}</button>
    </PageHeading>

    {#if errorMessage}<GatewayError message={errorMessage} {tr} onRetry={load} />
    {:else if loading}<InlineLoading label={'Loading {page}…'} {tr} vars={{ page: tr('Gateway API keys').toLowerCase() }} />
    {:else}
      <section class="api-key-summary"><span class="api-key-summary-icon"><KeyRound size={20} /></span><div><strong>{tr('Showing {count} API keys on this page', { count: apiKeys.length })}</strong><span>{tr('Clients must send a key to use the /v1 endpoints')}</span></div></section>
      {#if apiKeys.length}
        <div class="api-key-grid">{#each apiKeys as apiKey (apiKey.id)}<GatewayApiKeyCard {apiKey} {tr} {locale} deleting={deletingId === apiKey.id} onDetails={showDetails} onRevoke={revoke} />{/each}</div>
      {:else if query.trim()}
        <EmptyState icon="search" title={tr('No matching API keys')} description={tr('Try a different search, or clear the filter.')} />
      {:else}
        <EmptyState icon="api-keys" title={tr('No client API keys')} description={tr('Requests to /v1 are blocked until you create an API key.')} action={tr('Create key')} onclick={openDialog} />
      {/if}
      {#if pageCursors.length > 1 || nextCursor}
        <div class="provider-key-pagination">
          <button class="secondary-button compact" disabled={loading || pageCursors.length <= 1} onclick={previousPage}>{tr('Previous keys')}</button>
          <span>{tr('Page {current}', { current: pageCursors.length })}</span>
          <button class="secondary-button compact" disabled={loading || !nextCursor} onclick={nextPage}>{tr('Next keys')}</button>
        </div>
      {/if}
    {/if}
  {/if}

  <GatewayApiKeyDialog open={dialogOpen} {tr} onClose={() => dialogOpen = false} onCreated={refreshToFirstPage} />

  <ArkDialog
    open={keyToRevoke !== null}
    role="alertdialog"
    closeLabel={tr('Close dialog')}
    title={tr('Are you absolutely sure?')}
    kicker={tr('EXOROUTE CONTROL PLANE')}
    onClose={() => { keyToRevoke = null; }}
  >
    <div class="modal-form">
      <p class="modal-description">
        {tr('Revoke “{name}”? Clients using it will lose access.', { name: keyToRevoke?.name ?? '' })}
      </p>
      <div class="modal-actions" style="margin-top: 16px;">
        <button type="button" class="secondary-button" disabled={Boolean(deletingId)} onclick={() => { keyToRevoke = null; }}>
          {tr('Cancel')}
        </button>
        <button
          type="button"
          class="primary-button danger"
          disabled={Boolean(deletingId)}
          onclick={confirmRevoke}
        >
          {#if deletingId}<LoaderCircle size={14} class="spin" />{tr('Revoking…')}{:else}{tr('Revoke')}{/if}
        </button>
      </div>
    </div>
  </ArkDialog>
</div>
