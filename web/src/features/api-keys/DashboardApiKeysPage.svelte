<script lang="ts">
  import Button from '../../components/ui/Button.svelte';
  import Dialog from '../../components/ui/Dialog.svelte';
  import StatsCard from '../../components/ui/StatsCard.svelte';
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import Input from '../../components/ui/Input.svelte';
  import { localizedError } from '../../lib/errors';
  import { formatCount, formatCompactTokenTotal, formatCostMicroUsd, type Translate } from '../../lib/format';
  import type { DashboardPageRoute } from '../../lib/navigation';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import SelectOptions from '../../components/ui/SelectOptions.svelte';
  import type { GatewayApiKey, RequestStatistics } from '../../lib/types';
  import type { Locale } from '../../lib/i18n';
  import ApiKeyScopeFields from './ApiKeyScopeFields.svelte';
  import ApiKeyInspector from './ApiKeyInspector.svelte';
  import ApiKeyBreakdown from './ApiKeyBreakdown.svelte';
  import ApiKeyDetailPage from './ApiKeyDetailPage.svelte';
  import { modelRulesText, parseModelRules } from './scope';
  import { EMPTY_METRICS, filterAndSortKeys, formatIdleLabel, isStale, KEY_RANGES, metricsFromStatistics, type KeyMetrics, type KeyRange, type KeyScopeFilter, type KeySort, type KeyStatus } from './api-keys.data';
  interface Props { tr: Translate; locale: Locale; page?: DashboardPageRoute; onNavigate: (page: DashboardPageRoute) => void }
  let { tr, locale, page = 'api-keys', onNavigate }: Props = $props();
  let keys = $state<GatewayApiKey[]>([]);
  let nextCursor = $state<string | null>(null);
  let stats = $state<RequestStatistics | null>(null);
  let providerOptions = $state<{ id: string; name: string }[]>([]);
  let metrics = $state<Record<string, KeyMetrics>>({});
  let loading = $state(true);
  let error = $state('');
  let searchQuery = $state('');
  let statusFilter = $state<KeyStatus | 'all'>('all');
  let scopeFilter = $state<KeyScopeFilter>('all');
  let sortBy = $state<KeySort>('traffic');
  let view = $state<'table' | 'card'>('table');
  let range = $state<KeyRange>('1d');
  let scopeOpen = $state<string | null>(null);
  let guideOpen = $state(true);
  const gatewayBaseUrl = 'http://127.0.0.1:8686/v1';
  let keyDialogOpen = $state(false);
  let editDialogOpen = $state(false);
  let confirmDialogOpen = $state(false);
  let newName = $state(''); let newModels = $state(''); let newProviders = $state<string[]>([]);
  let createdKey = $state<{ name: string; key: string } | null>(null);
  let editTarget = $state<GatewayApiKey | null>(null);
  let editName = $state(''); let editModels = $state(''); let editProviders = $state<string[]>([]); let editEnabled = $state(true);
  let confirmTarget = $state<GatewayApiKey | null>(null); let confirmMode = $state<'disable' | 'delete'>('disable');
  let busy = $state(false);
  let inspectorKeyId = $state<string | null>(null);
  let controller: AbortController | null = null;
  const detailRouteId = $derived(page.startsWith('api-key-detail:') ? page.slice('api-key-detail:'.length) : null);
  const detailKey = $derived(detailRouteId ? keys.find((key) => key.id === detailRouteId) ?? null : null);
  const inspectorKey = $derived(inspectorKeyId ? keys.find((key) => key.id === inspectorKeyId) ?? null : null);
  const visibleKeys = $derived(filterAndSortKeys(keys, { query: searchQuery, status: statusFilter, scope: scopeFilter, sort: sortBy }, metrics));
  const activeCount = $derived(keys.filter((k) => k.enabled).length);
  const pausedCount = $derived(keys.filter((k) => !k.enabled).length);
  const staleCount = $derived(keys.filter((k) => isStale(k)).length);
  const failingCount = $derived(keys.filter((k) => (metrics[k.id]?.failures ?? 0) > 0).length);
  const totalTokens = $derived(stats ? stats.input_tokens + stats.output_tokens + stats.cached_tokens : 0);
  const costValue = $derived(stats ? formatCostMicroUsd(stats.cost.cost_reported_micro_usd) : '—');
  function statusLabel(key: GatewayApiKey): string {
    return key.enabled ? tr('Active') : tr('Paused');
  }
  function failureTone(failures: number, requests: number): 'mild' | 'strong' {
    return requests > 0 && failures / requests >= 0.05 ? 'strong' : 'mild';
  }
  function idleText(key: GatewayApiKey): string { return formatIdleLabel(key, tr); }
  async function loadMetrics(signal?: AbortSignal): Promise<void> {
    const next: Record<string, KeyMetrics> = {};
    const queue = [...keys.slice(0, 30)];
    const worker = async () => {
      while (queue.length) {
        const key = queue.shift()!;
        try { next[key.id] = metricsFromStatistics(await api.apiKeyStatistics(key.id, range, signal) as unknown as RequestStatistics); }
        catch { next[key.id] = { ...EMPTY_METRICS, error: true }; }
      }
    };
    await Promise.all([worker(), worker(), worker(), worker()]);
    metrics = next;
  }
  async function load(signal?: AbortSignal) {
    loading = true; error = '';
    try {
      const [page, s, provs] = await Promise.all([api.apiKeys({}, signal), api.statistics(range, signal).catch(() => null), api.providers().catch(() => null)]);
      keys = page.api_keys; nextCursor = page.next_cursor ?? null; stats = s;
      providerOptions = (provs ?? []).map((p) => ({ id: p.id, name: p.name }));
      await loadMetrics(signal);
    } catch (cause) { error = localizedError(cause, 'Could not load API keys.', tr); }
    finally { loading = false; }
  }
  onMount(() => { controller = new AbortController(); void load(controller.signal); });
  onDestroy(() => { controller?.abort(); });
  function openCreate() { newName = ''; newModels = ''; newProviders = []; createdKey = null; keyDialogOpen = true; }
  async function submitCreate(event: SubmitEvent) {
    event.preventDefault(); if (!newName.trim() || busy) return; busy = true;
    try {
      const created = await api.createApiKey(newName.trim(), { allowed_models: parseModelRules(newModels), allowed_provider_ids: newProviders });
      createdKey = { name: created.name, key: created.key };
      showDashboardToast(tr('API key created — copy it now, it is shown once')); await load();
    } catch (cause) { showDashboardToast(localizedError(cause, 'Could not create API key.', tr)); } finally { busy = false; }
  }
  async function copyText(text: string, label: string) {
    try { await navigator.clipboard.writeText(text); showDashboardToast(tr('{label} copied', { label })); }
    catch { showDashboardToast(tr('Copy failed')); }
  }
  function openEdit(key: GatewayApiKey) {
    editTarget = key; editName = key.name; editModels = modelRulesText(key.allowed_models);
    editProviders = [...(key.allowed_provider_ids ?? [])]; editEnabled = key.enabled; editDialogOpen = true;
  }
  async function submitEdit(event: SubmitEvent) {
    event.preventDefault(); if (!editTarget || busy) return; busy = true;
    try {
      await api.updateApiKey(editTarget.id, { name: editName.trim() || undefined, enabled: editEnabled, allowed_models: parseModelRules(editModels), allowed_provider_ids: editProviders });
      editDialogOpen = false; showDashboardToast(tr('Key updated')); await load();
    } catch (cause) { showDashboardToast(localizedError(cause, 'Could not update API key.', tr)); } finally { busy = false; }
  }
  async function toggleEnabled(key: GatewayApiKey) {
    if (busy) return; busy = true;
    try { await api.updateApiKey(key.id, { enabled: !key.enabled }); showDashboardToast(tr('Key updated')); await load(); }
    catch (cause) { showDashboardToast(localizedError(cause, 'Action failed.', tr)); } finally { busy = false; }
  }
  function askConfirm(key: GatewayApiKey, mode: 'disable' | 'delete') { confirmTarget = key; confirmMode = mode; confirmDialogOpen = true; }
  async function runConfirm() {
    if (!confirmTarget || busy) return; busy = true;
    try {
      if (confirmMode === 'delete') { await api.deleteApiKey(confirmTarget.id); if (detailKey?.id === confirmTarget.id) onNavigate('api-keys'); if (inspectorKeyId === confirmTarget.id) inspectorKeyId = null; showDashboardToast(tr('Deleted "{name}"', { name: confirmTarget.name })); }
      else { await api.updateApiKey(confirmTarget.id, { enabled: false }); showDashboardToast(tr('Disabled "{name}"', { name: confirmTarget.name })); }
      confirmDialogOpen = false; await load();
    } catch (cause) { showDashboardToast(localizedError(cause, 'Action failed.', tr)); } finally { busy = false; }
  }
  function exportKeys() {
    const rows = [['name', 'id', 'enabled', 'created_at', 'last_used_at', 'scope']];
    for (const k of keys) rows.push([k.name, k.id, String(k.enabled), k.created_at, k.last_used_at ?? '', (k.allowed_models ?? []).join('|')]);
    const blob = new Blob([rows.map((r) => r.map((c) => `"${String(c).replaceAll('"', '""')}"`).join(',')).join('\n')], { type: 'text/csv' });
    const a = document.createElement('a'); a.href = URL.createObjectURL(blob); a.download = 'exoroute-api-keys.csv'; a.click(); URL.revokeObjectURL(a.href);
    showDashboardToast(tr('Exported API keys CSV'));
  }
