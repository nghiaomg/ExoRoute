<script lang="ts">
  import Button from "../../components/Button.svelte";
  import StatsCard from "../../components/StatsCard.svelte";
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import SelectOptions from '../../components/SelectOptions.svelte';
  import Input from '../../components/Input.svelte';
  import type { DashboardPage } from '../../lib/navigation';
  import type { Locale } from '../../lib/i18n';
  import type { Translate } from '../../lib/format';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import type { Provider, ProviderKey, ProviderPreset, UpstreamProtocol } from '../../lib/types';
  import { providerLogo } from './provider-logo';
  import ProviderDetailsPage from './ProviderDetailsPage.svelte';
  import ProviderPresetSetupPage from './ProviderPresetSetupPage.svelte';

  interface Props {
    tr: Translate;
    locale: Locale;
    onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;
    onProviderCountChange: (count: number) => void;
  }

  let { tr, locale, onConnectionChange, onProviderCountChange }: Props = $props();

  let providers = $state<Provider[]>([]);
  let presets = $state<ProviderPreset[]>([]);
  let loading = $state(true);
  let errorMessage = $state('');
  let generation = 0;

  let search = $state('');
  let statusFilter = $state('all');

  // Keys dialog
  let keysDialog = $state<HTMLDialogElement>();
  let keysProvider = $state<Provider | null>(null);
  let keys = $state<ProviderKey[]>([]);
  let keysLoading = $state(false);
  let newKey = $state('');
  let keyBusy = $state(false);

  // Delete dialog
  let deleteDialog = $state<HTMLDialogElement>();
  let deleteTarget = $state<Provider | null>(null);
  let deleteBusy = $state(false);

  // Provider detail page: a full-page view addressed by ?id= so a reload or a
  // shared link reopens the same provider.
  let detailProviderId = $state<string | null>(null);

  // Test-all dialog
  let testDialog = $state<HTMLDialogElement>();
  let testRows = $state<{ id: string; name: string; state: 'pending' | 'testing' | 'ok' | 'fail'; detail: string }[]>([]);
  let testRunning = $state(false);
  let creatingPresetId = $state<string | null>(null);
  let setupPresetId = $state<string | null>(null);

  const presetById = $derived(new Map(presets.map((p) => [p.id, p])));
  const setupPreset = $derived(setupPresetId ? presetById.get(setupPresetId) ?? null : null);

  const providerCatalog = $derived.by(() => {
    const configuredAdapters = new Set(providers.map((provider) => provider.adapter_id));
    const entries: Array<{ key: string; preset: ProviderPreset; provider: Provider | null }> = [];
    for (const provider of providers) {
      const preset = presets.find((item) => item.adapter_id === provider.adapter_id || item.id === provider.adapter_id);
      if (preset) entries.push({ key: provider.id, preset, provider });
    }
    for (const preset of presets) {
      if (!configuredAdapters.has(preset.adapter_id)) entries.push({ key: 'preset:' + preset.id, preset, provider: null });
    }
    return entries;
  });

  const filteredProviders = $derived.by(() => {
    const q = search.trim().toLowerCase();
    return providerCatalog.filter(({ preset, provider }) => {
      if (provider && statusFilter === 'enabled' && !provider.enabled) return false;
      if (provider && statusFilter === 'disabled' && provider.enabled) return false;
      if (!q) return true;
      return [preset.name, preset.id, preset.adapter_id, preset.default_base_url ?? '', provider?.name ?? '', provider?.base_url ?? '']
        .some((value) => value.toLowerCase().includes(q));
    });
  });

  const enabledCount = $derived(providers.filter((p) => p.enabled).length);
  const totalKeys = $derived(providers.reduce((sum, p) => sum + (p.api_key_count ?? 0), 0));
  const totalModels = $derived(providers.reduce((sum, p) => sum + (p.model_count ?? 0), 0));
  const keylessCount = $derived(providers.filter((p) => p.enabled && !(p.api_key_count ?? 0)).length);

  /** The detail page mirrors the live provider record so edits, key changes,
      and toggles are reflected without reopening it. */
  const detailLive = $derived.by(() => {
    const id = detailProviderId;
    return id ? providers.find((p) => p.id === id) ?? null : null;
  });

  function providerType(provider: Provider): string {
    return presets.find((p) => p.adapter_id === provider.adapter_id || p.id === provider.adapter_id)?.name ?? provider.adapter_id;
  }

  function providerTypeLabel(preset: ProviderPreset): string {
    const labels = preset.labels.map((label) => tr(label)).join(', ');
    return labels ? labels : tr(CATEGORY_LABEL_KEYS[presetCategory(preset)]);
  }

  async function providerPreset(presetId: string): Promise<void> {
    if (creatingPresetId) return;
    const preset = presetById.get(presetId);
    if (!preset) return;

    const endpoint = preset.default_base_url?.trim();
    if (!endpoint) {
      setupPresetId = presetId;
      return;
    }

    creatingPresetId = presetId;
    try {
      const result = await api.createProvider({
        adapter_id: preset.adapter_id,
        name: preset.name,
        base_url: endpoint,
        logo_url: preset.default_logo_url ?? null,
        model_prefix: preset.default_model_prefix || undefined,
        enabled: true,
        auth_type: preset.supported_auth_types.includes('none') ? 'none' : preset.default_auth_type,
        api_keys: [],
        preferred_protocol: preset.default_preferred_protocol,
        supported_protocols: preset.default_supported_protocols as UpstreamProtocol[],
      });
      setupPresetId = null;
      const providerRows = await load();
      const provider = providerRows?.find((item) => item.id === result.id);
      if (!provider) {
        showDashboardToast(tr('Could not load providers.'));
        return;
      }
      openDetail(provider);
    } catch (error) {
      showDashboardToast(error instanceof Error ? error.message : tr('Could not save provider.'));
    } finally {
      creatingPresetId = null;
    }
  }

  /** Group order mirrors the preset catalog; adapters without a preset fall into Custom. */
  const CATEGORY_ORDER = ['cloud_api', 'gateway', 'oauth', 'custom'] as const;
  type ProviderGroupKey = (typeof CATEGORY_ORDER)[number];
  const CATEGORY_LABEL_KEYS: Record<ProviderGroupKey, string> = {
    cloud_api: 'Cloud API',
    gateway: 'Gateway',
    oauth: 'OAuth',
    custom: 'Custom',
  };

  function presetCategory(preset: ProviderPreset): ProviderGroupKey {
    return (CATEGORY_ORDER as readonly string[]).includes(preset.category)
      ? (preset.category as ProviderGroupKey)
      : 'custom';
  }

  const providerGroups = $derived.by(() => {
    const buckets: Record<ProviderGroupKey, typeof filteredProviders> = { cloud_api: [], gateway: [], oauth: [], custom: [] };
    for (const entry of filteredProviders) buckets[presetCategory(entry.preset)].push(entry);
    return CATEGORY_ORDER.map((key) => ({
      key,
      label: tr(CATEGORY_LABEL_KEYS[key]),
      items: buckets[key].sort((a, b) => a.preset.name.localeCompare(b.preset.name)),
    }));
  });

  async function load(): Promise<Provider[] | null> {
    const gen = ++generation;
    loading = true;
    errorMessage = '';
    onConnectionChange('loading');
    try {
      const [providerRows, presetRows] = await Promise.all([api.providers(), api.providerPresets()]);
      if (gen !== generation) return null;
      providers = providerRows;
      presets = presetRows;
      onProviderCountChange(providerRows.length);
      onConnectionChange('loaded');
      return providerRows;
    } catch (error) {
      if (gen !== generation) return null;
      errorMessage = error instanceof Error ? error.message : tr('Could not load providers.');
      onConnectionChange('error');
      return null;
    } finally {
      if (gen === generation) loading = false;
    }
  }



  async function toggleProvider(provider: Provider): Promise<void> {
    const next = !provider.enabled;
    providers = providers.map((p) => (p.id === provider.id ? { ...p, enabled: next } : p));
    try {
      await api.updateProvider(provider.id, { enabled: next });
      showDashboardToast(tr(next ? '{name} enabled' : '{name} disabled', { name: provider.name }));
    } catch (error) {
      providers = providers.map((p) => (p.id === provider.id ? { ...p, enabled: !next } : p));
      showDashboardToast(error instanceof Error ? error.message : tr('Update failed'));
    }
  }

  async function openKeys(provider: Provider): Promise<void> {
    keysProvider = provider;
    keys = [];
    newKey = '';
    keysDialog?.showModal();
    keysLoading = true;
    try {
      const page = await api.providerKeys(provider.id);
      keys = page.keys;
    } catch (error) {
      showDashboardToast(error instanceof Error ? error.message : tr('Could not load keys'));
    } finally {
      keysLoading = false;
    }
  }

  async function addKey(): Promise<void> {
    const value = newKey.trim();
    if (!keysProvider || !value || keyBusy) return;
    keyBusy = true;
    try {
      const res = await api.createProviderKey(keysProvider.id, undefined, value);
      newKey = '';
      showDashboardToast(res.test_passed ? tr('Key added — test passed') : tr('Key added — test {status}', { status: res.status ?? 'failed' }));
      const page = await api.providerKeys(keysProvider.id);
      keys = page.keys;
      await load();
    } catch (error) {
      showDashboardToast(error instanceof Error ? error.message : tr('Could not add key'));
    } finally {
      keyBusy = false;
    }
  }

  async function removeKey(key: ProviderKey): Promise<void> {
    if (!keysProvider) return;
    try {
      await api.deleteProviderKey(keysProvider.id, key.id);
      keys = keys.filter((k) => k.id !== key.id);
      showDashboardToast(tr('Key removed'));
      await load();
    } catch (error) {
      showDashboardToast(error instanceof Error ? error.message : tr('Could not remove key'));
    }
  }

  function openDelete(provider: Provider): void {
    deleteTarget = provider;
    deleteDialog?.showModal();
  }

  /** Opens the full-page provider detail view and records it in the URL. */
  async function finishPresetSetup(providerId: string): Promise<void> {
    const providerRows = await load();
    const provider = providerRows?.find((item) => item.id === providerId);
    if (provider) openDetail(provider);
  }

  function openDetail(provider: Provider): void {
    detailProviderId = provider.id;
    try {
      const url = new URL(window.location.href);
      url.searchParams.set('id', provider.id);
      window.history.pushState(null, '', url);
    } catch {
      // Ignore in sandboxed contexts.
    }
    window.scrollTo({ top: 0, behavior: 'instant' });
  }

  function closeDetail(): void {
    detailProviderId = null;
    try {
      const url = new URL(window.location.href);
      url.searchParams.delete('id');
      window.history.pushState(null, '', url);
    } catch {
      // Ignore in sandboxed contexts.
    }
  }

  /** Keeps the detail view in sync with ?id= after reload, back, or forward. */
  function syncDetailFromUrl(): void {
    try {
      const requested = new URL(window.location.href).searchParams.get('id');
      detailProviderId = requested;
    } catch {
      detailProviderId = null;
    }
  }

  /** Runs an action from the detail page and leaves it so the target dialog is visible. */
  function closeDetailThen(action: (provider: Provider) => void): void {
    const provider = detailLive;
    closeDetail();
    if (provider) action(provider);
  }

  async function confirmDelete(): Promise<void> {
    if (!deleteTarget || deleteBusy) return;
    deleteBusy = true;
    try {
      await api.deleteProvider(deleteTarget.id);
      providers = providers.filter((p) => p.id !== deleteTarget?.id);
      onProviderCountChange(providers.length);
      showDashboardToast(tr('Provider "{name}" deleted', { name: deleteTarget.name }));
      deleteDialog?.close();
    } catch (error) {
      showDashboardToast(error instanceof Error ? error.message : tr('Could not delete provider'));
    } finally {
      deleteBusy = false;
    }
  }

  async function syncModels(): Promise<void> {
    const targets = providers.filter((p) => p.enabled);
    if (!targets.length) { showDashboardToast(tr('No enabled providers to sync')); return; }
    let imported = 0;
    for (const p of targets) {
      try {
        const res = await api.importProviderModels(p.id);
        imported += res.models?.length ?? 0;
      } catch { /* per-provider failure is non-fatal */ }
    }
    showDashboardToast(tr('Model sync complete — {count} refreshed', { count: imported || tr('catalog') }));
    await load();
  }

  async function runAllTests(): Promise<void> {
    const targets = providers.filter((p) => p.enabled);
    testRows = targets.map((p) => ({ id: p.id, name: p.name, state: 'pending', detail: '' }));
    testRunning = true;
    testDialog?.showModal();
    for (let i = 0; i < testRows.length; i++) {
      const row = testRows[i];
      testRows[i] = { ...row, state: 'testing', detail: tr('testing…') };
      testRows = [...testRows];
      try {
        const catalog = await api.providerModels(row.id, { limit: 1 });
        const count = catalog.models?.length ?? 0;
        testRows[i] = {
          ...testRows[i],
          state: 'ok',
          detail: count ? `${tr('available')} · ${count} ${tr('models')}` : tr('No models found yet'),
        };
      } catch {
        testRows[i] = { ...testRows[i], state: 'fail', detail: tr('unreachable') };
      }
      testRows = [...testRows];
    }
    testRunning = false;
  }

  function fmtKeys(provider: Provider): string {
    const n = provider.api_key_count ?? 0;
    return `${n} ${tr(n === 1 ? 'key' : 'keys')}`;
  }

  function fmtLastUsed(key: ProviderKey): string {
    if (!key.last_used_at) return tr('never used');
    const diff = Date.now() - new Date(key.last_used_at).getTime();
    if (diff < 60_000) return tr('just now');
    if (diff < 3_600_000) return tr('{count}m ago', { count: Math.floor(diff / 60_000) });
    if (diff < 86_400_000) return tr('{count}h ago', { count: Math.floor(diff / 3_600_000) });
    return tr('{count}d ago', { count: Math.floor(diff / 86_400_000) });
  }

  onMount(() => {
    // A reload or a shared link carries the provider id in the query string.
    syncDetailFromUrl();
    const handlePopState = () => syncDetailFromUrl();
    window.addEventListener('popstate', handlePopState);
    void load();
    return () => window.removeEventListener('popstate', handlePopState);
  });
  onDestroy(() => { generation += 1; });
