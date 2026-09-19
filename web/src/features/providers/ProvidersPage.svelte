<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Cpu, KeyRound, Layers, LoaderCircle, Plus, Server, Sliders } from '@lucide/svelte';
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
  import type { UpstreamProtocol, Provider, ProviderPreset } from '../../lib/types';
  import ProviderCard from './ProviderCard.svelte';
  import ProviderCreateDialog from './ProviderCreateDialog.svelte';
  import ProviderDetailsPage from './ProviderDetailsPage.svelte';
  import ProviderKeysDialog from './ProviderKeysDialog.svelte';
  import ProviderPresetCatalog from './ProviderPresetCatalog.svelte';

  export let tr: Translate;
  export let locale: Locale;
  export let actionRequest: FeatureActionRequest | null = null;
  export let onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;
  export let onProviderCountChange: (count: number) => void;

  let providers: Provider[] = [];
  let presets: ProviderPreset[] = [];
  let query = '';
  let loading = true;
  let errorMessage = '';
  let deletingId = '';
  let providerToDelete: Provider | null = null;
  let createOpen = false;
  let keysOpen = false;
  let openingPresetId = '';
  let activeProvider: Provider | null = null;
  let viewingProviderId: string | null = null;
  let generation = 0;
  let handledActionId = 0;

  $: filteredProviders = providers.filter((provider) =>
    `${provider.name} ${provider.base_url} ${provider.preferred_protocol}`.toLowerCase().includes(query.toLowerCase())
  );
  $: customPreset = presets.find((p) => p.id === 'custom' || p.adapter_id === 'generic') ?? null;
  $: availablePresets = presets.filter((p) => p.id !== 'custom' && p.adapter_id !== 'generic');
  $: filteredAvailablePresets = availablePresets.filter((preset) =>
    `${preset.name} ${preset.description} ${preset.id} ${preset.category.replace(/_/g, ' ')} ${preset.labels.map((label) => label.replace(/_/g, ' ')).join(' ')}`
      .toLowerCase()
      .includes(query.toLowerCase())
  );
  $: customProviders = filteredProviders.filter((p) => p.adapter_id === 'generic');

  $: viewingProvider = viewingProviderId
    ? providers.find((p) => p.id === viewingProviderId) ?? null
    : null;

  $: totalCredentials = providers.reduce(
    (sum, p) => sum + (p.api_key_count ?? (p.api_key ? 1 : 0)),
    0
  );
  $: totalModels = providers.reduce((sum, p) => sum + (p.model_count ?? 0), 0);

  $: if (actionRequest?.page === 'providers' && actionRequest.id !== handledActionId) {
    handledActionId = actionRequest.id;
    openCustomProvider();
  }

  function checkUrlProvider(): void {
    if (typeof window === 'undefined') return;
    const params = new URLSearchParams(window.location.search);
    const id = params.get('id');
    if (id) {
      const match = providers.find(
        (p) =>
          p.id === id ||
          p.adapter_id === id ||
          p.id === id.replace(/-/g, '_') ||
          p.id === id.replace(/_/g, '-'),
      );
      if (match) {
        viewingProviderId = match.id;
        activeProvider = match;
        window.scrollTo({ top: 0, behavior: 'instant' });
      } else {
        viewingProviderId = null;
      }
    } else {
      viewingProviderId = null;
    }
  }

  function openCustomProvider(): void {
    createOpen = true;
  }

  async function handlePresetSelect(preset: ProviderPreset, provider: Provider | null): Promise<void> {
    if (openingPresetId) return;
    if (provider) {
      errorMessage = '';
      openDetails(provider);
      return;
    }

    const baseUrl = preset.default_base_url?.trim();
    const supportedProtocols = [...new Set(preset.default_supported_protocols.filter(
      (protocol): protocol is UpstreamProtocol => [
        'chat_completions', 'responses', 'messages', 'google_generate_content',
      ].includes(String(protocol)),
    ))];
    if (!baseUrl || !supportedProtocols.length) {
      errorMessage = tr('Could not open provider details. Reload the page and try again.');
      return;
    }

    const preferredProtocol = supportedProtocols.includes(preset.default_preferred_protocol as UpstreamProtocol)
      ? preset.default_preferred_protocol as UpstreamProtocol
      : supportedProtocols[0];
    openingPresetId = preset.id;
    errorMessage = '';
    let createError: unknown = null;
    try {
      await api.createProvider({
        adapter_id: preset.adapter_id,
        name: preset.name,
        base_url: baseUrl,
        logo_url: preset.default_logo_url ?? null,
        model_prefix: preset.default_model_prefix ?? undefined,
        enabled: true,
        auth_type: preset.default_auth_type,
        api_keys: [],
        preferred_protocol: preferredProtocol,
        supported_protocols: supportedProtocols,
      });
    } catch (error) {
      createError = error;
    }

    try {
      const latestProviders = await api.providers();
      providers = latestProviders;
      onProviderCountChange(latestProviders.length);
      const configuredProvider = latestProviders.find((item) => item.adapter_id === preset.adapter_id);
      if (configuredProvider) {
        openDetails(configuredProvider);
        return;
      }
      if (createError) throw createError;
      errorMessage = tr('Could not open provider details. Reload the page and try again.');
    } catch (error) {
      errorMessage = localizedError(
        createError ?? error,
        'Could not open provider details. Reload the page and try again.',
        tr,
      );
    } finally {
      openingPresetId = '';
    }
  }

  async function load(): Promise<void> {
    const requestGeneration = ++generation;
    loading = true;
    errorMessage = '';
    onConnectionChange('loading');
    try {
      const [loadedProviders, loadedPresets] = await Promise.all([
        api.providers(),
        api.providerPresets(),
      ]);
      if (requestGeneration !== generation) return;
      providers = loadedProviders;
      presets = loadedPresets;
      checkUrlProvider();
      onProviderCountChange(providers.length);
      onConnectionChange('loaded');
    } catch (error) {
      if (requestGeneration !== generation) return;
      errorMessage = localizedError(error, 'Something went wrong while loading this page.', tr);
      onConnectionChange('error');
    } finally {
      if (requestGeneration === generation) loading = false;
    }
  }

  async function refreshProviders(): Promise<void> {
    const requestGeneration = ++generation;
    try {
      const latestProviders = await api.providers();
      if (requestGeneration !== generation) return;
      providers = latestProviders;
      onProviderCountChange(providers.length);
      if (activeProvider) activeProvider = providers.find((item) => item.id === activeProvider?.id) ?? activeProvider;
      if (viewingProviderId && !providers.some((p) => p.id === viewingProviderId)) {
        closeDetails();
      }
    } catch (error) {
      if (requestGeneration !== generation) return;
      errorMessage = localizedError(error, 'Could not load providers.', tr);
    }
  }

  async function providerSaved(providerId: string, adapterId: string): Promise<void> {
    await load();
    const preset = presets.find((item) => item.adapter_id === adapterId);
    if (preset?.capabilities.oauth_accounts) {
      const provider = providers.find((item) => item.id === providerId);
      if (provider) {
        createOpen = false;
        openDetails(provider);
      }
    } else if (preset?.capabilities.api_key_auth_assist) {
      const provider = providers.find((item) => item.id === providerId);
      if (provider && provider.api_key_count === 0) {
        createOpen = false;
        openKeys(provider);
      }
    }
  }

  function openDetails(provider: Provider): void {
    activeProvider = provider;
    viewingProviderId = provider.id;
    if (typeof window !== 'undefined') {
      const url = new URL(window.location.href);
      url.searchParams.set('id', provider.id);
      window.history.pushState({ providerId: provider.id }, '', url.toString());
      window.scrollTo({ top: 0, behavior: 'instant' });
    }
  }

  function closeDetails(): void {
    viewingProviderId = null;
    if (typeof window !== 'undefined') {
      const url = new URL(window.location.href);
      url.searchParams.delete('id');
      window.history.pushState({}, '', url.pathname + (url.search ? url.search : ''));
      window.scrollTo({ top: 0, behavior: 'instant' });
    }
  }

  function openKeys(provider: Provider): void {
    activeProvider = provider;
    keysOpen = true;
  }

  function removeProvider(provider: Provider): void {
    providerToDelete = provider;
  }

  async function confirmRemoveProvider(): Promise<void> {
    if (!providerToDelete) return;
    const provider = providerToDelete;
    deletingId = provider.id;
    errorMessage = '';
    try {
      await api.deleteProvider(provider.id);
      providers = providers.filter((item) => item.id !== provider.id);
      if (activeProvider?.id === provider.id) activeProvider = null;
      if (viewingProviderId === provider.id) closeDetails();
      onProviderCountChange(providers.length);
      providerToDelete = null;
    } catch (error) {
      errorMessage = localizedError(error, 'Could not delete this provider.', tr);
    } finally {
      deletingId = '';
    }
  }

  function providerUpdated(updated: Provider): void {
    providers = providers.map((provider) => provider.id === updated.id ? updated : provider);
    activeProvider = updated;
  }

  onMount(() => {
    void load();
    const handlePopState = () => { checkUrlProvider(); };
    window.addEventListener('popstate', handlePopState);
    return () => {
      window.removeEventListener('popstate', handlePopState);
    };
  });
  onDestroy(() => { generation += 1; });