</script>

{#if detailRouteId}
  <ApiKeyDetailPage
    apiKey={detailKey}
    keyId={detailRouteId}
    loading={loading}
    error={error}
    {tr}
    {locale}
    onBack={() => onNavigate('api-keys')}
    onEdit={openEdit}
    onToggle={toggleEnabled}
    {onNavigate}
    onRetry={() => load()}
  />
{:else}
<div class="page-heading">
  <div class="heading-copy"><h1>{tr('API keys')}</h1><p>{tr('Client credentials for the gateway. Secrets are shown once at creation.')}</p></div>
  <div class="heading-actions">
    <Button class="action-button" type="button" onclick={exportKeys}><span class="button-icon"><svg class="icon"><use href="#i-download" /></svg></span><span class="button-label">{tr('Export')}</span></Button>
    <Button class="action-button primary" type="button" onclick={openCreate}><span class="button-icon"><svg class="icon"><use href="#i-plus" /></svg></span><span class="button-label">{tr('New API key')}</span></Button>
  </div>
</div>
<div class="page-surface">
  {#if error}<div class="empty-state">{error} <Button class="mini-add" type="button" onclick={() => load()}>{tr('Retry')}</Button></div>
  {:else}
    <section class="key-guide" class:collapsed={!guideOpen} aria-label={tr('How to use API keys')}>
    <button type="button" class="guide-toggle" aria-expanded={guideOpen} onclick={() => (guideOpen = !guideOpen)}><strong>{tr('How to use API keys')}</strong><span>{guideOpen ? '−' : '+'}</span></button>
    {#if guideOpen}<div class="guide-body">
      <div><span>{tr('Step')} 1</span><p>{tr('Create a key')} — {tr('New API key')}</p></div>
      <div><span>{tr('Step')} 2</span><p>{tr('Set Base URL')}: <code>{gatewayBaseUrl}</code> <Button class="mini-add" type="button" onclick={() => copyText(gatewayBaseUrl, tr('Copy base URL'))}>{tr('Copy base URL')}</Button></p></div>
      <div><span>{tr('Step')} 3</span><p>{tr('Pass the key as a Bearer token')}: <code>Authorization: Bearer &lt;key&gt;</code></p></div>
    </div>{/if}
  </section>

  <section class="stats-grid" aria-label={tr('API key overview')}>
      <StatsCard index={1} eyebrow={tr('Keys')} badgeTone="dot" badge={String(activeCount)} value={formatCount(keys.length, locale)} title={tr('Active Keys')} trend={`${activeCount} ${tr('Active')} · ${pausedCount} ${tr('Paused')} · ${failingCount} ${tr('keys with recent errors')}`} />
      <StatsCard index={2} eyebrow={tr('Traffic')} badge={KEY_RANGES.find((r) => r.key === range)?.label} value={stats ? formatCount(stats.total_requests, locale) : '—'} title={tr('Requests')} trend={tr('Request volume in the selected time window')} />
      <StatsCard index={3} eyebrow={tr('Spend')} value={costValue} title={tr('Spend')} trend={stats ? `${tr('Input')} / ${tr('Output')} / ${tr('Cache')}: ${formatCompactTokenTotal(totalTokens, locale)}` : tr('Cost unavailable')} />
<button type="button" class="kpi-clickable" onclick={() => (statusFilter = statusFilter === 'erroring' ? 'all' : 'erroring')} title={tr('Keys with errors')}>
              <StatsCard index={4} eyebrow={tr('Reliability')} badgeTone={failingCount > 0 ? 'chip' : 'success'} badge={failingCount > 0 ? `${failingCount} ${tr('failed')}` : undefined} value={stats ? formatCount(stats.failures, locale) : '—'} title={tr('Failures Today')} trend={stats ? `${stats.success_rate.toFixed(1)}% ${tr('success')}` : tr('24h window')} />
      </button>
    </section>
    <ApiKeyBreakdown tr={tr} {locale} {keys} {range} onRangeChange={(r) => (range = r)} onInspect={(key) => onNavigate(`api-key-detail:${key.id}`)} />
    <section class="course-card surface keys-board" aria-labelledby="keys-heading">
      <div class="card-title-row"><h2 id="keys-heading">{tr('Keys')}</h2><div class="view-switch"><button type="button" aria-pressed={view === 'table'} class:active={view === 'table'} onclick={() => (view = 'table')}>{tr('Table view')}</button><button type="button" aria-pressed={view === 'card'} class:active={view === 'card'} onclick={() => (view = 'card')}>{tr('Card view')}</button></div></div>
      <div class="page-toolbar items-center">
        <Input class="w-[min(280px,100%)] flex-[1_1_200px]" type="search" placeholder={tr('Search keys…')} aria-label={tr('Search API keys')} bind:value={searchQuery} />
        <SelectOptions class="prov-select" ariaLabel={tr('Filter keys')} bind:value={statusFilter} items={[{ label: tr('All statuses'), value: 'all' }, { label: tr('Active'), value: 'active' }, { label: tr('Paused keys'), value: 'paused' }, { label: tr('Keys with errors'), value: 'erroring' }, { label: tr('Stale keys'), value: 'stale' }]} />
        <SelectOptions class="prov-select" ariaLabel={tr('All scopes')} bind:value={scopeFilter} items={[{ label: tr('All scopes'), value: 'all' }, { label: tr('Any model'), value: 'any' }, { label: tr('Restricted models'), value: 'restricted' }]} />
        <SelectOptions class="prov-select" ariaLabel={tr('Sort by')} bind:value={sortBy} items={[{ label: tr('Most traffic'), value: 'traffic' }, { label: tr('Most failures'), value: 'failures' }, { label: tr('Last used recently'), value: 'lastUsed' }, { label: tr('Name A–Z'), value: 'name' }]} />
      </div>
      {#if view === 'table'}
      <div class="key-table-wrap">
        <table class="key-table">
          <thead><tr><th>{tr('Key name & prefix')}</th><th>{tr('Operational status')}</th><th>{tr('Model scope')}</th><th>{tr('Traffic & tokens')}</th><th>{tr('Reliability')}</th><th>{tr('Last used')}</th><th>{tr('Actions')}</th></tr></thead>
          <tbody>
          {#each visibleKeys as key (key.id)}
            <tr class="key-row" onclick={() => onNavigate(`api-key-detail:${key.id}`)} tabindex="0" onkeydown={(e) => e.key === 'Enter' && onNavigate(`api-key-detail:${key.id}`)}>
              <td><button type="button" class="key-name" onclick={() => onNavigate(`api-key-detail:${key.id}`)}>{key.name}</button><div class="key-prefix"><code>{key.id.slice(0, 8)}…</code><button type="button" title={tr('Copy prefix')} onclick={() => copyText(key.id, tr('Key prefix'))}><svg class="icon"><use href="#i-copy" /></svg></button></div></td>
              <td><span class="status-badge {key.enabled ? 'complete' : 'pending'}" title={tr('Operational status')}><i></i>{statusLabel(key)}</span>{#if isStale(key)}<span class="badge-warn">{tr('Stale')}</span>{/if}</td>
              <td>
                {#if !key.scope_restricted}<span class="badge">{tr('Any model')}</span>
                {:else}<div class="scope-cell"><span class="badge">{key.allowed_models?.[0] ?? tr('Restricted models')}</span>{#if (key.allowed_models?.length ?? 0) > 1}<button type="button" class="badge more" onclick={() => (scopeOpen = scopeOpen === key.id ? null : key.id)}>+{(key.allowed_models?.length ?? 0) - 1}</button>{/if}</div>
                  {#if scopeOpen === key.id}<div class="scope-popover">{#each key.allowed_models ?? [] as model}<code>{model}</code>{/each}</div>{/if}{/if}
              </td>
              <td class="numeric tabular-nums">{metrics[key.id]?.loaded ? `${formatCount(metrics[key.id].requests, locale)} ${tr('requests')}` : '—'}<div class="muted">{metrics[key.id]?.loaded ? `${formatCompactTokenTotal(metrics[key.id].tokens, locale)} ${tr('Tokens')}` : ''}</div></td>
              <td class="numeric tabular-nums">{metrics[key.id]?.loaded ? (metrics[key.id].successRate?.toFixed(1) ?? '—') + '%' : '—'}{#if (metrics[key.id]?.failures ?? 0) > 0}<button type="button" class="badge-warn error-badge {failureTone(metrics[key.id].failures, metrics[key.id].requests)}" title={tr('Failed requests using this key')} onclick={(e) => { e.stopPropagation(); onNavigate(`api-key-detail:${key.id}`); }}>{formatCount(metrics[key.id].failures, locale)} {tr('failed')}</button>{/if}</td>
              <td title={key.last_used_at ?? ''}>{idleText(key)}</td>
              <td class="row-actions"><button type="button" class="action-detail" title={tr('Details')} aria-label={tr('Details')} onclick={(e) => { e.stopPropagation(); onNavigate(`api-key-detail:${key.id}`); }}>{tr('Details')}</button><button type="button" title={tr('Edit key')} aria-label={tr('Edit key')} onclick={(e) => { e.stopPropagation(); openEdit(key); }}><svg class="icon"><use href="#i-edit" /></svg></button><button type="button" title={key.enabled ? tr('Pause') : tr('Resume')} aria-label={key.enabled ? tr('Pause') : tr('Resume')} onclick={(e) => { e.stopPropagation(); key.enabled ? askConfirm(key, 'disable') : toggleEnabled(key); }}>{#if key.enabled}<span class="pause-mark">II</span>{:else}<svg class="icon"><use href="#i-play" /></svg>{/if}</button><button type="button" class="danger" title={tr('Delete permanently')} aria-label={tr('Delete permanently')} onclick={(e) => { e.stopPropagation(); askConfirm(key, 'delete'); }}><svg class="icon"><use href="#i-trash" /></svg></button></td>
            </tr>
          {/each}
          </tbody>
        </table>
      </div>
      {:else}
      <div class="key-cards">
        {#each visibleKeys as key (key.id)}
          <article class="key-card">
            <button type="button" class="key-name" onclick={() => onNavigate(`api-key-detail:${key.id}`)}>{key.name}</button>
            <div class="key-prefix"><code>{key.id.slice(0, 8)}…</code><button type="button" title={tr('Copy prefix')} onclick={() => copyText(key.id, tr('Key prefix'))}><svg class="icon"><use href="#i-copy" /></svg></button></div>
            <p><span class="status {key.enabled ? 'complete' : 'pending'}">{statusLabel(key)}</span>{#if isStale(key)} <span class="badge-warn">{tr('Stale')}</span>{/if}</p>
            <p class="muted">{idleText(key)}</p>
            <div class="card-metrics tabular-nums"><span>{metrics[key.id]?.loaded ? formatCount(metrics[key.id].requests, locale) : '—'} {tr('Requests')}</span><span>{metrics[key.id]?.loaded ? formatCompactTokenTotal(metrics[key.id].tokens, locale) : '—'} {tr('Tokens')}</span></div>
            <div class="card-actions"><Button class="mini-add" type="button" onclick={() => openEdit(key)}>{tr('Edit key')}</Button><Button class="mini-add" type="button" onclick={() => askConfirm(key, 'delete')}>{tr('Revoke')}</Button></div>
          </article>
        {/each}
      </div>
      {/if}
      {#if loading}<div class="empty-state">{tr('Loading…')}</div>{:else if visibleKeys.length === 0}<div class="empty-state">{keys.length ? tr('No keys match the current filters.') : tr('No keys yet')}</div>{/if}
    </section>
  {/if}
</div>
{/if}

<ApiKeyInspector tr={tr} {locale} apiKey={inspectorKey} metrics={inspectorKey ? metrics[inspectorKey.id] ?? EMPTY_METRICS : EMPTY_METRICS} onClose={() => (inspectorKeyId = null)} {onNavigate} />

<!-- New API key (preserved reauth + one-time secret reveal) -->
<Dialog bind:open={keyDialogOpen} title={createdKey ? tr('Key created') : tr('New API key')} kicker="" closeLabel={tr('Close dialog')}>
  <form class="modal-form" onsubmit={submitCreate}>
    <p class="modal-description">{createdKey ? tr('Copy it now — it will not be shown again.') : tr('Create a client credential. The secret is shown once.')}</p>
    {#if createdKey}
      <div class="mt-4 flex flex-col gap-1.5 rounded-[14px] border border-line bg-surface-sunken px-[18px] py-4"><strong>{createdKey.name}</strong><div class="mt-1 flex items-center gap-2.5"><code class="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap rounded-[10px] border border-text-dim bg-surface px-3.5 py-2.5 text-[13px] tracking-[0.02em] select-all">{createdKey.key}</code><Button class="page-action" type="button" onclick={() => copyText(createdKey!.key, tr('API key'))}><svg class="icon"><use href="#i-copy" /></svg> {tr('Copy')}</Button></div></div>
      <div class="modal-actions"><Button class="page-action primary" type="button" onclick={() => (keyDialogOpen = false)}>{tr('Done')}</Button></div>
    {:else}
      <div class="modal-fields"><label class="field provd-wide">{tr('Key name')}<Input type="text" required placeholder={tr('e.g. staging-app')} bind:value={newName} /></label><div class="field provd-wide api-key-create-scope"><ApiKeyScopeFields modelRulesId="new-api-key-model-rules" {tr} providers={providerOptions} bind:selectedProviderIds={newProviders} bind:modelRules={newModels} /></div></div>
      <div class="modal-actions"><Button class="page-action" type="button" data-close-modal>{tr('Cancel')}</Button><Button class="page-action primary" type="submit" disabled={busy || !newName.trim()}>{tr('Create key')}</Button></div>
    {/if}
  </form>
</Dialog>

<!-- Edit key -->
<Dialog bind:open={editDialogOpen} title={tr('Edit API key')} kicker="" wide closeLabel={tr('Close dialog')} class="w-[min(94vw,680px)]! max-h-[calc(100dvh-32px)]! overflow-visible! max-[640px]:w-[min(94vw,480px)]!">
  <form class="modal-form max-h-[calc(100dvh-32px)]! gap-4! overflow-hidden! rounded-[inherit] [grid-template-rows:auto_minmax(0,1fr)_auto]! px-6! py-[22px]! max-[640px]:px-4!" onsubmit={submitEdit}>
    <p class="modal-description">{editTarget ? tr('Adjust name, scope and state for "{name}".', { name: editTarget.name }) : ''}</p>
    <div class="modal-fields grid-cols-[minmax(0,1fr)_minmax(0,1fr)]! gap-3.5! min-h-0 overflow-y-auto max-[640px]:grid-cols-1!">
      <label class="field col-span-full">{tr('Key name')}<Input type="text" required bind:value={editName} /></label>
      <div class="col-span-full rounded-xl border border-line bg-surface-sunken p-3.5"><ApiKeyScopeFields {tr} providers={providerOptions} modelRulesId="edit-api-key-model-rules" bind:selectedProviderIds={editProviders} bind:modelRules={editModels} /></div>
      <label class="col-span-full flex items-center gap-3 cursor-pointer"><Input type="checkbox" bind:checked={editEnabled} /><span><strong>{tr('Key enabled')}</strong></span></label>
    </div>
    <div class="modal-actions pt-3 border-t border-line"><Button class="page-action" type="button" data-close-modal>{tr('Cancel')}</Button><Button class="page-action primary" type="submit" disabled={busy}>{tr('Save')}</Button></div>
  </form>
</Dialog>

<!-- Disable / delete confirm -->
<Dialog bind:open={confirmDialogOpen} title={confirmMode === 'delete' ? tr('Delete API key?') : tr('Disable API key?')} kicker="" closeLabel={tr('Close dialog')}>
  <p class="modal-description">{confirmTarget ? (confirmMode === 'delete' ? tr('Permanently delete "{name}"? This cannot be undone.', { name: confirmTarget.name }) : tr('Requests using "{name}" will start failing immediately.', { name: confirmTarget.name })) : ''}</p>
  <div class="modal-actions"><Button class="page-action" type="button" data-close-modal>{tr('Cancel')}</Button><Button class="page-action danger" type="button" onclick={runConfirm} disabled={busy}>{confirmMode === 'delete' ? tr('Delete permanently') : tr('Disable key')}</Button></div>
</Dialog>
