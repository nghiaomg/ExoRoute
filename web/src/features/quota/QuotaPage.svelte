<script lang="ts">
  import { Check, Gauge, LoaderCircle, RefreshCw, X } from '@lucide/svelte';
  import { onDestroy, onMount } from 'svelte';
  import { Portal } from '@ark-ui/svelte/portal';
  import GatewayError from '../../components/GatewayError.svelte';
  import PageHeading from '../../components/PageHeading.svelte';
  import type { DashboardPage } from '../../lib/navigation';
  import type { Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
  import type { Provider } from '../../lib/types';
  import QuotaCredentialCard from './QuotaCredentialCard.svelte';
  import {
    credentialStateFor,
    createCredentialState,
    hasCredentialItems,
    supportsCredentialUsage,
    supportsQuota,
    type CredentialState,
  } from './quota.state';
  import { createQuotaCredentialsController } from './quota.credentials';

  export let tr: Translate;
  export let locale: Locale;
  export let onNavigate: (page: DashboardPage) => void;
  export let onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;
  export let onProviderCountChange: (count: number) => void;

  let providers: Provider[] = [];
  let credentialStates: Record<string, CredentialState> = {};
  let providersLoading = true;
  let providerError = '';

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

  // Data flow lives in quota.credentials.ts; the component stays reactive and
  // passes these callbacks so the controller can update this state.
  const credentials = createQuotaCredentialsController({
    getProviders: () => providers,
    getCredentialStates: () => credentialStates,
    onProviders: (next) => { providers = next; },
    onCredentialStates: (next) => { credentialStates = next; },
    onProvidersLoading: (loading) => { providersLoading = loading; },
    onProviderError: (message) => { providerError = message; },
    onConnectionChange,
    onProviderCountChange,
    tr,
  });

  // Thin delegations keep the original template handler names.
  function loadProviders(): Promise<void> { return credentials.load(); }
  function refreshCredentials(): Promise<void> { return credentials.refreshAll(); }
  function refreshProviderCredentials(providerId: string): Promise<void> { return credentials.refreshProvider(providerId); }
  function loadCredentialPage(providerId: string): Promise<void> { return credentials.reloadProvider(providerId); }
  function nextPage(providerId: string): Promise<void> { return credentials.nextPage(providerId); }
  function previousPage(providerId: string): Promise<void> { return credentials.previousPage(providerId); }

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
    void credentials.load();
  });
  onDestroy(() => {
    credentials.destroy();
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
