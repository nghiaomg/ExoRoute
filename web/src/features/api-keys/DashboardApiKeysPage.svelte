<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import { localizedError } from '../../lib/errors';
  import { formatCostMicroUsd } from '../../lib/format';
  import type { DashboardPage } from '../../lib/navigation';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import type { GatewayApiKey, RequestStatistics } from '../../lib/types';
  import '../../styles/dashboard/pages/apikeys.css';

  interface Props {
    tr: (key: string) => string;
    onNavigate: (page: DashboardPage) => void;
  }
  let { tr, onNavigate }: Props = $props();

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

  const expiringSoon = (key: GatewayApiKey) => {
    // GatewayApiKey has no expiry field; treat nothing as expiring.
    return false;
  };
  const scopeLabel = (key: GatewayApiKey) => {
    if (!key.scope_restricted) return 'all models';
    const parts: string[] = [];
    if (key.allowed_models?.length) parts.push(key.allowed_models.slice(0, 2).join(', ') + (key.allowed_models.length > 2 ? '…' : ''));
    if (key.allowed_provider_ids?.length) parts.push(`${key.allowed_provider_ids.length} provider${key.allowed_provider_ids.length > 1 ? 's' : ''}`);
    return parts.join(' · ') || 'restricted';
  };
  const visibleKeys = $derived(keys.filter((key) => {
    const q = searchQuery.trim().toLowerCase();
    if (q && !key.name.toLowerCase().includes(q) && !(key.last_used_at ?? '').toLowerCase().includes(q)) return false;
    if (statusFilter === 'active') return key.enabled;
    if (statusFilter === 'disabled') return !key.enabled;
    if (statusFilter === 'expiring') return expiringSoon(key);
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
      showDashboardToast('API key created — copy it now, it is shown once');
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
      showDashboardToast(`${label} copied`);
    } catch {
      showDashboardToast('Copy failed');
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
      showDashboardToast('Key updated');
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
  async function runConfirm() {
    if (!confirmTarget || busy) return;
    busy = true;
    try {
      if (confirmMode === 'delete') {
        await api.deleteApiKey(confirmTarget.id);
        showDashboardToast(`Deleted "${confirmTarget.name}"`);
      } else {
        await api.updateApiKey(confirmTarget.id, { enabled: false });
        showDashboardToast(`Disabled "${confirmTarget.name}"`);
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
    showDashboardToast('Exported API keys CSV');
  }
</script>

<div class="page-heading">
  <div class="heading-copy">
    <h1>API keys</h1>
    <p>Client credentials for the gateway. Secrets are shown once at creation.</p>
  </div>
  <div class="heading-actions">
    <button class="action-button" type="button" onclick={exportKeys}>
      <span class="button-icon"><svg class="icon"><use href="#i-download" /></svg></span>
      <span class="button-label">Export</span>
    </button>
    <button class="action-button primary" type="button" onclick={openCreate}>
      <span class="button-icon"><svg class="icon"><use href="#i-plus" /></svg></span>
      <span class="button-label">New API key</span>
    </button>
  </div>
</div>

<div class="page-surface">
  {#if error}
    <div class="empty-state">{error} <button class="mini-add" type="button" onclick={() => load()}>Retry</button></div>
  {:else}
    <section class="stats-grid" aria-label="API key overview">
      <article class="stat-card surface">
        <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-key" /></svg></span><span class="stat-number">{activeCount}</span></div>
        <h2>Active Keys</h2>
        <div class="stat-trend">{disabledCount} disabled</div>
        <button class="details-link" type="button" onclick={() => { statusFilter = 'active'; boardEl?.scrollIntoView({ behavior: 'smooth' }); }}>Show active <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-clipboard" /></svg></span><span class="stat-number">{stats ? stats.total_requests.toLocaleString() : '—'}</span></div>
        <h2>Requests Today</h2>
        <div class="stat-trend">Across all keys · 24h</div>
        <button class="details-link" type="button" onclick={() => onNavigate('requests')}>View requests <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-dollar" /></svg></span><span class="stat-number">{stats ? formatCostMicroUsd(stats.cost.cost_reported_micro_usd) : '—'}</span></div>
        <h2>Spend Today</h2>
        <div class="stat-trend">Estimated upstream cost</div>
        <button class="details-link" type="button" onclick={() => onNavigate('quota')}>Check quota <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-alert" /></svg></span><span class="stat-number">{stats ? stats.failures.toLocaleString() : '—'}</span></div>
        <h2>Failures Today</h2>
        <div class="stat-trend">{stats ? `${stats.success_rate.toFixed(1)}% success` : '24h window'}</div>
        <button class="details-link" type="button" onclick={() => onNavigate('requests')}>View requests <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
    </section>

    <section class="course-card surface keys-board" id="keys-board" bind:this={boardEl} aria-labelledby="keys-heading">
      <div class="card-title-row"><h2 id="keys-heading">Keys</h2></div>
      <p class="card-subtitle">{activeCount} active · {disabledCount} disabled{nextCursor ? ' · more available' : ''}</p>
      <div class="page-toolbar keys-toolbar">
        <input class="prov-search" type="search" placeholder="Search keys…" aria-label="Search API keys" bind:value={searchQuery} />
        <select class="prov-select" aria-label="Filter keys" bind:value={statusFilter}>
          <option value="all">All keys</option>
          <option value="active">Active</option>
          <option value="disabled">Disabled</option>
          <option value="expiring">Expiring soon</option>
        </select>
      </div>
      <div class="course-list">
        {#each visibleKeys as key (key.id)}
          <article class="course-row key-card" class:key-revoked={!key.enabled}>
            <span class="course-icon"><svg class="icon"><use href="#i-key" /></svg></span>
            <div class="course-copy">
              <strong>{key.name} <span class="prov-type">{scopeLabel(key)}</span></strong>
              <span>{key.id.slice(0, 8)}… · {key.last_used_at ? `used ${key.last_used_at}` : 'never used'} · {key.request_count ?? 0} requests</span>
            </div>
            <span class="status {key.enabled ? 'complete' : 'pending'}">{key.enabled ? 'Active' : 'Disabled'}</span>
            <div class="course-actions">
              <button class="icon-action" type="button" title="Edit key" onclick={() => openEdit(key)}><svg class="icon"><use href="#i-edit" /></svg></button>
              {#if key.enabled}
                <button class="icon-action danger" type="button" title="Disable key" onclick={() => askConfirm(key, 'disable')}><svg class="icon"><use href="#i-x-circle" /></svg></button>
              {:else}
                <button class="icon-action danger" type="button" title="Delete permanently" onclick={() => askConfirm(key, 'delete')}><svg class="icon"><use href="#i-trash" /></svg></button>
              {/if}
            </div>
          </article>
        {/each}
        {#if loading}
          <div class="empty-state">Loading…</div>
        {:else if visibleKeys.length === 0}
          <div class="empty-state">No keys match the current filters.</div>
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
        <h2 id="key-dialog-title">{createdKey ? 'Key created' : 'New API key'}</h2>
        <p>{createdKey ? 'Copy it now — it will not be shown again.' : 'Create a client credential. The secret is shown once.'}</p>
      </div>
      <button class="modal-close" type="button" data-close-modal aria-label="Close dialog">×</button>
    </div>
    {#if createdKey}
      <div class="key-reveal">
        <strong>{createdKey.name}</strong>
        <div class="key-reveal-row">
          <code>{createdKey.key}</code>
          <button class="page-action" type="button" onclick={() => copyText(createdKey!.key, 'API key')}><svg class="icon"><use href="#i-copy" /></svg> Copy</button>
        </div>
      </div>
      <div class="modal-actions">
        <button class="page-action primary" type="button" onclick={() => keyDialog?.close()}>Done</button>
      </div>
    {:else}
      <div class="modal-fields">
        <label class="field provd-wide">Key name<input type="text" required placeholder="e.g. staging-app" bind:value={newName} /></label>
        <label class="field provd-wide">Allowed models (comma separated, empty = all)<input type="text" placeholder="gpt-4o, claude-*" bind:value={newModels} /></label>
        <div class="field provd-wide">
          Allowed providers
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
            {#if providerOptions.length === 0}<span class="card-subtitle">No providers configured</span>{/if}
          </div>
        </div>
      </div>
      <div class="modal-actions">
        <button class="page-action" type="button" data-close-modal>Cancel</button>
        <button class="page-action primary" type="submit" disabled={busy || !newName.trim()}>Create key</button>
      </div>
    {/if}
  </form>
</dialog>

<!-- Edit key -->
<dialog class="modal" bind:this={editDialog} aria-labelledby="edit-dialog-title">
  <form class="modal-card" onsubmit={submitEdit}>
    <div class="modal-heading">
      <div>
        <h2 id="edit-dialog-title">Edit API key</h2>
        <p>{editTarget ? `Adjust name, scope and state for "${editTarget.name}".` : ''}</p>
      </div>
      <button class="modal-close" type="button" data-close-modal aria-label="Close dialog">×</button>
    </div>
    <div class="modal-fields">
      <label class="field provd-wide">Key name<input type="text" required bind:value={editName} /></label>
      <label class="field provd-wide">Allowed models (comma separated, empty = all)<input type="text" bind:value={editModels} /></label>
      <div class="field provd-wide">
        Allowed providers
        <div style="display:flex;flex-wrap:wrap;gap:8px">
          {#each providerOptions as p (p.id)}
            <label style="display:flex;gap:6px;align-items:center;font-weight:400">
              <input type="checkbox" checked={editProviders.includes(p.id)} onchange={(e) => {
                const on = (e.currentTarget as HTMLInputElement).checked;
                editProviders = on ? [...editProviders, p.id] : editProviders.filter((id) => id !== p.id);
              }} />
              {p.name}
            </label>
          {/each}
        </div>
      </div>
      <label class="field provd-wide check-row-inline">
        <input type="checkbox" bind:checked={editEnabled} />
        Key enabled
      </label>
    </div>
    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>Cancel</button>
      <button class="page-action primary" type="submit" disabled={busy}>Save</button>
    </div>
  </form>
</dialog>

<!-- Disable / delete confirm -->
<dialog class="modal" bind:this={confirmDialog} aria-labelledby="confirm-title">
  <div class="modal-card">
    <div class="modal-heading">
      <div>
        <h2 id="confirm-title">{confirmMode === 'delete' ? 'Delete API key?' : 'Disable API key?'}</h2>
        <p>{confirmTarget ? (confirmMode === 'delete'
          ? `Permanently delete "${confirmTarget.name}"? This cannot be undone.`
          : `Requests using "${confirmTarget.name}" will start failing immediately.`) : ''}</p>
      </div>
      <button class="modal-close" type="button" data-close-modal aria-label="Close dialog">×</button>
    </div>
    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>Cancel</button>
      <button class="page-action danger" type="button" onclick={runConfirm} disabled={busy}>{confirmMode === 'delete' ? 'Delete permanently' : 'Disable key'}</button>
    </div>
  </div>
</dialog>
