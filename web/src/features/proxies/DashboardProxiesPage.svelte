<script lang="ts">
  import { onMount } from 'svelte';
  import { ArrowDown, ArrowUp, GitBranch, Plus, Save, Trash2 } from '@lucide/svelte';
  import ArkCheckbox from '../../components/ArkCheckbox.svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import { proxyApi } from '../../lib/api/proxies';
  import { localizedError } from '../../lib/errors';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import type { Translate } from '../../lib/format';
  import type { ProxyEndpoint, GatewayProxy, ProxyProviderOption, ProxyTrafficStats } from '../../lib/types';
  import BulkProxyImportDialog from './BulkProxyImportDialog.svelte';
  import ProxyEndpointEditor from './ProxyEndpointEditor.svelte';
  import { blankProxy, normalizeProxyDraft } from './proxy.state';
  import '../../styles/dashboard/pages/proxies.css';

  function formatBytes(value: number): string {
    if (value < 1024) return String(value) + ' B';
    const units = ['KiB', 'MiB', 'GiB', 'TiB'];
    let size = value;
    let unit = -1;
    do { size /= 1024; unit += 1; } while (size >= 1024 && unit < units.length - 1);
    return size.toLocaleString(undefined, { maximumFractionDigits: 1 }) + ' ' + units[unit];
  }

  function formatRate(value: number | null): string {
    return value === null ? '—' : formatBytes(value) + '/s';
  }

  interface Props { tr: Translate; locale: string; }
  let { tr, locale: _locale }: Props = $props();

  let proxies = $state<GatewayProxy[]>([]);
  let trafficStats = $state<ProxyTrafficStats[]>([]);
  let statsError = $state('');
  let statsTruncated = $state(false);
  let options = $state<ProxyProviderOption[]>([]);
  let optionsError = $state('');
  let optionsQuery = $state('');
  let optionsLoading = $state(false);
  let optionsLoaded = $state(false);
  let loading = $state(true);
  let saving = $state(false);
  let error = $state('');
  let query = $state('');
  let enabledFilter = $state<'all' | 'enabled' | 'disabled'>('all');
  let optionCursor = $state<string | null>(null);
  let editDialog = $state(false);
  let deleteDialog = $state(false);
  let draft = $state<Omit<GatewayProxy, 'id' | 'created_at' | 'updated_at' | 'provider_count'>>(blankProxy());
  let editingId = $state<string | null>(null);
  let deleting = $state<GatewayProxy | null>(null);
  let deleteBusy = $state(false);

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return proxies.filter((proxy) => {
      if (enabledFilter === 'enabled' && !proxy.enabled) return false;
      if (enabledFilter === 'disabled' && proxy.enabled) return false;
      return !q || proxy.name.toLowerCase().includes(q)
        || proxy.endpoints.some((endpoint) => `${endpoint.url ?? ''} ${endpoint.ref_proxy_id ?? ''} ${endpoint.label ?? ''}`.toLowerCase().includes(q));
    });
  });

  async function loadTrafficStats(): Promise<void> {
    try {
      const result = await proxyApi.trafficStats();
      trafficStats = result.proxies;
      statsTruncated = result.truncated;
      statsError = '';
    } catch (cause) {
      statsError = localizedError(cause, 'Could not load proxy traffic stats.', tr);
    }
  }

  async function load(): Promise<void> {
    loading = true;
    void loadTrafficStats();
    error = '';
    try {
      proxies = await proxyApi.proxies();
    } catch (cause) {
      error = localizedError(cause, 'Could not load proxies.', tr);
      loading = false;
      return;
    }
    try {
      await loadMoreProxyOptions('');
    } finally {
      loading = false;
    }
  }

  async function loadMoreProxyOptions(search = ''): Promise<void> {
    optionsLoading = true;
    try {
      const page = await proxyApi.proxyOptions({ q: search || undefined, cursor: search ? undefined : (optionCursor ?? undefined) });
      options = search
        ? page.proxies
        : [...options, ...page.proxies.filter((item) => !options.some((existing) => existing.id === item.id))];
      optionCursor = page.next_cursor ?? null;
      optionsQuery = search;
      optionsError = '';
      optionsLoaded = true;
    } catch (cause) {
      optionsError = localizedError(cause, 'Could not load proxy options.', tr);
    } finally {
      optionsLoading = false;
    }
  }

  function openCreate(): void {
    editingId = null;
    draft = blankProxy();
    editDialog = true;
  }

  function openEdit(proxy: GatewayProxy): void {
    editingId = proxy.id;
    draft = {
      name: proxy.name,
      strategy: proxy.strategy,
      enabled: proxy.enabled,
      endpoints: proxy.endpoints.map((endpoint) => ({ ...endpoint, secret: null })),
    };
    editDialog = true;
  }

  function addEndpoint(): void {
    if (draft.endpoints.length >= 64) return;
    draft.endpoints = [...draft.endpoints, { endpoint_id: crypto.randomUUID(), url: '', priority: draft.endpoints.length, enabled: true }];
  }

  function removeEndpoint(index: number): void {
    draft.endpoints = draft.endpoints.filter((_, current) => current !== index)
      .map((endpoint, priority) => ({ ...endpoint, priority }));
  }

  function moveEndpoint(index: number, direction: -1 | 1): void {
    const next = index + direction;
    if (next < 0 || next >= draft.endpoints.length) return;
    const endpoints = [...draft.endpoints];
    [endpoints[index], endpoints[next]] = [endpoints[next], endpoints[index]];
    draft.endpoints = endpoints.map((endpoint, priority) => ({ ...endpoint, priority }));
  }

  async function save(): Promise<void> {
    if (saving) return;
    const name = draft.name.trim();
    if (!name) { showDashboardToast(tr('Proxy name is required.')); return; }
    const endpoints = draft.endpoints.map((endpoint, priority) => ({ ...normalizeProxyDraft(endpoint), priority }));
    const hasInvalid = endpoints.some((endpoint) => !endpoint.ref_proxy_id && !endpoint.url);
    if (!endpoints.length || hasInvalid) {
      showDashboardToast(tr('Add at least one proxy URL or embedded proxy.'));
      return;
    }
    saving = true;
    try {
      const payload = { ...draft, name, endpoints };
      if (editingId) await proxyApi.updateProxy(editingId, payload);
      else await proxyApi.createProxy(payload);
      editDialog = false;
      await load();
      showDashboardToast(tr('Proxy saved.'));
    } catch (cause) {
      showDashboardToast(localizedError(cause, 'Could not save proxy.', tr));
    } finally {
      saving = false;
    }
  }

  async function confirmDelete(): Promise<void> {
    if (!deleting || deleteBusy) return;
    deleteBusy = true;
    try {
      await proxyApi.deleteProxy(deleting.id);
      deleteDialog = false;
      deleting = null;
      await load();
      showDashboardToast(tr('Proxy deleted.'));
    } catch (cause) {
      showDashboardToast(localizedError(cause, 'Could not delete proxy.', tr));
    } finally {
      deleteBusy = false;
    }
  }

  async function onBulkImported(created: number): Promise<void> {
    await load();
    showDashboardToast(tr('Created {count} proxies.', { count: created }));
  }

  onMount(() => { void load(); });