</script>

<div class="providers-view">
{#if setupPreset}
  <ProviderPresetSetupPage
    preset={setupPreset}
    {tr}
    onBack={() => { setupPresetId = null; }}
    onCreated={(providerId) => { setupPresetId = null; void finishPresetSetup(providerId); }}
  />
{:else if detailLive}
  <ProviderDetailsPage
    provider={detailLive}
    {tr}
    {locale}
    onBack={closeDetail}
    onManageKeys={(provider) => void openKeys(provider)}
    onProviderChanged={() => void load()}
    onProviderUpdated={(updated) => { providers = providers.map((p) => (p.id === updated.id ? updated : p)); }}
    onDelete={(provider) => openDelete(provider)}
  />
{:else}
<div class="page-heading">
  <div class="heading-copy">
    <h1>{tr('Providers')}</h1>
    <p>{tr('Manage upstream AI providers, credentials, and synced models.')}</p>
  </div>
  <div class="heading-actions">
    <Button class="action-button" type="button" onclick={runAllTests} disabled={testRunning}>
      <span class="button-icon"><svg class="icon"><use href="#i-zap" /></svg></span>
      <span class="button-label">{tr('Test all')}</span>
    </Button>
  </div>
</div>

{#if errorMessage}
  <section class="course-card surface" aria-label={tr('Load error')}>
    <div class="card-title-row"><h2>{tr('Could not load providers')}</h2></div>
    <p class="card-subtitle">{errorMessage}</p>
    <div class="modal-actions" style="padding-top:14px">
      <Button class="page-action primary disabled:opacity-55 disabled:cursor-default" type="button" onclick={load}>{tr('Retry')}</Button>
    </div>
  </section>
{:else if loading}
  <section
    class="grid min-h-0 grid-cols-[repeat(4,minmax(0,1fr))] gap-[2%] max-[680px]:grid-cols-[repeat(2,minmax(0,1fr))] max-[680px]:gap-[8px]"
    aria-label={tr('Loading')}
  >
    {#each [0, 1, 2, 3] as i (i)}<StatsCard skeleton />{/each}
  </section>
{:else}
  <div class="page-surface">
    <section
      class="grid min-h-0 grid-cols-[repeat(4,minmax(0,1fr))] gap-[2%] max-[680px]:grid-cols-[repeat(2,minmax(0,1fr))] max-[680px]:gap-[8px]"
      aria-label={tr('Provider overview')}
    >
      <StatsCard icon="#i-link" value={providers.length} title={tr('Total Providers')} trend={tr('{enabled} enabled, {disabled} disabled', { enabled: enabledCount, disabled: providers.length - enabledCount })} actionLabel={tr('View all')} onAction={ () => (statusFilter = 'all') } />
      <StatsCard icon="#i-zap" value={enabledCount} title={tr('Enabled')} trend={tr('Serving live traffic')} actionLabel={tr('Show enabled only')} onAction={ () => (statusFilter = 'enabled') } />
      <StatsCard icon="#i-key" value={totalKeys} title={tr('Credentials')} trend={tr('Across {count} providers', { count: providers.filter((p) => (p.api_key_count ?? 0) > 0).length })} actionLabel={tr('Manage keys')} onAction={ () => { if (providers[0]) void openKeys(providers[0]); } } />
      <StatsCard icon="#i-refresh" value={totalModels} title={tr('Synced Models')} trend={keylessCount ? tr('{count} enabled without keys', { count: keylessCount }) : tr('All enabled providers have keys')} actionLabel={tr('Sync now')} onAction={ syncModels } />
    </section>

    <section class="course-card surface" aria-labelledby="providers-heading">
      <div class="card-title-row">
        <h2 id="providers-heading">{tr('Providers')}</h2>
        <Button class="mini-add" type="button" onclick={() => void load()} disabled={loading}>
          <svg class="icon"><use href="#i-refresh" /></svg>
          {tr('Refresh current page')}
        </Button>
      </div>
      <p class="card-subtitle">{tr('{providers} providers · {enabled} enabled · {keys} credentials', { providers: providers.length, enabled: enabledCount, keys: totalKeys })}</p>
      <div class="page-toolbar provider-toolbar mt-3 items-center">
        <Input class="min-h-[34px]! w-[min(280px,100%)]! flex-[1_1_200px] rounded-[10px]! border-line! bg-surface! px-3! text-muted! text-dash-body! focus:outline-none! focus:border-[color-mix(in_srgb,var(--accent)_55%,var(--surface))]! focus:ring-3! focus:ring-db-accent-ring!" type="search" placeholder={tr('Search providers...')} aria-label={tr('Search providers')} bind:value={search} />
        <SelectOptions
          class="prov-select"
          ariaLabel={tr('Filter by status')}
          bind:value={statusFilter}
          items={[{ label: tr('All statuses'), value: 'all' }, { label: tr('Enabled'), value: 'enabled' }, { label: tr('Disabled'), value: 'disabled' }]}
        />
      </div>
      <div class="grid gap-[18px] mt-[16px]" aria-live="polite">
        {#each providerGroups as group (group.key)}
          {#if group.items.length}
            <section class="provider-group">
              <h3 class="flex items-center gap-[7px] m-0 mb-[8px] text-muted text-caption font-bold uppercase [letter-spacing:var(--tracking-caps)]">
                {group.label}
                <span class="rounded-full bg-surface-sunken px-[7px] py-[1px] text-muted text-meta font-bold">{group.items.length}</span>
              </h3>
              <div class="course-list mt-3! grid! grid-cols-[repeat(auto-fill,minmax(min(100%,360px),1fr))]! content-start! gap-3!">
                {#each group.items as entry (entry.key)}
                  {@const { preset, provider } = entry}
                  {@const logo = providerLogo(provider?.logo_url ?? preset.default_logo_url, preset.adapter_id, provider?.id ?? preset.id, provider?.name ?? preset.name)}
                  {#if provider}
                    <!-- Existing records open the detail page; the trailing hint stays the single action. -->
                    <article class="course-row min-h-[88px]! gap-3! rounded-[11px]! border! border-line! bg-surface! p-0! cursor-pointer! transition-colors! hover:bg-db-tint-strong!" data-status={provider.enabled ? 'enabled' : 'disabled'}>
                      <Button class="provider-row-trigger group flex w-full min-w-0 items-center gap-3 rounded-[inherit] border-0 bg-transparent p-[13px] text-left text-inherit cursor-pointer focus-visible:outline-2 focus-visible:outline-[color-mix(in_srgb,var(--accent)_55%,var(--surface))] focus-visible:outline-offset-[-2px] [&_.course-copy]:grid [&_.course-copy]:gap-0.5 [&_.course-copy]:min-w-0 max-nav:flex-wrap max-nav:[&_.course-copy]:flex-[1_1_calc(100%-60px)] max-nav:[&_.status]:order-3 max-nav:[&>div]:order-4 max-nav:[&>div]:ms-auto" type="button" aria-label={tr('Open {name} details', { name: provider.name })} onclick={() => openDetail(provider)}>
                        <span class="course-icon provider-logo">
                          {#if logo}<img src={logo} alt="" aria-hidden="true" />{:else}<svg class="icon"><use href="#i-zap" /></svg>{/if}
                        </span>
                        <span class="course-copy">
                          <strong>{provider.name} <span class="ml-[6px] rounded-full bg-surface-sunken px-[7px] py-[2px] text-muted text-caption font-medium [vertical-align:1px]">{preset.name}</span></strong>
                          <span>{provider.base_url} · {fmtKeys(provider)} · {provider.model_count ?? 0} {tr('models')}</span>
                        </span>
                        {#if provider.enabled && !(provider.api_key_count ?? 0)}
                          <span class="status pending">{tr('No keys')}</span>
                        {:else}
                          <span class="status {provider.enabled ? 'complete' : 'pending'}">{provider.enabled ? tr('Enabled') : tr('Disabled')}</span>
                        {/if}
                        <span class="grid w-[22px] h-[22px] shrink-0 place-items-center rounded-full bg-surface-sunken text-text-dim group-hover:bg-accent group-hover:text-accent-contrast" aria-hidden="true"><svg class="icon w-[12px] h-[12px]"><use href="#i-arrow-right" /></svg></span>
                      </Button>
                    </article>
                  {:else}
                    <!-- A catalog preset is a single click target, like an existing provider. -->
                    <article class="course-row min-h-[88px]! gap-3! rounded-[11px]! border! border-line! bg-surface! p-0! cursor-pointer! transition-colors! hover:bg-db-tint-strong!">
                      <Button class="provider-row-trigger group flex w-full min-w-0 items-center gap-3 rounded-[inherit] border-0 bg-transparent p-[13px] text-left text-inherit cursor-pointer focus-visible:outline-2 focus-visible:outline-[color-mix(in_srgb,var(--accent)_55%,var(--surface))] focus-visible:outline-offset-[-2px] [&_.course-copy]:grid [&_.course-copy]:gap-0.5 [&_.course-copy]:min-w-0 max-nav:flex-wrap max-nav:[&_.course-copy]:flex-[1_1_calc(100%-60px)] max-nav:[&_.status]:order-3 max-nav:[&>div]:order-4 max-nav:[&>div]:ms-auto" type="button" aria-label={tr('Open {name} details', { name: preset.name })} disabled={creatingPresetId !== null} onclick={() => void providerPreset(preset.id)}>
                        <span class="course-icon provider-logo">
                          {#if logo}<img src={logo} alt="" aria-hidden="true" />{:else}<svg class="icon"><use href="#i-zap" /></svg>{/if}
                        </span>
                        <span class="course-copy">
                          <strong>{preset.name} <span class="ml-[6px] rounded-full bg-surface-sunken px-[7px] py-[2px] text-muted text-caption font-medium [vertical-align:1px]">{providerTypeLabel(preset)}</span></strong>
                          <span>{preset.default_base_url ?? preset.adapter_id}</span>
                        </span>
                        {#if creatingPresetId === preset.id}
                          <span class="status progress">{tr('Creating…')}</span>
                        {:else}
                          <span class="grid w-[22px] h-[22px] shrink-0 place-items-center rounded-full bg-surface-sunken text-text-dim group-hover:bg-accent group-hover:text-accent-contrast" aria-hidden="true"><svg class="icon w-[12px] h-[12px]"><use href="#i-arrow-right" /></svg></span>
                        {/if}
                      </Button>
                    </article>
                  {/if}
                {/each}
              </div>
            </section>
          {/if}
        {/each}
        {#if !filteredProviders.length}
          <div class="empty-state">{tr('No providers match the current filters.')}</div>
        {/if}
      </div>
    </section>
  </div>
{/if}
{/if}
</div>


<!-- Manage API keys -->
<dialog class="modal modal-wide w-[min(94vw,560px)]!" bind:this={keysDialog} aria-labelledby="keys-dialog-title">
  <div class="modal-card">
    <div class="modal-heading">
      <div>
        <h2 id="keys-dialog-title">{tr('API keys')}</h2>
        <p>{tr('Credentials for')} <strong>{keysProvider?.name ?? tr('provider')}</strong>.</p>
      </div>
      <Button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</Button>
    </div>
    <div class="grid max-h-60 gap-2 overflow-y-auto">
      {#if keysLoading}
        <p class="card-subtitle" style="padding:12px 4px">{tr('Loading keys…')}</p>
      {:else if keys.length === 0}
        <p class="card-subtitle" style="padding:12px 4px">{tr('No keys configured.')}</p>
      {:else}
        {#each keys as key (key.id)}
          <div class="flex items-center justify-between gap-2.5 rounded-[11px] border border-line bg-db-tint px-3 py-[9px] [&_code]:text-ink! [&_code]:text-meta!">
            <div class="course-copy">
              <strong>{key.name || tr('Unnamed key')} <span class="ml-[6px] rounded-full bg-surface-sunken px-[7px] py-[2px] text-muted text-caption font-medium [vertical-align:1px]">{key.credential_type ?? 'api_key'}</span></strong>
              <span>{key.enabled ? (key.invalid ? tr('marked invalid') : tr('healthy')) : tr('disabled')} · {fmtLastUsed(key)}</span>
            </div>
            <div class="flex gap-1.5">
              <Button class="icon-action danger" type="button" title={tr('Remove key')} onclick={() => removeKey(key)}><svg class="icon"><use href="#i-trash" /></svg></Button>
            </div>
          </div>
        {/each}
      {/if}
    </div>
    <form class="flex gap-2 [&_input]:min-h-[38px]! [&_input]:flex-1! [&_input]:rounded-[10px]! [&_input]:border! [&_input]:border-line! [&_input]:bg-db-tint! [&_input]:px-3! [&_input]:text-ink! [&_input]:text-dash-body! [&_input:focus]:outline-0! [&_input:focus]:ring-3! [&_input:focus]:ring-db-accent-ring!" onsubmit={(e) => { e.preventDefault(); void addKey(); }}>
      <Input bind:value={newKey} type="password" placeholder={tr('Paste a new API key…')} autocomplete="off" />
      <Button class="page-action primary disabled:opacity-55 disabled:cursor-default" type="submit" disabled={keyBusy || !newKey.trim()}>{keyBusy ? tr('Testing…') : tr('Add key')}</Button>
    </form>
    <div class="modal-actions">
      <Button class="page-action disabled:opacity-55 disabled:cursor-default" type="button" data-close-modal>{tr('Close')}</Button>
    </div>
  </div>
</dialog>

<!-- Delete confirm -->
<dialog class="modal" bind:this={deleteDialog} aria-labelledby="delete-dialog-title">
  <div class="modal-card">
    <div class="modal-heading">
      <div>
        <h2 id="delete-dialog-title">{tr('Delete provider?')}</h2>
        <p>{tr('This removes {name} and its credentials. Combos referencing it will lose that target.', { name: deleteTarget?.name ?? tr('the provider') })}</p>
      </div>
      <Button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</Button>
    </div>
    <div class="modal-actions">
      <Button class="page-action disabled:opacity-55 disabled:cursor-default" type="button" data-close-modal>{tr('Cancel')}</Button>
      <Button class="page-action danger" type="button" onclick={confirmDelete} disabled={deleteBusy}>{deleteBusy ? tr('Deleting…') : tr('Delete provider')}</Button>
    </div>
  </div>
</dialog>

<!-- Test all results -->
<dialog class="modal" bind:this={testDialog} aria-labelledby="test-all-title">
  <div class="modal-card">
    <div class="modal-heading">
      <div>
        <h2 id="test-all-title">{tr('Connection test')}</h2>
        <p>{tr('Test all')}</p>
      </div>
      <Button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</Button>
    </div>
    <div class="grid gap-[8px]">
      {#each testRows as row (row.id)}
        <div
          class="flex items-center justify-between rounded-[11px] border border-line bg-db-tint px-[13px] py-[10px] text-dash-body"
          data-state={row.state}
        >
          <span class="text-ink font-medium">{row.name}</span>
          <span
            class="text-muted text-meta [[data-state=ok]_&]:text-db-success [[data-state=fail]_&]:text-db-danger [[data-state=ok]_&]:font-semibold [[data-state=fail]_&]:font-semibold [[data-state=pending]_&]:after:content-[''] [[data-state=pending]_&]:after:inline-block [[data-state=pending]_&]:after:w-[10px] [[data-state=pending]_&]:after:h-[10px] [[data-state=pending]_&]:after:ml-[6px] [[data-state=pending]_&]:after:rounded-full [[data-state=pending]_&]:after:border-2 [[data-state=pending]_&]:after:border-text-dim [[data-state=pending]_&]:after:border-t-accent [[data-state=pending]_&]:after:animate-spin [[data-state=pending]_&]:after:[vertical-align:-1px]"
          >{row.detail}</span>
        </div>
      {/each}
    </div>
    <div class="modal-actions">
      <Button class="page-action disabled:opacity-55 disabled:cursor-default" type="button" data-close-modal>{tr('Close')}</Button>
      <Button class="page-action primary disabled:opacity-55 disabled:cursor-default" type="button" onclick={runAllTests} disabled={testRunning}>{testRunning ? tr('Running…') : tr('Run again')}</Button>
    </div>
  </div>
</dialog>
