<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import type { DashboardPage } from '../../lib/navigation';
  import type { Locale } from '../../lib/i18n';
  import type { Translate } from '../../lib/format';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import type { Provider, ProviderKey, ProviderPreset, UpstreamProtocol } from '../../lib/types';
  import { providerLogo } from './provider-logo';
  import '../../styles/dashboard/pages/providers.css';

  interface Props {
    tr: Translate;
    locale: Locale;
    onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;
    onProviderCountChange: (count: number) => void;
  }

  let { tr: _tr, locale: _locale, onConnectionChange, onProviderCountChange }: Props = $props();

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

  function providerType(provider: Provider): string {
    return presets.find((p) => p.adapter_id === provider.adapter_id || p.id === provider.adapter_id)?.name ?? provider.adapter_id;
  }

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
      errorMessage = error instanceof Error ? error.message : 'Could not load providers.';
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
        pfNote = failed ? `${failed} API key test(s) failed; keys were kept.` : '';
        showDashboardToast(`Provider "${name}" updated`);
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
        pfNote = failed ? `${failed} API key test(s) failed; keys were still saved.` : '';
        showDashboardToast(`Provider "${name}" created`);
      }
      providerDialog?.close();
      await load();
    } catch (error) {
      pfError = error instanceof Error ? error.message : 'Could not save provider.';
    } finally {
      pfBusy = false;
    }
  }

  async function toggleProvider(provider: Provider): Promise<void> {
    const next = !provider.enabled;
    providers = providers.map((p) => (p.id === provider.id ? { ...p, enabled: next } : p));
    try {
      await api.updateProvider(provider.id, { enabled: next });
      showDashboardToast(`${provider.name} ${next ? 'enabled' : 'disabled'}`);
    } catch (error) {
      providers = providers.map((p) => (p.id === provider.id ? { ...p, enabled: !next } : p));
      showDashboardToast(error instanceof Error ? error.message : 'Update failed');
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
      showDashboardToast(error instanceof Error ? error.message : 'Could not load keys');
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
      showDashboardToast(res.test_passed ? 'Key added — test passed' : `Key added — test ${res.status ?? 'failed'}`);
      const page = await api.providerKeys(keysProvider.id);
      keys = page.keys;
      await load();
    } catch (error) {
      showDashboardToast(error instanceof Error ? error.message : 'Could not add key');
    } finally {
      keyBusy = false;
    }
  }

  async function removeKey(key: ProviderKey): Promise<void> {
    if (!keysProvider) return;
    try {
      await api.deleteProviderKey(keysProvider.id, key.id);
      keys = keys.filter((k) => k.id !== key.id);
      showDashboardToast('Key removed');
      await load();
    } catch (error) {
      showDashboardToast(error instanceof Error ? error.message : 'Could not remove key');
    }
  }

  function openDelete(provider: Provider): void {
    deleteTarget = provider;
    deleteDialog?.showModal();
  }

  async function confirmDelete(): Promise<void> {
    if (!deleteTarget || deleteBusy) return;
    deleteBusy = true;
    try {
      await api.deleteProvider(deleteTarget.id);
      providers = providers.filter((p) => p.id !== deleteTarget?.id);
      onProviderCountChange(providers.length);
      showDashboardToast(`Provider "${deleteTarget.name}" deleted`);
      deleteDialog?.close();
    } catch (error) {
      showDashboardToast(error instanceof Error ? error.message : 'Could not delete provider');
    } finally {
      deleteBusy = false;
    }
  }

  async function testProvider(provider: Provider, btn?: HTMLElement): Promise<void> {
    if (btn) btn.textContent = '…';
    const started = performance.now();
    try {
      await api.providerModels(provider.id, { limit: 1 });
      const ms = Math.round(performance.now() - started);
      showDashboardToast(`${provider.name}: OK · ${ms} ms`);
    } catch (error) {
      showDashboardToast(`${provider.name}: unreachable — ${error instanceof Error ? error.message : 'error'}`);
    } finally {
      if (btn) btn.innerHTML = '<svg class="icon"><use href="#i-wifi" /></svg>';
    }
  }

  async function syncModels(): Promise<void> {
    const targets = providers.filter((p) => p.enabled);
    if (!targets.length) { showDashboardToast('No enabled providers to sync'); return; }
    let imported = 0;
    for (const p of targets) {
      try {
        const res = await api.importProviderModels(p.id);
        imported += res.models?.length ?? 0;
      } catch { /* per-provider failure is non-fatal */ }
    }
    showDashboardToast(`Model sync complete — ${imported || 'catalog'} refreshed`);
    await load();
  }

  async function runAllTests(): Promise<void> {
    const targets = providers.filter((p) => p.enabled);
    testRows = targets.map((p) => ({ id: p.id, name: p.name, state: 'pending', detail: '' }));
    testRunning = true;
    testDialog?.showModal();
    for (let i = 0; i < testRows.length; i++) {
      const row = testRows[i];
      testRows[i] = { ...row, state: 'testing', detail: 'testing…' };
      testRows = [...testRows];
      const started = performance.now();
      try {
        await api.providerModels(row.id, { limit: 1 });
        const ms = Math.round(performance.now() - started);
        testRows[i] = { ...testRows[i], state: 'ok', detail: `OK · ${100 + ms} ms` };
      } catch {
        testRows[i] = { ...testRows[i], state: 'fail', detail: 'unreachable' };
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
    return `${n} ${n === 1 ? 'key' : 'keys'}`;
  }

  function fmtLastUsed(key: ProviderKey): string {
    if (!key.last_used_at) return 'never used';
    const diff = Date.now() - new Date(key.last_used_at).getTime();
    if (diff < 60_000) return 'just now';
    if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
    if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
    return `${Math.floor(diff / 86_400_000)}d ago`;
  }

  onMount(() => { void load(); });
  onDestroy(() => { generation += 1; });
</script>

<div class="page-heading">
  <div class="heading-copy">
    <h1>Providers</h1>
    <p>Manage upstream AI providers, credentials, and synced models.</p>
  </div>
  <div class="heading-actions">
    <button class="action-button" type="button" onclick={runAllTests} disabled={testRunning}>
      <span class="button-icon"><svg class="icon"><use href="#i-zap" /></svg></span>
      <span class="button-label">Test all</span>
    </button>
    <button class="action-button primary" type="button" onclick={() => openProviderDialog(null)}>
      <span class="button-icon"><svg class="icon"><use href="#i-plus" /></svg></span>
      <span class="button-label">Add provider</span>
    </button>
  </div>
</div>

{#if errorMessage}
  <section class="course-card surface" aria-label="Load error">
    <div class="card-title-row"><h2>Could not load providers</h2></div>
    <p class="card-subtitle">{errorMessage}</p>
    <div class="modal-actions" style="padding-top:14px">
      <button class="page-action primary" type="button" onclick={load}>Retry</button>
    </div>
  </section>
{:else if loading}
  <section class="stats-grid" aria-label="Loading">
    {#each [0, 1, 2, 3] as i (i)}<article class="stat-card surface" style="min-height:132px"></article>{/each}
  </section>
{:else}
  <div class="page-surface">
    <section class="stats-grid" aria-label="Provider overview">
      <article class="stat-card surface">
        <div class="stat-top">
          <span class="stat-icon"><svg class="icon"><use href="#i-link" /></svg></span>
          <span class="stat-number">{providers.length}</span>
        </div>
        <h2>Total Providers</h2>
        <div class="stat-trend">{enabledCount} enabled, {providers.length - enabledCount} disabled</div>
        <button class="details-link" type="button" onclick={() => (statusFilter = 'all')}>View all <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top">
          <span class="stat-icon"><svg class="icon"><use href="#i-zap" /></svg></span>
          <span class="stat-number">{enabledCount}</span>
        </div>
        <h2>Enabled</h2>
        <div class="stat-trend">Serving live traffic</div>
        <button class="details-link" type="button" onclick={() => (statusFilter = 'enabled')}>Show enabled only <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top">
          <span class="stat-icon"><svg class="icon"><use href="#i-key" /></svg></span>
          <span class="stat-number">{totalKeys}</span>
        </div>
        <h2>Credentials</h2>
        <div class="stat-trend">Across {providers.filter((p) => (p.api_key_count ?? 0) > 0).length} providers</div>
        <button class="details-link" type="button" onclick={() => { if (providers[0]) void openKeys(providers[0]); }}>Manage keys <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top">
          <span class="stat-icon"><svg class="icon"><use href="#i-refresh" /></svg></span>
          <span class="stat-number">{totalModels}</span>
        </div>
        <h2>Synced Models</h2>
        <div class="stat-trend">{keylessCount ? `${keylessCount} enabled without keys` : 'All enabled providers have keys'}</div>
        <button class="details-link" type="button" onclick={syncModels}>Sync now <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
    </section>

    <section class="course-card surface provider-section" aria-labelledby="providers-heading">
      <div class="card-title-row">
        <h2 id="providers-heading">Providers</h2>
        <button class="mini-add" type="button" onclick={syncModels}>
          <svg class="icon"><use href="#i-refresh" /></svg>
          Sync models
        </button>
      </div>
      <p class="card-subtitle">{providers.length} providers · {enabledCount} enabled · {totalKeys} credentials</p>
      <div class="page-toolbar provider-toolbar">
        <input class="prov-search" type="search" placeholder="Search providers..." aria-label="Search providers" bind:value={search} />
        <select class="prov-select" aria-label="Filter by status" bind:value={statusFilter}>
          <option value="all">All statuses</option>
          <option value="enabled">Enabled</option>
          <option value="disabled">Disabled</option>
        </select>
      </div>
      <div class="course-list provider-list" aria-live="polite">
        {#each filteredProviders as provider (provider.id)}
          {@const logo = providerLogo(provider.logo_url, provider.adapter_id, provider.id, provider.name)}
          <article class="course-row provider-row" data-status={provider.enabled ? 'enabled' : 'disabled'}>
            <span class="course-icon provider-logo">
              {#if logo}
                <img src={logo} alt="" aria-hidden="true" />
              {:else}
                <svg class="icon"><use href="#i-zap" /></svg>
              {/if}
            </span>
            <div class="course-copy">
              <strong>{provider.name} <span class="prov-type">{providerType(provider)}</span></strong>
              <span>{provider.base_url} · {fmtKeys(provider)} · {provider.model_count ?? 0} models</span>
            </div>
            {#if provider.enabled && !(provider.api_key_count ?? 0)}
              <span class="status progress prov-toggle" role="switch" aria-checked="true" tabindex="0" onclick={() => toggleProvider(provider)} onkeydown={(e) => e.key === 'Enter' && toggleProvider(provider)}>No keys</span>
            {:else}
              <span class="status {provider.enabled ? 'complete' : 'pending'} prov-toggle" role="switch" aria-checked={provider.enabled} tabindex="0" onclick={() => toggleProvider(provider)} onkeydown={(e) => e.key === 'Enter' && toggleProvider(provider)}>{provider.enabled ? 'Enabled' : 'Disabled'}</span>
            {/if}
            <div class="course-actions">
              <button class="icon-action" type="button" title="Edit provider" onclick={() => openProviderDialog(provider)}><svg class="icon"><use href="#i-edit" /></svg></button>
              <button class="icon-action" type="button" title="Manage API keys" onclick={() => openKeys(provider)}><svg class="icon"><use href="#i-key" /></svg></button>
              <button class="icon-action" type="button" title="Test connection" onclick={(e) => testProvider(provider, e.currentTarget)}><svg class="icon"><use href="#i-wifi" /></svg></button>
              <button class="icon-action danger" type="button" title="Delete provider" onclick={() => openDelete(provider)}><svg class="icon"><use href="#i-trash" /></svg></button>
            </div>
          </article>
        {:else}
          <div class="empty-state">No providers match the current filters.</div>
        {/each}
      </div>
    </section>
  </div>
{/if}

<!-- Add / edit provider -->
<dialog class="modal" bind:this={providerDialog} aria-labelledby="provider-dialog-title" onclose={() => { pfError = ''; pfNote = ''; }}>
  <form class="modal-card" onsubmit={(e) => { e.preventDefault(); void saveProvider(); }}>
    <div class="modal-heading">
      <div>
        <h2 id="provider-dialog-title">{editingProvider ? 'Edit provider' : 'Add provider'}</h2>
        <p>{editingProvider ? `Update ${editingProvider.name}` : 'Connect an upstream AI provider.'}</p>
      </div>
      <button class="modal-close" type="button" data-close-modal aria-label="Close dialog">×</button>
    </div>
    <div class="modal-fields">
      <label class="field prov-field-wide">
        Provider name
        <input bind:value={pfName} maxlength="60" required placeholder="e.g. OpenAI" />
      </label>
      <label class="field">
        Type
        <select bind:value={pfPreset} onchange={applyPreset} disabled={!!editingProvider}>
          {#each presets as preset (preset.id)}
            <option value={preset.id}>{preset.name}</option>
          {/each}
        </select>
      </label>
      <label class="field">
        Base URL
        <input bind:value={pfUrl} type="url" required placeholder="https://api.example.com/v1" />
      </label>
      <label class="field">
        API key{editingProvider ? ' (leave blank to keep existing)' : ''}
        <input bind:value={pfKey} type="password" placeholder="sk-..." autocomplete="off" />
      </label>
      <label class="field prov-field-wide">
        Model prefix (optional)
        <input bind:value={pfModel} maxlength="120" placeholder="e.g. gpt-4o-mini" />
      </label>
    </div>
    {#if pfError}<p class="form-error" role="alert">{pfError}</p>{/if}
    {#if pfNote}<p class="form-note">{pfNote}</p>{/if}
    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>Cancel</button>
      <button class="page-action primary" type="submit" disabled={pfBusy}>{pfBusy ? 'Saving…' : 'Save provider'}</button>
    </div>
  </form>
</dialog>

<!-- Manage API keys -->
<dialog class="modal modal-wide" bind:this={keysDialog} aria-labelledby="keys-dialog-title">
  <div class="modal-card">
    <div class="modal-heading">
      <div>
        <h2 id="keys-dialog-title">API keys</h2>
        <p>Credentials for <strong>{keysProvider?.name ?? 'provider'}</strong>.</p>
      </div>
      <button class="modal-close" type="button" data-close-modal aria-label="Close dialog">×</button>
    </div>
    <div class="keys-list">
      {#if keysLoading}
        <p class="card-subtitle" style="padding:12px 4px">Loading keys…</p>
      {:else if keys.length === 0}
        <p class="card-subtitle" style="padding:12px 4px">No keys configured.</p>
      {:else}
        {#each keys as key (key.id)}
          <div class="key-row">
            <div class="course-copy">
              <strong>{key.name || 'Unnamed key'} <span class="prov-type">{key.credential_type ?? 'api_key'}</span></strong>
              <span>{key.enabled ? (key.invalid ? 'marked invalid' : 'healthy') : 'disabled'} · {fmtLastUsed(key)}</span>
            </div>
            <div class="key-actions">
              <button class="icon-action danger" type="button" title="Remove key" onclick={() => removeKey(key)}><svg class="icon"><use href="#i-trash" /></svg></button>
            </div>
          </div>
        {/each}
      {/if}
    </div>
    <form class="keys-add" onsubmit={(e) => { e.preventDefault(); void addKey(); }}>
      <input bind:value={newKey} type="password" placeholder="Paste a new API key…" autocomplete="off" />
      <button class="page-action primary" type="submit" disabled={keyBusy || !newKey.trim()}>{keyBusy ? 'Testing…' : 'Add key'}</button>
    </form>
    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>Close</button>
    </div>
  </div>
</dialog>

<!-- Delete confirm -->
<dialog class="modal" bind:this={deleteDialog} aria-labelledby="delete-dialog-title">
  <div class="modal-card">
    <div class="modal-heading">
      <div>
        <h2 id="delete-dialog-title">Delete provider?</h2>
        <p>This removes {deleteTarget?.name ?? 'the provider'} and its credentials. Combos referencing it will lose that target.</p>
      </div>
      <button class="modal-close" type="button" data-close-modal aria-label="Close dialog">×</button>
    </div>
    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>Cancel</button>
      <button class="page-action danger" type="button" onclick={confirmDelete} disabled={deleteBusy}>{deleteBusy ? 'Deleting…' : 'Delete provider'}</button>
    </div>
  </div>
</dialog>

<!-- Test all results -->
<dialog class="modal" bind:this={testDialog} aria-labelledby="test-all-title">
  <div class="modal-card">
    <div class="modal-heading">
      <div>
        <h2 id="test-all-title">Connection test</h2>
        <p>Probing each enabled provider…</p>
      </div>
      <button class="modal-close" type="button" data-close-modal aria-label="Close dialog">×</button>
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
      <button class="page-action" type="button" data-close-modal>Close</button>
      <button class="page-action primary" type="button" onclick={runAllTests} disabled={testRunning}>{testRunning ? 'Running…' : 'Run again'}</button>
    </div>
  </div>
</dialog>