</script>

<div class="page-heading">
  <div class="heading-copy"><h1>{tr('Proxies')}</h1><p>{tr('Configure egress proxy groups and ordered connect fallbacks.')}</p></div>
  <div class="heading-actions">
    <BulkProxyImportDialog buttonClass="action-button" {tr} onImported={onBulkImported} />
    <button class="action-button primary" type="button" onclick={openCreate}><span class="button-icon"><svg class="icon"><use href="#i-plus" /></svg></span><span class="button-label">{tr('Add proxy')}</span></button>
  </div>
</div>

{#if !loading}
  <section class="proxy-traffic-panel course-card surface" aria-labelledby="proxy-traffic-heading">
    <div class="proxy-traffic-heading">
      <div><h2 id="proxy-traffic-heading">{tr('Network usage')}</h2><p>{tr('Application payload sent through configured proxy groups since this process started.')}</p></div>
      <button class="page-action" type="button" onclick={() => void loadTrafficStats()}>{tr('Refresh usage')}</button>
    </div>
    {#if statsError}<p class="form-error" role="alert">{statsError}</p>{/if}
    {#if statsTruncated}<p class="form-help" role="status">{tr('Proxy usage history was shortened because the runtime stats cache reached its limit.')}</p>{/if}
    <div class="proxy-traffic-grid">
      {#each proxies as proxy (proxy.id)}
        {@const stats = trafficStats.find((item) => item.proxy_id === proxy.id)}
        <article class="proxy-traffic-card" aria-label={proxy.name}>
          <strong>{proxy.name}</strong>
          <div><span>{tr('Received')}</span><b>{formatBytes(stats?.received_bytes ?? 0)}</b></div>
          <div><span>{tr('Sent')}</span><b>{formatBytes(stats?.sent_bytes ?? 0)}</b></div>
          <div><span>{tr('Total used')}</span><b>{formatBytes(stats?.total_bytes ?? 0)}</b></div>
          <div><span>{tr('Average speed')}</span><b>{formatRate(stats?.average_bytes_per_second ?? null)}</b></div>
          <small>{tr('{count} requests', { count: stats?.requests ?? 0 })}</small>
        </article>
      {:else}<p class="card-subtitle">{tr('No proxies configured yet.')}</p>{/each}
    </div>
    <small class="proxy-traffic-note">{tr('Traffic counts include application payload bytes only, not TLS or network protocol overhead. Runtime totals reset when the gateway restarts. Average speed uses combined request durations; it is not live link speed.')}</small>
  </section>
{/if}

{#if error}
  <section class="course-card surface"><div class="card-title-row"><h2>{tr('Could not load proxies')}</h2></div><p class="card-subtitle">{error}</p><button class="page-action" type="button" onclick={() => void load()}>{tr('Retry')}</button></section>
{:else if loading}
  <section class="stats-grid" aria-label={tr('Loading proxies')}>
    {#each [0, 1, 2, 3] as index (index)}<article class="stat-card surface" style="min-height:120px"></article>{/each}
  </section>
{:else}
  <div class="proxy-toolbar page-toolbar">
    <input class="prov-search" type="search" bind:value={query} placeholder={tr('Search proxies…')} aria-label={tr('Search proxies')} />
    <select class="prov-select" bind:value={enabledFilter} aria-label={tr('Filter proxies')}>
      <option value="all">{tr('All proxies')}</option><option value="enabled">{tr('Enabled')}</option><option value="disabled">{tr('Disabled')}</option>
    </select>
  </div>
  <div class="proxy-grid">
    {#each filtered as proxy (proxy.id)}
      <article class="course-card surface proxy-card">
        <div class="proxy-card-head"><span class="proxy-icon"><GitBranch size={18} /></span><div class="proxy-title"><strong>{proxy.name}</strong><code>{proxy.id}</code></div><span class="status" class:complete={proxy.enabled} class:pending={!proxy.enabled}>{tr(proxy.enabled ? 'Enabled' : 'Disabled')}</span></div>
        <div class="proxy-meta"><span>{tr('{count} endpoints', { count: proxy.endpoints.length })}</span><span>{tr('{count} providers', { count: proxy.provider_count })}</span><span>{tr(proxy.strategy === 'round_robin' ? 'Round robin' : 'Priority')}</span></div>
        <div class="proxy-endpoint-list">
          {#each proxy.endpoints.slice(0, 4) as endpoint (endpoint.priority)}
            <div class="proxy-endpoint-preview"><code>{endpoint.ref_proxy_id ? `↳ ${options.find((option) => option.id === endpoint.ref_proxy_id)?.name ?? endpoint.ref_proxy_id}` : endpoint.url}</code><span>{tr(endpoint.has_secret ? 'Auth' : 'No auth')}</span></div>
          {/each}
          {#if proxy.endpoints.length > 4}<small>{tr('+{count} more', { count: proxy.endpoints.length - 4 })}</small>{/if}
        </div>
        <div class="proxy-card-actions"><button class="page-action" type="button" onclick={() => openEdit(proxy)}><svg class="icon"><use href="#i-edit" /></svg>{tr('Edit')}</button><button class="page-action danger" type="button" onclick={() => { deleting = proxy; deleteDialog = true; }}><Trash2 size={14} />{tr('Delete')}</button></div>
      </article>
    {:else}
      <section class="course-card surface proxy-empty"><div class="empty-state">{query ? tr('No proxies match your search.') : tr('No proxies configured yet.')}</div></section>
    {/each}
  </div>
{/if}

<ArkDialog bind:open={editDialog} wide title={tr(editingId ? 'Edit proxy' : 'Add proxy')} kicker={tr('Egress proxy')} closeLabel={tr('Close dialog')} preventClose={saving} onClose={() => { editDialog = false; }}>
  <form class="modal-form proxy-editor" onsubmit={(event) => { event.preventDefault(); void save(); }}>
    <label class="field">{tr('Proxy name')}<input maxlength="256" bind:value={draft.name} placeholder={tr('e.g. Residential pool')} required /></label>
    <label class="field">{tr('Endpoint selection')}<select bind:value={draft.strategy}><option value="priority">{tr('Priority order')}</option><option value="round_robin">{tr('Round robin')}</option></select></label>
    <ArkCheckbox checked={draft.enabled} label={tr('Enable this proxy')} onCheckedChange={(checked) => draft.enabled = checked === true} />
    <div class="proxy-endpoints-title"><div><h3>{tr('Endpoints and groups')}</h3><p>{tr('A group expands in place. Fallback happens only when connecting to an endpoint fails.')}</p></div><button class="secondary-button compact" type="button" onclick={addEndpoint} disabled={draft.endpoints.length >= 64}><Plus size={14}/>{tr('Add endpoint')}</button></div>
    <div class="proxy-endpoints-editor">
      {#each draft.endpoints as endpoint, index (index)}
        <ProxyEndpointEditor
          {endpoint}
          {index}
          {options}
          optionsLoading={optionsLoading}
          optionsQuery={optionsQuery}
          hasMoreOptions={optionCursor !== null}
          {tr}
          onOptionsSearch={(search) => { void loadMoreProxyOptions(search); }}
          onOptionsMore={() => { void loadMoreProxyOptions(optionsQuery); }}
          onChange={(next) => draft.endpoints[index] = next}
          onMove={(direction) => moveEndpoint(index, direction)}
          onRemove={() => removeEndpoint(index)}
        />
      {/each}
    </div>
    <div class="modal-actions"><button class="secondary-button" type="button" onclick={() => editDialog = false}>{tr('Cancel')}</button><button class="primary-button" type="submit" disabled={saving}>{#if saving}<span class="spin">◌</span>{tr('Saving…')}{:else}<Save size={14}/>{tr('Save proxy')}{/if}</button></div>
  </form>
</ArkDialog>

<ArkDialog bind:open={deleteDialog} title={tr('Delete proxy?')} kicker={tr('Egress proxy')} closeLabel={tr('Close dialog')} onClose={() => { deleteDialog = false; }}>
  <div class="modal-form"><p>{tr('Delete {name}? Providers or proxy groups still using it will block deletion.', { name: deleting?.name ?? '' })}</p><div class="modal-actions"><button class="secondary-button" type="button" onclick={() => deleteDialog = false}>{tr('Cancel')}</button><button class="danger-button" type="button" disabled={deleteBusy} onclick={() => void confirmDelete()}>{tr(deleteBusy ? 'Deleting…' : 'Delete proxy')}</button></div></div>
</ArkDialog>

