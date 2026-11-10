<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import type { DashboardPage } from '../../lib/navigation';
  import type { Locale } from '../../lib/i18n';
  import type { Translate } from '../../lib/format';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import type { Provider, ProviderKey, ProviderPreset, UpstreamProtocol } from '../../lib/types';
  import { providerLogo } from './provider-logo';
  import ProviderDetailsPage from './ProviderDetailsPage.svelte';
  import '../../styles/dashboard/pages/providers.css';

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

  // Provider dialog
  let providerDialog = $state<HTMLDialogElement>();
  let editingProvider = $state<Provider | null>(null);
  let pfPreset = $state('');
  let pfName = $state('');
  let pfUrl = $state('');
  let pfKey = $state('');
  let pfModel = $state('');
  let pfBusy = $state(false);
  let pfError = $state('');
  let pfNote = $state('');

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

  const presetById = $derived(new Map(presets.map((p) => [p.id, p])));
  const customPreset = $derived(presets.find((p) => p.id === 'custom' || p.adapter_id === 'generic') ?? presets[0] ?? null);

  const filteredProviders = $derived.by(() => {
    const q = search.trim().toLowerCase();
    return providers.filter((p) => {
      if (statusFilter === 'enabled' && !p.enabled) return false;
      if (statusFilter === 'disabled' && p.enabled) return false;
      if (!q) return true;
      return p.name.toLowerCase().includes(q) || p.base_url.toLowerCase().includes(q) || p.adapter_id.toLowerCase().includes(q);
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

  /** Group order mirrors the preset catalog; adapters without a preset fall into Custom. */
  const CATEGORY_ORDER = ['cloud_api', 'gateway', 'oauth', 'custom'] as const;
  type ProviderGroupKey = (typeof CATEGORY_ORDER)[number];
  const CATEGORY_LABEL_KEYS: Record<ProviderGroupKey, string> = {
    cloud_api: 'Cloud API',
    gateway: 'Gateway',
    oauth: 'OAuth',
    custom: 'Custom',
  };

  function providerCategory(provider: Provider): ProviderGroupKey {
    const preset = presets.find((p) => p.adapter_id === provider.adapter_id || p.id === provider.adapter_id);
    return preset && (CATEGORY_ORDER as readonly string[]).includes(preset.category)
      ? (preset.category as ProviderGroupKey)
      : 'custom';
  }

  const providerGroups = $derived.by(() => {
    const buckets: Record<ProviderGroupKey, Provider[]> = { cloud_api: [], gateway: [], oauth: [], custom: [] };
    for (const provider of filteredProviders) buckets[providerCategory(provider)].push(provider);
    return CATEGORY_ORDER.map((key) => ({
      key,
      label: tr(CATEGORY_LABEL_KEYS[key]),
      items: buckets[key].sort((a, b) => a.name.localeCompare(b.name)),
    }));
  });

  async function load(): Promise<void> {
    const gen = ++generation;
    loading = true;
    errorMessage = '';
    onConnectionChange('loading');
    try {
      const [providerRows, presetRows] = await Promise.all([api.providers(), api.providerPresets()]);
      if (gen !== generation) return;
      providers = providerRows;
      presets = presetRows;
      onProviderCountChange(providerRows.length);
      onConnectionChange('loaded');
    } catch (error) {
      if (gen !== generation) return;
      errorMessage = error instanceof Error ? error.message : tr('Could not load providers.');
      onConnectionChange('error');
    } finally {
      if (gen === generation) loading = false;
    }
  }

  function openProviderDialog(provider: Provider | null): void {
    editingProvider = provider;
    pfError = '';
    pfNote = '';
    pfBusy = false;
    if (provider) {
      pfPreset = presets.find((p) => p.adapter_id === provider.adapter_id)?.id ?? customPreset?.id ?? '';
      pfName = provider.name;
      pfUrl = provider.base_url;
      pfKey = '';
      pfModel = provider.model_prefix ?? '';
    } else {
      pfPreset = customPreset?.id ?? '';
      pfName = '';
      pfUrl = '';
      pfKey = '';
      pfModel = '';
    }
    providerDialog?.showModal();
  }

  async function saveProvider(): Promise<void> {
    const name = pfName.trim();
    const baseUrl = pfUrl.trim();
    if (!name || !baseUrl || pfBusy) return;
    pfBusy = true;
    pfError = '';
    pfNote = '';
    try {
      if (editingProvider) {
        const res = await api.updateProvider(editingProvider.id, {
          name,
          base_url: baseUrl,
          model_prefix: pfModel.trim() || undefined,
        });
        const failed = res.key_tests.filter((t) => !t.test_passed).length;
        pfNote = failed ? tr('{failed} API key test(s) failed; keys were kept.', { failed }) : '';
        showDashboardToast(tr('Provider "{name}" updated', { name }));
      } else {
        const preset = presetById.get(pfPreset) ?? customPreset;
        const keys = pfKey.trim() ? [pfKey.trim()] : [];
        const res = await api.createProvider({
          adapter_id: preset?.adapter_id ?? 'generic',
          name,
          base_url: baseUrl,
          logo_url: preset?.default_logo_url ?? null,
          model_prefix: pfModel.trim() || undefined,
          enabled: true,
          auth_type: keys.length ? (preset?.default_auth_type ?? 'bearer') : 'none',
          api_keys: keys,
          preferred_protocol: preset?.default_preferred_protocol ?? 'chat_completions',
          supported_protocols: preset?.default_supported_protocols?.length
            ? (preset.default_supported_protocols as UpstreamProtocol[])
            : ['chat_completions', 'responses', 'messages'],
        });
        const failed = res.key_tests.filter((t) => !t.test_passed).length;
        pfNote = failed ? tr('{failed} API key test(s) failed; keys were still saved.', { failed }) : '';
        showDashboardToast(tr('Provider "{name}" created', { name }));
      }
      providerDialog?.close();
      await load();
    } catch (error) {
      pfError = error instanceof Error ? error.message : tr('Could not save provider.');
    } finally {
      pfBusy = false;
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

  function applyPreset() {
    const preset = presetById.get(pfPreset) ?? customPreset;
    if (!preset || editingProvider) return;
    if (!pfUrl) pfUrl = preset.default_base_url ?? '';
    if (!pfModel && preset.default_model_prefix) pfModel = preset.default_model_prefix;
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
{#if detailLive}
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
    <button class="action-button" type="button" onclick={runAllTests} disabled={testRunning}>
      <span class="button-icon"><svg class="icon"><use href="#i-zap" /></svg></span>
      <span class="button-label">{tr('Test all')}</span>
    </button>
    <button class="action-button primary" type="button" onclick={() => openProviderDialog(null)}>
      <span class="button-icon"><svg class="icon"><use href="#i-plus" /></svg></span>
      <span class="button-label">{tr('Add provider')}</span>
    </button>
  </div>
</div>

{#if errorMessage}
  <section class="course-card surface" aria-label={tr('Load error')}>
    <div class="card-title-row"><h2>{tr('Could not load providers')}</h2></div>
    <p class="card-subtitle">{errorMessage}</p>
    <div class="modal-actions" style="padding-top:14px">
      <button class="page-action primary" type="button" onclick={load}>{tr('Retry')}</button>
    </div>
  </section>
{:else if loading}
  <section class="stats-grid" aria-label={tr('Loading')}>
    {#each [0, 1, 2, 3] as i (i)}<article class="stat-card surface" style="min-height:132px"></article>{/each}
  </section>
{:else}
  <div class="page-surface">
    <section class="stats-grid" aria-label={tr('Provider overview')}>
      <article class="stat-card surface">
        <div class="stat-top">
          <span class="stat-icon"><svg class="icon"><use href="#i-link" /></svg></span>
          <span class="stat-number">{providers.length}</span>
        </div>
        <h2>{tr('Total Providers')}</h2>
        <div class="stat-trend">{tr('{enabled} enabled, {disabled} disabled', { enabled: enabledCount, disabled: providers.length - enabledCount })}</div>
        <button class="details-link" type="button" onclick={() => (statusFilter = 'all')}>{tr('View all')} <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top">
          <span class="stat-icon"><svg class="icon"><use href="#i-zap" /></svg></span>
          <span class="stat-number">{enabledCount}</span>
        </div>
        <h2>{tr('Enabled')}</h2>
        <div class="stat-trend">{tr('Serving live traffic')}</div>
        <button class="details-link" type="button" onclick={() => (statusFilter = 'enabled')}>{tr('Show enabled only')} <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top">
          <span class="stat-icon"><svg class="icon"><use href="#i-key" /></svg></span>
          <span class="stat-number">{totalKeys}</span>
        </div>
        <h2>{tr('Credentials')}</h2>
        <div class="stat-trend">{tr('Across {count} providers', { count: providers.filter((p) => (p.api_key_count ?? 0) > 0).length })}</div>
        <button class="details-link" type="button" onclick={() => { if (providers[0]) void openKeys(providers[0]); }}>{tr('Manage keys')} <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top">
          <span class="stat-icon"><svg class="icon"><use href="#i-refresh" /></svg></span>
          <span class="stat-number">{totalModels}</span>
        </div>
        <h2>{tr('Synced Models')}</h2>
        <div class="stat-trend">{keylessCount ? tr('{count} enabled without keys', { count: keylessCount }) : tr('All enabled providers have keys')}</div>
        <button class="details-link" type="button" onclick={syncModels}>{tr('Sync now')} <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
    </section>

    <section class="course-card surface provider-section" aria-labelledby="providers-heading">
      <div class="card-title-row">
        <h2 id="providers-heading">{tr('Providers')}</h2>
        <button class="mini-add" type="button" onclick={syncModels}>
          <svg class="icon"><use href="#i-refresh" /></svg>
          {tr('Sync models')}
        </button>
      </div>
      <p class="card-subtitle">{tr('{providers} providers · {enabled} enabled · {keys} credentials', { providers: providers.length, enabled: enabledCount, keys: totalKeys })}</p>
      <div class="page-toolbar provider-toolbar">
        <input class="prov-search" type="search" placeholder={tr('Search providers...')} aria-label={tr('Search providers')} bind:value={search} />
        <select class="prov-select" aria-label={tr('Filter by status')} bind:value={statusFilter}>
          <option value="all">{tr('All statuses')}</option>
          <option value="enabled">{tr('Enabled')}</option>
          <option value="disabled">{tr('Disabled')}</option>
        </select>
      </div>
      <div class="provider-groups" aria-live="polite">
        {#each providerGroups as group (group.key)}
          {#if group.items.length}
            <section class="provider-group">
              <h3 class="provider-group-title">
                {group.label}
                <span class="provider-group-count">{group.items.length}</span>
              </h3>
              <div class="course-list provider-list">
                {#each group.items as provider (provider.id)}
                  {@const logo = providerLogo(provider.logo_url, provider.adapter_id, provider.id, provider.name)}
                  <!-- The whole row is the only affordance: all actions live in the detail sheet. -->
                  <article class="course-row provider-row provider-row-button" data-status={provider.enabled ? 'enabled' : 'disabled'}>
                    <button
                      class="provider-row-trigger"
                      type="button"
                      aria-label={tr('Open {name} details', { name: provider.name })}
                      onclick={() => openDetail(provider)}
                    >
                      <span class="course-icon provider-logo">
                        {#if logo}
                          <img src={logo} alt="" aria-hidden="true" />
                        {:else}
                          <svg class="icon"><use href="#i-zap" /></svg>
                        {/if}
                      </span>
                      <span class="course-copy">
                        <strong>{provider.name} <span class="prov-type">{providerType(provider)}</span></strong>
                        <span>{provider.base_url} · {fmtKeys(provider)} · {provider.model_count ?? 0} {tr('models')}</span>
                      </span>
                      {#if provider.enabled && !(provider.api_key_count ?? 0)}
                        <span class="status pending">{tr('No keys')}</span>
                      {:else}
                        <span class="status {provider.enabled ? 'complete' : 'pending'}">{provider.enabled ? tr('Enabled') : tr('Disabled')}</span>
                      {/if}
                      <span class="detail-hint" aria-hidden="true"><svg class="icon"><use href="#i-arrow-right" /></svg></span>
                    </button>
                  </article>
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


<!-- Add / edit provider -->
<dialog class="modal" bind:this={providerDialog} aria-labelledby="provider-dialog-title" onclose={() => { pfError = ''; pfNote = ''; }}>
  <form class="modal-card" onsubmit={(e) => { e.preventDefault(); void saveProvider(); }}>
    <div class="modal-heading">
      <div>
        <h2 id="provider-dialog-title">{editingProvider ? tr('Edit provider') : tr('Add provider')}</h2>
        <p>{editingProvider ? tr('Update {name}', { name: editingProvider.name }) : tr('Connect an upstream AI provider.')}</p>
      </div>
      <button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</button>
    </div>
    <div class="modal-fields">
      <label class="field prov-field-wide">
        {tr('Provider name')}
        <input bind:value={pfName} maxlength="60" required placeholder={tr('e.g. OpenAI')} />
      </label>
      <label class="field">
        {tr('Type')}
        <select bind:value={pfPreset} onchange={applyPreset} disabled={!!editingProvider}>
          {#each presets as preset (preset.id)}
            <option value={preset.id}>{preset.name}</option>
          {/each}
        </select>
      </label>
      <label class="field">
        {tr('Base URL')}
        <input bind:value={pfUrl} type="url" required placeholder="https://api.example.com/v1" />
      </label>
      <label class="field">
        {editingProvider ? tr('API key (leave blank to keep existing)') : tr('API key')}
        <input bind:value={pfKey} type="password" placeholder="sk-..." autocomplete="off" />
      </label>
      <label class="field prov-field-wide">
        {tr('Model prefix (optional)')}
        <input bind:value={pfModel} maxlength="120" placeholder={tr('e.g. gpt-4o-mini')} />
      </label>
    </div>
    {#if pfError}<p class="form-error" role="alert">{pfError}</p>{/if}
    {#if pfNote}<p class="form-note">{pfNote}</p>{/if}
    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>{tr('Cancel')}</button>
      <button class="page-action primary" type="submit" disabled={pfBusy}>{pfBusy ? tr('Saving…') : tr('Save provider')}</button>
    </div>
  </form>
</dialog>

<!-- Manage API keys -->
<dialog class="modal modal-wide" bind:this={keysDialog} aria-labelledby="keys-dialog-title">
  <div class="modal-card">
    <div class="modal-heading">
      <div>
        <h2 id="keys-dialog-title">{tr('API keys')}</h2>
        <p>{tr('Credentials for')} <strong>{keysProvider?.name ?? tr('provider')}</strong>.</p>
      </div>
      <button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</button>
    </div>
    <div class="keys-list">
      {#if keysLoading}
        <p class="card-subtitle" style="padding:12px 4px">{tr('Loading keys…')}</p>
      {:else if keys.length === 0}
        <p class="card-subtitle" style="padding:12px 4px">{tr('No keys configured.')}</p>
      {:else}
        {#each keys as key (key.id)}
          <div class="key-row">
            <div class="course-copy">
              <strong>{key.name || tr('Unnamed key')} <span class="prov-type">{key.credential_type ?? 'api_key'}</span></strong>
              <span>{key.enabled ? (key.invalid ? tr('marked invalid') : tr('healthy')) : tr('disabled')} · {fmtLastUsed(key)}</span>
            </div>
            <div class="key-actions">
              <button class="icon-action danger" type="button" title={tr('Remove key')} onclick={() => removeKey(key)}><svg class="icon"><use href="#i-trash" /></svg></button>
            </div>
          </div>
        {/each}
      {/if}
    </div>
    <form class="keys-add" onsubmit={(e) => { e.preventDefault(); void addKey(); }}>
      <input bind:value={newKey} type="password" placeholder={tr('Paste a new API key…')} autocomplete="off" />
      <button class="page-action primary" type="submit" disabled={keyBusy || !newKey.trim()}>{keyBusy ? tr('Testing…') : tr('Add key')}</button>
    </form>
    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>{tr('Close')}</button>
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
      <button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</button>
    </div>
    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>{tr('Cancel')}</button>
      <button class="page-action danger" type="button" onclick={confirmDelete} disabled={deleteBusy}>{deleteBusy ? tr('Deleting…') : tr('Delete provider')}</button>
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
      <button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</button>
    </div>
    <div class="test-results">
      {#each testRows as row (row.id)}
        <div class="test-row" data-state={row.state}>
          <span class="test-name">{row.name}</span>
          <span class="test-state">{row.detail}</span>
        </div>
      {/each}
    </div>
    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>{tr('Close')}</button>
      <button class="page-action primary" type="button" onclick={runAllTests} disabled={testRunning}>{testRunning ? tr('Running…') : tr('Run again')}</button>
    </div>
  </div>
</dialog>