</script>

<div class="providers-view">
  {#if viewingProvider}
    <ProviderDetailsPage
      provider={viewingProvider}
      {tr}
      {locale}
      onBack={closeDetails}
      onManageKeys={openKeys}
      onProviderChanged={refreshProviders}
      onProviderUpdated={providerUpdated}
      onDelete={removeProvider}
    />
  {:else}
    <PageHeading title={tr('Providers')} subtitle={tr('Manage the upstream services behind your gateway.')} {tr}>
      <PageSearch bind:value={query} pageLabel={tr('Providers')} {tr} />
      <button type="button" class="primary-button" onclick={openCustomProvider}><Plus size={17} />{tr('Add provider')}</button>
    </PageHeading>

    {#if errorMessage}
      <GatewayError message={errorMessage} {tr} onRetry={load} />
    {:else if loading}
      <InlineLoading label={'Loading {page}…'} {tr} vars={{ page: tr('Providers').toLowerCase() }} />
    {:else}
      <!-- Quick Metrics Banner -->
      <section class="providers-metrics-banner" aria-label={tr('Quick Overview')}>
        <div class="metric-chip">
          <div class="metric-chip-icon providers-icon"><Server size={18} /></div>
          <div class="metric-chip-info">
            <strong>{providers.length}</strong>
            <span>{tr('Connected Providers')}</span>
          </div>
        </div>
        <div class="metric-chip">
          <div class="metric-chip-icon credentials-icon"><KeyRound size={18} /></div>
          <div class="metric-chip-info">
            <strong>{totalCredentials}</strong>
            <span>{tr('Total Credentials')}</span>
          </div>
        </div>
        <div class="metric-chip">
          <div class="metric-chip-icon models-icon"><Cpu size={18} /></div>
          <div class="metric-chip-info">
            <strong>{totalModels}</strong>
            <span>{tr('Saved Models')}</span>
          </div>
        </div>
        <div class="metric-chip">
          <div class="metric-chip-icon presets-icon"><Layers size={18} /></div>
          <div class="metric-chip-info">
            <strong>{availablePresets.length + 1}</strong>
            <span>{tr('Available Presets')}</span>
          </div>
        </div>
      </section>

      <!-- Section 1: Custom Provider -->
      <section class="provider-category-section">
        <div class="category-section-header">
          <div>
            <h3 class="category-title">{tr('Custom Provider')}</h3>
            <p class="category-subtitle">{tr('Connect an OpenAI-compatible API using an API key or custom header.')}</p>
          </div>
          {#if customProviders.length}
            <span class="category-count-badge">{customProviders.length} {tr('connected')}</span>
          {/if}
        </div>

        <div class="preset-cards-grid">
          <div
            class="preset-card custom-preset-card"
            role="button"
            tabindex="0"
            onkeydown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                openCustomProvider();
              }
            }}
            onclick={openCustomProvider}
          >
            <div class="preset-card-top">
              <div class="preset-card-icon custom">
                <Sliders size={22} />
              </div>
              <span class="preset-badge">
                OpenAI / REST
              </span>
            </div>

            <div class="preset-card-body">
              <h4>{tr('New Custom Provider')}</h4>
              <p>{tr('OpenAI-compatible endpoint, DeepSeek, Groq, Ollama, etc.')}</p>
            </div>

            <div class="preset-card-footer">
              <span class="preset-connect-cta">
                <Plus size={14} />
                {tr('Connect')}
              </span>
            </div>
          </div>

          {#each customProviders as provider (provider.id)}
            <ProviderCard
              {provider}
              {tr}
              deleting={deletingId === provider.id}
              onDetails={openDetails}
              onManageKeys={openKeys}
              onDelete={removeProvider}
            />
          {/each}
        </div>
      </section>

      <ProviderPresetCatalog
        presets={filteredAvailablePresets}
        {providers}
        {tr}
        {openingPresetId}
        onSelect={handlePresetSelect}
      />

      {#if query.trim() && !customProviders.length && !filteredAvailablePresets.length}
        <EmptyState icon="search" title={tr('No matching providers')} description={tr('Try a different search, or clear the filter.')} />
      {/if}
    {/if}
  {/if}
</div>

<ProviderCreateDialog bind:open={createOpen} {presets} {tr} onSaved={providerSaved} />
<ProviderKeysDialog bind:open={keysOpen} provider={activeProvider} {tr} {locale} onChanged={refreshProviders} />

<ArkDialog
  open={providerToDelete !== null}
  role="alertdialog"
  closeLabel={tr('Close dialog')}
  title={tr('Are you absolutely sure?')}
  kicker={tr('EXOROUTE CONTROL PLANE')}
  onClose={() => { providerToDelete = null; }}
>
  <div class="modal-form">
    <p class="modal-description">
      {tr('Delete {name}? This cannot be undone.', { name: providerToDelete?.name ?? '' })}
    </p>
    <div class="modal-actions" style="margin-top: 16px;">
      <button type="button" class="secondary-button" disabled={Boolean(deletingId)} onclick={() => { providerToDelete = null; }}>
        {tr('Cancel')}
      </button>
      <button
        type="button"
        class="primary-button danger"
        disabled={Boolean(deletingId)}
        onclick={confirmRemoveProvider}
      >
        {#if deletingId}<LoaderCircle size={14} class="spin" />{tr('Deleting…')}{:else}{tr('Delete provider')}{/if}
      </button>
    </div>
  </div>
</ArkDialog>

