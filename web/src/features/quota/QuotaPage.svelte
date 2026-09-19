<script lang="ts">
  import { Check, Gauge, LoaderCircle, RefreshCw, X } from '@lucide/svelte';
  import { onDestroy, onMount } from 'svelte';
  import { Portal } from '@ark-ui/svelte/portal';
  import GatewayError from '../../components/GatewayError.svelte';
  import PageHeading from '../../components/PageHeading.svelte';
  import type { DashboardPage } from '../../lib/navigation';
  import { api } from '../../lib/api';
  import type { Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { Provider } from '../../lib/types';
  import QuotaCredentialCard from './QuotaCredentialCard.svelte';
  import {
    credentialStateFor,
    createCredentialState,
    hasCredentialItems,
    mergeProviderUsage,
    supportsCredentialUsage,
    supportsQuota,
    type CredentialState,
  } from './quota.state';

  export let tr: Translate;
  export let locale: Locale;
  export let onNavigate: (page: DashboardPage) => void;
  export let onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;
  export let onProviderCountChange: (count: number) => void;

  const PROVIDER_REQUEST_CONCURRENCY = 4;

  let providers: Provider[] = [];
  let credentialStates: Record<string, CredentialState> = {};
  let providersLoading = true;
  let providerError = '';
  let providerGeneration = 0;
  let lifecycleGeneration = 0;

  $: quotaProviders = providers.filter(supportsQuota);
  $: credentialProviders = quotaProviders.filter(supportsCredentialUsage);
  $: visibleQuotaProviders = quotaProviders.filter((provider) => {
    const state = credentialStates[provider.id];
    return hasCredentialItems(state) || Boolean(state?.error);
  });
  $: credentialsLoading = credentialProviders.some((provider) => credentialStates[provider.id]?.loading === true);
  $: credentialsRefreshing = credentialProviders.some((provider) => credentialStates[provider.id]?.refreshing === true);
  $: hasCredentialKeys = credentialProviders.some((provider) => hasCredentialItems(credentialStates[provider.id]));
  $: isInitialLoading = providersLoading || (quotaProviders.length > 0 && credentialsLoading && !visibleQuotaProviders.length);

  function updateCredentialState(providerId: string, patch: Partial<CredentialState>): void {
    credentialStates = {
      ...credentialStates,
      [providerId]: { ...credentialStateFor(providerId, credentialStates), ...patch },
    };
  }

  async function loadProviders(): Promise<void> {
    const generation = ++providerGeneration;
    const lifecycle = lifecycleGeneration;
    providersLoading = true;
    providerError = '';
    onConnectionChange('loading');
    try {
      const result = await api.providers();
      if (generation !== providerGeneration || lifecycle !== lifecycleGeneration) return;
      providers = result;
      onProviderCountChange(result.length);
      const supported = result.filter(supportsQuota);
      const nextCredentialStates: Record<string, CredentialState> = {};
      for (const provider of supported) {
        nextCredentialStates[provider.id] = credentialStates[provider.id] ?? createCredentialState();
      }
      credentialStates = nextCredentialStates;
      await loadCredentialPages(supported, generation, lifecycle);
      if (generation !== providerGeneration || lifecycle !== lifecycleGeneration) return;
      onConnectionChange('loaded');
    } catch (error) {
      if (generation !== providerGeneration || lifecycle !== lifecycleGeneration) return;
      providerError = localizedError(error, 'Something went wrong while loading this page.', tr);
      onConnectionChange('error');
    } finally {
      if (generation === providerGeneration && lifecycle === lifecycleGeneration) providersLoading = false;
    }
  }

  async function loadCredentialPages(
    supported: Provider[],
    providerGenerationToken: number,
    lifecycleToken: number,
  ): Promise<void> {
    const providersWithUsage = supported.filter(supportsCredentialUsage);
    for (let index = 0; index < providersWithUsage.length; index += PROVIDER_REQUEST_CONCURRENCY) {
      if (providerGenerationToken !== providerGeneration || lifecycleToken !== lifecycleGeneration) return;
      const batch = providersWithUsage.slice(index, index + PROVIDER_REQUEST_CONCURRENCY);
      await Promise.all(batch.map((provider) => (
        loadCredentialPage(provider.id, providerGenerationToken, lifecycleToken)
      )));
    }
  }

  async function loadCredentialPage(
    providerId: string,
    providerGenerationToken = providerGeneration,
    lifecycleToken = lifecycleGeneration,
  ): Promise<void> {
    const provider = providers.find((candidate) => candidate.id === providerId && supportsQuota(candidate));
    if (!provider || !supportsCredentialUsage(provider)) return;

    const current = credentialStateFor(providerId, credentialStates);
    const generation = current.generation + 1;
    const cursor = current.pageCursors[current.pageCursors.length - 1] ?? undefined;
    updateCredentialState(providerId, {
      loading: true,
      refreshing: false,
      error: '',
      generation,
    });
    try {
      const [keyPageRes, usagePageRes] = await Promise.allSettled([
        api.providerKeys(providerId, cursor),
        api.providerUsage(providerId, cursor),
      ]);

      if (
        providerGenerationToken !== providerGeneration
        || lifecycleToken !== lifecycleGeneration
        || credentialStates[providerId]?.generation !== generation
      ) return;

      if (keyPageRes.status === 'rejected' && usagePageRes.status === 'rejected') {
        const primaryError = keyPageRes.reason || usagePageRes.reason;
        updateCredentialState(providerId, {
          error: localizedError(primaryError, 'Could not load usage limits.', tr),
          loading: false,
        });
        return;
      }

      const keyPage = keyPageRes.status === 'fulfilled' ? keyPageRes.value : { keys: [], next_cursor: null };
      const usagePage = usagePageRes.status === 'fulfilled' ? usagePageRes.value : { accounts: [], next_cursor: null };

      const merged = mergeProviderUsage(
        keyPage.keys ?? [],
        usagePage.accounts ?? [],
        (id) => tr('Account ({id})', { id: id.slice(0, 8) }),
      );

      updateCredentialState(providerId, {
        keys: merged.keys,
        usageByKey: merged.usageByKey,
        nextCursor: keyPage.next_cursor ?? usagePage.next_cursor ?? null,
        loading: false,
        error: '',
      });
    } catch (error) {
      if (
        providerGenerationToken === providerGeneration
        && lifecycleToken === lifecycleGeneration
        && credentialStates[providerId]?.generation === generation
      ) {
        updateCredentialState(providerId, {
          error: localizedError(error, 'Could not load usage limits.', tr),
          loading: false,
        });
      }
    } finally {
      // Unconditionally reset loading to ensure UI never hangs on loading spinner
      if (lifecycleToken === lifecycleGeneration) {
        updateCredentialState(providerId, { loading: false });
      }
    }
  }

  async function refreshCredentials(): Promise<void> {
    const targets = credentialProviders.filter((provider) => {
      const state = credentialStates[provider.id];
      return hasCredentialItems(state) && !state?.loading && !state?.refreshing;
    });
    for (let index = 0; index < targets.length; index += PROVIDER_REQUEST_CONCURRENCY) {
      const batch = targets.slice(index, index + PROVIDER_REQUEST_CONCURRENCY);
      await Promise.all(batch.map((provider) => refreshProviderCredentials(provider.id)));
    }
  }

  async function refreshProviderCredentials(providerId: string): Promise<void> {
    const provider = providers.find((candidate) => candidate.id === providerId && supportsQuota(candidate));
    const current = credentialStateFor(providerId, credentialStates);
    if (!provider || !supportsCredentialUsage(provider) || !hasCredentialItems(current) || current.loading || current.refreshing) return;

    const providerGenerationToken = providerGeneration;
    const lifecycleToken = lifecycleGeneration;
    const generation = current.generation + 1;
    const cursor = current.pageCursors[current.pageCursors.length - 1] ?? undefined;
    updateCredentialState(providerId, { refreshing: true, error: '', generation });
    try {
      const result = await api.refreshProviderUsage(providerId, cursor);
      if (
        providerGenerationToken !== providerGeneration
        || lifecycleToken !== lifecycleGeneration
        || credentialStates[providerId]?.generation !== generation
      ) return;

      const merged = mergeProviderUsage(
        current.keys,
        result.accounts ?? [],
        (id) => tr('Account ({id})', { id: id.slice(0, 8) }),
      );

      updateCredentialState(providerId, {
        keys: merged.keys,
        usageByKey: merged.usageByKey,
        nextCursor: result.next_cursor ?? current.nextCursor,
        refreshing: false,
      });
    } catch (error) {
      if (
        providerGenerationToken === providerGeneration
        && lifecycleToken === lifecycleGeneration
        && credentialStates[providerId]?.generation === generation
      ) {
        updateCredentialState(providerId, {
          error: localizedError(error, 'Could not load usage limits.', tr),
          refreshing: false,
        });
      }
    } finally {
      if (lifecycleToken === lifecycleGeneration) {
        updateCredentialState(providerId, { refreshing: false });
      }
    }
  }

  async function nextPage(providerId: string): Promise<void> {
    const state = credentialStateFor(providerId, credentialStates);
    if (!state.nextCursor || state.loading || state.refreshing) return;
    updateCredentialState(providerId, { pageCursors: [...state.pageCursors, state.nextCursor] });
    await loadCredentialPage(providerId);
  }

  async function previousPage(providerId: string): Promise<void> {
    const state = credentialStateFor(providerId, credentialStates);
    if (state.pageCursors.length <= 1 || state.loading || state.refreshing) return;
    updateCredentialState(providerId, { pageCursors: state.pageCursors.slice(0, -1) });
    await loadCredentialPage(providerId);
  }

  interface QuotaToast {
    tone: 'success' | 'error';
    title: string;
    message: string;
  }
  let toast: QuotaToast | null = null;
  let toastTimer: ReturnType<typeof setTimeout> | undefined;

  function showToast(tone: QuotaToast['tone'], title: string, message: string): void {
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

  onMount(() => {
    lifecycleGeneration += 1;
    void loadProviders();
  });
  onDestroy(() => {
    providerGeneration += 1;
    lifecycleGeneration += 1;
    if (toastTimer) clearTimeout(toastTimer);
  });
</script>

<PageHeading title={tr('Quota')} subtitle={tr('Monitor provider quota and local usage from one place.')} {tr}>
  {#if hasCredentialKeys}
    <button class="secondary-button" disabled={credentialsLoading || credentialsRefreshing || !hasCredentialKeys} onclick={refreshCredentials}>
      {#if credentialsRefreshing}<LoaderCircle size={14} class="spin" />{tr('Refreshing limits…')}
      {:else}<RefreshCw size={14} />{tr('Refresh limits')}{/if}
    </button>
  {/if}
</PageHeading>

{#if isInitialLoading}
  <div class="quota-skeleton-list" aria-label={tr('Loading quota providers…')}>
    <section class="quota-skeleton-section">
      <div class="quota-skeleton-header">
        <div class="quota-skeleton-title skeleton"></div>
        <div class="quota-skeleton-badge skeleton"></div>
      </div>
      <div class="quota-credential-list">
        {#each [1, 2, 3, 4] as item (item)}
          <div class="quota-skeleton-card">
            <div class="quota-skeleton-card-header">
              <div class="quota-skeleton-avatar skeleton"></div>
              <div class="quota-skeleton-lines">
                <div class="quota-skeleton-line-title skeleton"></div>
                <div class="quota-skeleton-line-sub skeleton"></div>
              </div>
            </div>
            <div class="quota-skeleton-meter">
              <div class="quota-skeleton-meter-val skeleton"></div>
              <div class="quota-skeleton-meter-track skeleton"></div>
            </div>
            <div class="quota-skeleton-footer skeleton"></div>
          </div>
        {/each}
      </div>
    </section>
  </div>
{:else if providerError}
  <GatewayError message={providerError} {tr} onRetry={loadProviders} />
{:else if !visibleQuotaProviders.length}
  <section class="quota-empty-state">
    <span class="quota-empty-icon"><Gauge size={22} /></span>
    <div><h2>{tr('No quota providers configured.')}</h2><p>{tr('Add or connect a provider that exposes quota or local usage data to manage it here.')}</p></div>
    <button type="button" class="primary-button" onclick={() => onNavigate('providers')}>{tr('Open Providers')}</button>
  </section>
{:else}
  <div class="quota-provider-list">
    {#each visibleQuotaProviders as provider (provider.id)}
      {@const credentialState = credentialStateFor(provider.id, credentialStates)}
      {@const itemCount = credentialState.keys.length}
      <section class="quota-provider-section">
        <div class="quota-provider-section-heading">
          <div class="quota-provider-title-group">
            <h2>{provider.name}</h2>
            {#if itemCount > 0}
              <span class="quota-count-badge">{itemCount} {tr(itemCount === 1 ? 'credential' : 'credentials')}</span>
            {/if}
          </div>
          {#if supportsCredentialUsage(provider)}
            <button
              type="button"
              class="secondary-button compact quota-provider-refresh-btn"
              disabled={credentialState.loading || credentialState.refreshing}
              onclick={() => { void refreshProviderCredentials(provider.id); }}
              aria-label={tr('Refresh limits')}
              title={tr('Refresh limits')}
            >
              {#if credentialState.refreshing}
                <LoaderCircle size={13} class="spin" />
                <span class="refresh-btn-text">{tr('Refreshing limits…')}</span>
              {:else}
                <RefreshCw size={13} />
                <span class="refresh-btn-text">{tr('Refresh')}</span>
              {/if}
            </button>
          {/if}
        </div>
        <div class="quota-page-grid single-column">
          {#if supportsCredentialUsage(provider)}
            <section class="quota-credentials-section">
              {#if credentialState.refreshing}
                <div class="quota-credentials-heading">
                  <span class="provider-usage-note"><LoaderCircle size={12} class="spin" />{tr('Refreshing limits…')}</span>
                </div>
              {/if}
              {#if credentialState.error}
                <p class="provider-usage-request-error" role="alert">{credentialState.error}</p>
              {/if}
              {#if credentialState.loading && !credentialState.keys.length}
                <div class="provider-keys-loading">
                  <LoaderCircle size={18} class="spin" />
                  <span>{tr('Loading usage limits…')}</span>
                </div>
              {:else if credentialState.keys.length}
                <div class="quota-credential-list">
                  {#each credentialState.keys as key (key.id)}
                    <QuotaCredentialCard
                      providerId={provider.id}
                      {key}
                      account={credentialState.usageByKey[key.id]}
                      {locale}
                      {tr}
                      onBudgetChanged={() => { void loadCredentialPage(provider.id); }}
                      onStatusChanged={() => { void loadCredentialPage(provider.id); }}
                      onToast={showToast}
                    />
                  {/each}
                </div>
                {#if credentialState.pageCursors.length > 1 || credentialState.nextCursor}
                  <div class="quota-pagination">
                    <button type="button" class="secondary-button compact" disabled={credentialState.loading || credentialState.refreshing || credentialState.pageCursors.length <= 1} onclick={() => { void previousPage(provider.id); }}>{tr('Previous keys')}</button>
                    <span>{tr('Page {current}', { current: credentialState.pageCursors.length })}</span>
                    <button type="button" class="secondary-button compact" disabled={credentialState.loading || credentialState.refreshing || !credentialState.nextCursor} onclick={() => { void nextPage(provider.id); }}>{tr('Next keys')}</button>
                  </div>
                {/if}
              {:else}
                <div class="quota-empty-credentials">{tr('This provider has no credentials to monitor yet.')}</div>
              {/if}
            </section>
          {/if}
        </div>
      </section>
    {/each}
  </div>
{/if}

{#if toast}
  <Portal>
    <div class="model-test-toast" class:success={toast.tone === 'success'} class:error={toast.tone === 'error'} role="status">
      <span class="model-test-toast-mark">
        {#if toast.tone === 'success'}
          <Check size={15} />
        {:else}
          <X size={15} />
        {/if}
      </span>
      <div class="model-test-toast-copy">
        <strong>{toast.title}</strong>
        <small>{toast.message}</small>
      </div>
      <button class="model-test-toast-dismiss" aria-label={tr('Dismiss notification')} onclick={dismissToast}>
        <X size={14} />
      </button>
    </div>
  </Portal>
{/if}
