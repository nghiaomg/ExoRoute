<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import { localizedError } from '../../lib/errors';
  import { formatCostMicroUsd, type Translate } from '../../lib/format';
  import type { DashboardPage } from '../../lib/navigation';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import type { GatewayApiKey, RequestStatistics } from '../../lib/types';
  import type { Locale } from '../../lib/i18n';
  import ApiKeyDetailSheet from './ApiKeyDetailSheet.svelte';
  import '../../styles/dashboard/pages/api-keys.css';

  interface Props {
    tr: Translate;
    locale: Locale;
    onNavigate: (page: DashboardPage) => void;
  }
  let { tr, locale, onNavigate }: Props = $props();

  let keys = $state<GatewayApiKey[]>([]);
  let nextCursor = $state<string | null>(null);
  let stats = $state<RequestStatistics | null>(null);
  let providerOptions = $state<{ id: string; name: string }[]>([]);
  let loading = $state(true);
  let error = $state('');
  let searchQuery = $state('');
  let statusFilter = $state('all');
  let boardEl = $state<HTMLElement | null>(null);

  let keyDialog = $state<HTMLDialogElement | null>(null);
  let editDialog = $state<HTMLDialogElement | null>(null);
  let confirmDialog = $state<HTMLDialogElement | null>(null);
  let newName = $state('');
  let newModels = $state('');
  let newProviders = $state<string[]>([]);
  let createdKey = $state<{ name: string; key: string } | null>(null);
  let editTarget = $state<GatewayApiKey | null>(null);
  let editName = $state('');
  let editModels = $state('');
  let editProviders = $state<string[]>([]);
  let editEnabled = $state(true);
  let confirmTarget = $state<GatewayApiKey | null>(null);
  let confirmMode = $state<'disable' | 'delete'>('disable');
  let busy = $state(false);
  let detailKey = $state<GatewayApiKey | null>(null);

  const scopeLabel = (key: GatewayApiKey) => {
    if (!key.scope_restricted) return tr('all models');
    const parts: string[] = [];
    if (key.allowed_models?.length) parts.push(key.allowed_models.slice(0, 2).join(', ') + (key.allowed_models.length > 2 ? '…' : ''));
    const providerCount = key.allowed_provider_ids?.length ?? 0;
    if (providerCount) parts.push(providerCount > 1 ? tr('{count} providers', { count: providerCount }) : tr('{count} provider', { count: providerCount }));
    return parts.join(' · ') || tr('restricted');
  };
  const visibleKeys = $derived(keys.filter((key) => {
    const q = searchQuery.trim().toLowerCase();
    if (q && !key.name.toLowerCase().includes(q) && !(key.last_used_at ?? '').toLowerCase().includes(q)) return false;
    if (statusFilter === 'active') return key.enabled;
    if (statusFilter === 'disabled') return !key.enabled;
    return true;
  }));
  const activeCount = $derived(keys.filter((k) => k.enabled).length);
  const disabledCount = $derived(keys.length - activeCount);

  async function load(signal?: AbortSignal) {
    loading = true;
    error = '';
    try {
      const [page, s, provs] = await Promise.all([
        api.apiKeys({}, signal),
        api.statistics('1d', signal).catch(() => null),
        api.providers().catch(() => null),
      ]);
      keys = page.api_keys;
      nextCursor = page.next_cursor ?? null;
      stats = s;
      providerOptions = (provs ?? []).map((p) => ({ id: p.id, name: p.name }));
      // Keep an open detail sheet on the refreshed record so a scope save or a
      // toggle is reflected there without closing and reopening it.
      const openDetailId = detailKey?.id;
      if (openDetailId) {
        detailKey = page.api_keys.find((k) => k.id === openDetailId) ?? detailKey;
      }
    } catch (cause) {
      error = localizedError(cause, 'Could not load API keys.', tr);
    } finally {
      loading = false;
    }
  }
  onMount(() => {
    const controller = new AbortController();
    void load(controller.signal);
    return () => controller.abort();
  });
  onDestroy(() => {});

  function openCreate() {
    newName = '';
    newModels = '';
    newProviders = [];
    createdKey = null;
    keyDialog?.showModal();
  }
  async function submitCreate(event: SubmitEvent) {
    event.preventDefault();
    if (!newName.trim() || busy) return;
    busy = true;
    try {
      const scope = {
        allowed_models: newModels.split(',').map((s) => s.trim()).filter(Boolean),
        allowed_provider_ids: newProviders,
      };
      const created = await api.createApiKey(newName.trim(), scope);
      createdKey = { name: created.name, key: created.key };
      showDashboardToast(tr('API key created — copy it now, it is shown once'));
      await load();
    } catch (cause) {
      showDashboardToast(localizedError(cause, 'Could not create API key.', tr));
    } finally {
      busy = false;
    }
  }
  async function copyText(text: string, label: string) {
    try {
      await navigator.clipboard.writeText(text);
      showDashboardToast(tr('{label} copied', { label }));
    } catch {
      showDashboardToast(tr('Copy failed'));
    }
  }
  function openEdit(key: GatewayApiKey) {
    editTarget = key;
    editName = key.name;
    editModels = (key.allowed_models ?? []).join(', ');
    editProviders = [...(key.allowed_provider_ids ?? [])];
    editEnabled = key.enabled;
    editDialog?.showModal();
  }
  async function submitEdit(event: SubmitEvent) {
    event.preventDefault();
    if (!editTarget || busy) return;
    busy = true;
    try {
      await api.updateApiKey(editTarget.id, {
        name: editName.trim() || undefined,
        enabled: editEnabled,
        allowed_models: editModels.split(',').map((s) => s.trim()).filter(Boolean),
        allowed_provider_ids: editProviders,
      });
      editDialog?.close();
      showDashboardToast(tr('Key updated'));
      await load();
    } catch (cause) {
      showDashboardToast(localizedError(cause, 'Could not update API key.', tr));
    } finally {
      busy = false;
    }
  }
  function askConfirm(key: GatewayApiKey, mode: 'disable' | 'delete') {
    confirmTarget = key;
    confirmMode = mode;
    confirmDialog?.showModal();
  }
  /** Deletion removes the row, so an open detail sheet is closed first. */
  function openDetail(key: GatewayApiKey) {
    detailKey = key;
  }
  async function runConfirm() {
    if (!confirmTarget || busy) return;
    busy = true;
    try {
      if (confirmMode === 'delete') {
        await api.deleteApiKey(confirmTarget.id);
        if (detailKey?.id === confirmTarget.id) detailKey = null;
        showDashboardToast(tr('Deleted "{name}"', { name: confirmTarget.name }));
      } else {
        await api.updateApiKey(confirmTarget.id, { enabled: false });
        showDashboardToast(tr('Disabled "{name}"', { name: confirmTarget.name }));
      }
      confirmDialog?.close();
      await load();
    } catch (cause) {
      showDashboardToast(localizedError(cause, 'Action failed.', tr));
    } finally {
      busy = false;
    }
  }
  function exportKeys() {
    const rows = [['name', 'id', 'enabled', 'created_at', 'last_used_at', 'scope']];
    for (const k of keys) rows.push([k.name, k.id, String(k.enabled), k.created_at, k.last_used_at ?? '', scopeLabel(k)]);
    const blob = new Blob([rows.map((r) => r.map((c) => `"${String(c).replaceAll('"', '""')}"`).join(',')).join('\n')], { type: 'text/csv' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = 'exoroute-api-keys.csv';
    a.click();
    URL.revokeObjectURL(a.href);
    showDashboardToast(tr('Exported API keys CSV'));
  }
</script>

<div class="page-heading">
  <div class="heading-copy">
    <h1>{tr('API keys')}</h1>
    <p>{tr('Client credentials for the gateway. Secrets are shown once at creation.')}</p>
  </div>
  <div class="heading-actions">
    <button class="action-button" type="button" onclick={exportKeys}>
      <span class="button-icon"><svg class="icon"><use href="#i-download" /></svg></span>
      <span class="button-label">{tr('Export')}</span>
    </button>
    <button class="action-button primary" type="button" onclick={openCreate}>
      <span class="button-icon"><svg class="icon"><use href="#i-plus" /></svg></span>
      <span class="button-label">{tr('New API key')}</span>
    </button>
  </div>
</div>

<div class="page-surface">
  {#if error}
    <div class="empty-state">{error} <button class="mini-add" type="button" onclick={() => load()}>{tr('Retry')}</button></div>
  {:else}
    <section class="stats-grid" aria-label={tr('API key overview')}>
      <article class="stat-card surface">
        <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-key" /></svg></span><span class="stat-number">{activeCount}</span></div>
        <h2>{tr('Active Keys')}</h2>
        <div class="stat-trend">{tr('Showing {count} API keys on this page', { count: keys.length })}</div>
        <button class="details-link" type="button" onclick={() => { statusFilter = 'active'; boardEl?.scrollIntoView({ behavior: 'smooth' }); }}>{tr('Show active')} <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-clipboard" /></svg></span><span class="stat-number">{stats ? stats.total_requests.toLocaleString() : '—'}</span></div>
        <h2>{tr('Requests Today')}</h2>
        <div class="stat-trend">{tr('Across all keys · 24h')}</div>
        <button class="details-link" type="button" onclick={() => onNavigate('requests')}>{tr('View requests')} <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-dollar" /></svg></span><span class="stat-number">{stats ? formatCostMicroUsd(stats.cost.cost_reported_micro_usd) : '—'}</span></div>
        <h2>{tr('Spend Today')}</h2>
        <div class="stat-trend">{tr('Estimated upstream cost')}</div>
        <button class="details-link" type="button" onclick={() => onNavigate('quota')}>{tr('Check quota')} <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-alert" /></svg></span><span class="stat-number">{stats ? stats.failures.toLocaleString() : '—'}</span></div>
        <h2>{tr('Failures Today')}</h2>
        <div class="stat-trend">{stats ? `${stats.success_rate.toFixed(1)}% ${tr('success')}` : tr('24h window')}</div>
        <button class="details-link" type="button" onclick={() => onNavigate('requests')}>{tr('View requests')} <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
    </section>

    <section class="course-card surface keys-board" id="keys-board" bind:this={boardEl} aria-labelledby="keys-heading">
      <div class="card-title-row"><h2 id="keys-heading">{tr('Keys')}</h2></div>
      <p class="card-subtitle">{tr('Showing {count} API keys on this page', { count: keys.length })}{nextCursor ? ` · ${tr('more available')}` : ''}</p>
      <div class="page-toolbar keys-toolbar">
        <input class="prov-search" type="search" placeholder={tr('Search keys…')} aria-label={tr('Search API keys')} bind:value={searchQuery} />
        <select class="prov-select" aria-label={tr('Filter keys')} bind:value={statusFilter}>
          <option value="all">{tr('All keys')}</option>
          <option value="active">{tr('Active')}</option>
          <option value="disabled">{tr('Disabled')}</option>
        </select>
      </div>
      <div class="course-list">
        {#each visibleKeys as key (key.id)}
          <article class="course-row key-card" class:key-revoked={!key.enabled}>
            <!-- The row body opens the detail sheet; the actions beside it stay
                 independent buttons rather than being swallowed by the trigger. -->
            <button
              class="key-card-trigger"
              type="button"
              aria-label={tr('Open {name} details', { name: key.name })}
              onclick={() => openDetail(key)}
            >
              <span class="course-icon"><svg class="icon"><use href="#i-key" /></svg></span>
              <span class="course-copy">
                <strong>{key.name} <span class="prov-type">{scopeLabel(key)}</span></strong>
                <span>{key.id.slice(0, 8)}… · {key.last_used_at ? tr('Used {date}', { date: key.last_used_at }) : tr('Never used')} · {key.request_count ?? 0} {tr('requests')}</span>
              </span>
            </button>
            <span class="status {key.enabled ? 'complete' : 'pending'}">{key.enabled ? tr('Active') : tr('Disabled')}</span>
            <div class="course-actions">
              <button class="icon-action" type="button" title={tr('Edit key')} onclick={() => openEdit(key)}><svg class="icon"><use href="#i-edit" /></svg></button>
              {#if key.enabled}
                <button class="icon-action danger" type="button" title={tr('Disable key')} onclick={() => askConfirm(key, 'disable')}><svg class="icon"><use href="#i-x-circle" /></svg></button>
              {:else}
                <button class="icon-action danger" type="button" title={tr('Delete permanently')} onclick={() => askConfirm(key, 'delete')}><svg class="icon"><use href="#i-trash" /></svg></button>
              {/if}
            </div>
          </article>
        {/each}
        {#if loading}
          <div class="empty-state">{tr('Loading…')}</div>
        {:else if visibleKeys.length === 0}
          <div class="empty-state">{tr('No keys match the current filters.')}</div>
        {/if}
      </div>
    </section>
  {/if}
</div>

<!-- New API key -->
<dialog class="modal" bind:this={keyDialog} aria-labelledby="key-dialog-title">
  <form class="modal-card" onsubmit={submitCreate}>
    <div class="modal-heading">
      <div>
        <h2 id="key-dialog-title">{createdKey ? tr('Key created') : tr('New API key')}</h2>
        <p>{createdKey ? tr('Copy it now — it will not be shown again.') : tr('Create a client credential. The secret is shown once.')}</p>
      </div>
      <button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</button>
    </div>
    {#if createdKey}
      <div class="key-reveal">
        <strong>{createdKey.name}</strong>
        <div class="key-reveal-row">
          <code>{createdKey.key}</code>
          <button class="page-action" type="button" onclick={() => copyText(createdKey!.key, tr('API key'))}><svg class="icon"><use href="#i-copy" /></svg> {tr('Copy')}</button>
        </div>
      </div>
      <div class="modal-actions">
        <button class="page-action primary" type="button" onclick={() => keyDialog?.close()}>{tr('Done')}</button>
      </div>
    {:else}
      <div class="modal-fields">
        <label class="field provd-wide">{tr('Key name')}<input type="text" required placeholder={tr('e.g. staging-app')} bind:value={newName} /></label>
        <label class="field provd-wide">{tr('Allowed models (comma separated, empty = all)')}<input type="text" placeholder={tr('gpt-4o, claude-*')} bind:value={newModels} /></label>
        <div class="field provd-wide">
          {tr('Allowed providers')}
          <div class="check-row-inline" style="flex-wrap:wrap;gap:8px">
            {#each providerOptions as p (p.id)}
              <label style="display:flex;gap:6px;align-items:center;font-weight:400">
                <input type="checkbox" checked={newProviders.includes(p.id)} onchange={(e) => {
                  const on = (e.currentTarget as HTMLInputElement).checked;
                  newProviders = on ? [...newProviders, p.id] : newProviders.filter((id) => id !== p.id);
                }} />
                {p.name}
              </label>
            {/each}
            {#if providerOptions.length === 0}<span class="card-subtitle">{tr('No providers configured')}</span>{/if}
          </div>
        </div>
      </div>
      <div class="modal-actions">
        <button class="page-action" type="button" data-close-modal>{tr('Cancel')}</button>
        <button class="page-action primary" type="submit" disabled={busy || !newName.trim()}>{tr('Create key')}</button>
      </div>
    {/if}
  </form>
</dialog>

<!-- Edit key -->
<dialog class="modal modal-wide api-key-edit-dialog" bind:this={editDialog} aria-labelledby="edit-dialog-title">
  <form class="modal-card api-key-edit-card" onsubmit={submitEdit}>
    <div class="modal-heading">
      <div>
        <h2 id="edit-dialog-title">{tr('Edit API key')}</h2>
        <p>{editTarget ? tr('Adjust name, scope and state for "{name}".', { name: editTarget.name }) : ''}</p>
      </div>
      <button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</button>
    </div>
    <div class="modal-fields api-key-edit-fields">
      <label class="field api-key-edit-section api-key-name-field">
        {tr('Key name')}
        <input type="text" required bind:value={editName} />
      </label>
      <label class="field api-key-edit-section api-key-model-field">
        {tr('Allowed models (comma separated, empty = all)')}
        <input type="text" placeholder={tr('gpt-4o, claude-*')} bind:value={editModels} />
      </label>
      <fieldset class="api-key-edit-section api-key-provider-field">
        <legend>{tr('Allowed providers')}</legend>
        <p class="api-key-edit-help">{tr('Leave empty to allow every provider.')}</p>
        {#if providerOptions.length}
          <div class="api-key-provider-list">
            {#each providerOptions as p (p.id)}
              <label class="api-key-provider-option">
                <input type="checkbox" checked={editProviders.includes(p.id)} onchange={(e) => {
                  const on = (e.currentTarget as HTMLInputElement).checked;
                  editProviders = on ? [...editProviders, p.id] : editProviders.filter((id) => id !== p.id);
                }} />
                <span>{p.name}</span>
              </label>
            {/each}
          </div>
        {:else}
          <p class="api-key-edit-empty">{tr('No providers configured')}</p>
        {/if}
      </fieldset>
      <label class="api-key-edit-section api-key-enabled-field">
        <input type="checkbox" bind:checked={editEnabled} />
        <span>
          <strong>{tr('Key enabled')}</strong>
        </span>
      </label>
    </div>
    <div class="modal-actions api-key-edit-actions">
      <button class="page-action" type="button" data-close-modal>{tr('Cancel')}</button>
      <button class="page-action primary" type="submit" disabled={busy}>{tr('Save')}</button>
    </div>
  </form>
</dialog>

<!-- Disable / delete confirm -->
<dialog class="modal" bind:this={confirmDialog} aria-labelledby="confirm-title">
  <div class="modal-card">
    <div class="modal-heading">
      <div>
        <h2 id="confirm-title">{confirmMode === 'delete' ? tr('Delete API key?') : tr('Disable API key?')}</h2>
        <p>{confirmTarget ? (confirmMode === 'delete'
          ? tr('Permanently delete "{name}"? This cannot be undone.', { name: confirmTarget.name })
          : tr('Requests using "{name}" will start failing immediately.', { name: confirmTarget.name })) : ''}</p>
      </div>
      <button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</button>
    </div>
    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>{tr('Cancel')}</button>
      <button class="page-action danger" type="button" onclick={runConfirm} disabled={busy}>{confirmMode === 'delete' ? tr('Delete permanently') : tr('Disable key')}</button>
    </div>
  </div>
</dialog>

<!-- Key detail: facts, access scope and per-key usage in one sheet -->
<ApiKeyDetailSheet
  apiKey={detailKey}
  {tr}
  {locale}
  onUpdated={() => void load()}
  onClose={() => (detailKey = null)}
/>

<style>
  /* The row's copy cluster is the detail trigger. It is a real button so the
     whole clickable area is keyboard reachable and announced; the status pill
     and action buttons beside it stay separate controls. */
  .key-card-trigger {
    display: flex;
    flex: 1;
    min-width: 0;
    align-items: center;
    gap: 9px;
    padding: 0;
    color: inherit;
    border: 0;
    background: transparent;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .key-card-trigger:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    border-radius: 8px;
  }
</style>
