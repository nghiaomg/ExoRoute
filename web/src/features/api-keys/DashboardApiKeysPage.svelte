<script lang="ts">
  import Button from "../../components/Button.svelte";
  import StatsCard from "../../components/StatsCard.svelte";
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import Input from '../../components/Input.svelte';
  import { localizedError } from '../../lib/errors';
  import { formatCostMicroUsd, type Translate } from '../../lib/format';
  import type { DashboardPage } from '../../lib/navigation';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import SelectOptions from '../../components/SelectOptions.svelte';
  import type { GatewayApiKey, RequestStatistics } from '../../lib/types';
  import type { Locale } from '../../lib/i18n';
  import ApiKeyDetailSheet from './ApiKeyDetailSheet.svelte';
  import ApiKeyScopeFields from './ApiKeyScopeFields.svelte';
  import { modelRulesText, parseModelRules } from './scope';

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
  let detailKeyId = $state<string | null>(null);
  const detailKey = $derived(detailKeyId ? keys.find((key) => key.id === detailKeyId) ?? null : null);

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
      // The selected detail is derived from this refreshed page of records.
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
        allowed_models: parseModelRules(newModels),
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
    editModels = modelRulesText(key.allowed_models);
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
        allowed_models: parseModelRules(editModels),
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
  /** Deletion removes the row, so leave its detail page first. */
  function openDetail(key: GatewayApiKey) {
    detailKeyId = key.id;
  }

  function closeDetail(): void {
    detailKeyId = null;
  }
  async function runConfirm() {
    if (!confirmTarget || busy) return;
    busy = true;
    try {
      if (confirmMode === 'delete') {
        await api.deleteApiKey(confirmTarget.id);
        if (detailKey?.id === confirmTarget.id) closeDetail();
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

{#if detailKey}
  <ApiKeyDetailSheet
    apiKey={detailKey}
    {tr}
    {locale}
    onUpdated={() => void load()}
    onBack={closeDetail}
  />
{:else}
<div class="page-heading">
  <div class="heading-copy">
    <h1>{tr('API keys')}</h1>
    <p>{tr('Client credentials for the gateway. Secrets are shown once at creation.')}</p>
  </div>
  <div class="heading-actions">
    <Button class="action-button" type="button" onclick={exportKeys}>
      <span class="button-icon"><svg class="icon"><use href="#i-download" /></svg></span>
      <span class="button-label">{tr('Export')}</span>
    </Button>
    <Button class="action-button primary" type="button" onclick={openCreate}>
      <span class="button-icon"><svg class="icon"><use href="#i-plus" /></svg></span>
      <span class="button-label">{tr('New API key')}</span>
    </Button>
  </div>
</div>

<div class="page-surface">
  {#if error}
    <div class="empty-state">{error} <Button class="mini-add" type="button" onclick={() => load()}>{tr('Retry')}</Button></div>
  {:else}
    <section class="stats-grid" aria-label={tr('API key overview')}>
      <StatsCard index={1} eyebrow={tr('Keys')} badgeTone="dot" badge={String(activeCount)} value={activeCount} title={tr('Active Keys')} trend={tr('Showing {count} API keys on this page', { count: keys.length })} />
      <StatsCard index={2} eyebrow={tr('Traffic')} badge={stats ? tr('24H') : undefined} value={stats ? stats.total_requests.toLocaleString() : '—'} title={tr('Requests Today')} trend={tr('Across all keys · 24h')} />
      <StatsCard index={3} eyebrow={tr('Spend')} value={stats ? formatCostMicroUsd(stats.cost.cost_reported_micro_usd) : '—'} title={tr('Spend Today')} trend={tr('Estimated upstream cost')} />
      <StatsCard index={4} eyebrow={tr('Reliability')} value={stats ? stats.failures.toLocaleString() : '—'} title={tr('Failures Today')} trend={stats ? `${stats.success_rate.toFixed(1)}% ${tr('success')}` : tr('24h window')} />
    </section>

    <section class="course-card surface keys-board" id="keys-board" bind:this={boardEl} aria-labelledby="keys-heading">
      <div class="card-title-row"><h2 id="keys-heading">{tr('Keys')}</h2></div>
      <p class="card-subtitle">{tr('Showing {count} API keys on this page', { count: keys.length })}{nextCursor ? ` · ${tr('more available')}` : ''}</p>
      <div class="page-toolbar items-center">
        <Input class="w-[min(280px,100%)] flex-[1_1_200px]" type="search" placeholder={tr('Search keys…')} aria-label={tr('Search API keys')} bind:value={searchQuery} />
        <SelectOptions
          class="prov-select"
          ariaLabel={tr('Filter keys')}
          bind:value={statusFilter}
          items={[{ label: tr('All keys'), value: 'all' }, { label: tr('Active'), value: 'active' }, { label: tr('Disabled'), value: 'disabled' }]}
        />
      </div>
      <div class="mt-3.5 grid grid-cols-[repeat(3,minmax(0,1fr))] items-start gap-2.5 max-[1100px]:grid-cols-2 max-nav:grid-cols-1">
        {#each visibleKeys as key (key.id)}
          <article class="course-row key-card min-w-0 items-start flex-wrap {!key.enabled ? 'opacity-72' : ''}">
            <!-- The row opens a full-page detail view; adjacent actions stay independent. -->
            <Button
              class="key-card-trigger"
              type="button"
              aria-label={tr('Open {name} details', { name: key.name })}
              onclick={() => openDetail(key)}
            >
              <span class="course-icon"><svg class="icon"><use href="#i-key" /></svg></span>
              <span class="course-copy">
                <strong>{key.name} <span class="ms-1.5 px-[7px] py-0.5 rounded-full bg-surface-sunken text-muted text-caption font-medium align-[1px]">{scopeLabel(key)}</span></strong>
                <span>{key.id.slice(0, 8)}… · {key.last_used_at ? tr('Used {date}', { date: key.last_used_at }) : tr('Never used')} · {key.request_count ?? 0} {tr('requests')}</span>
              </span>
            </Button>
            <span class="status {key.enabled ? 'complete' : 'pending'}">{key.enabled ? tr('Active') : tr('Disabled')}</span>
            <div class="course-actions">
              <Button class="icon-action" type="button" title={tr('Edit key')} onclick={() => openEdit(key)}><svg class="icon"><use href="#i-edit" /></svg></Button>
              {#if key.enabled}
                <Button class="icon-action danger hover:text-db-danger! hover:bg-db-accent-tint!" type="button" title={tr('Disable key')} onclick={() => askConfirm(key, 'disable')}><svg class="icon"><use href="#i-x-circle" /></svg></Button>
              {:else}
                <Button class="icon-action danger hover:text-db-danger! hover:bg-db-accent-tint!" type="button" title={tr('Delete permanently')} onclick={() => askConfirm(key, 'delete')}><svg class="icon"><use href="#i-trash" /></svg></Button>
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
      <Button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</Button>
    </div>
    {#if createdKey}
      <div class="mt-4 flex flex-col gap-1.5 rounded-[14px] border border-dashed border-text-dim bg-surface-sunken px-[18px] py-4">
        <strong>{createdKey.name}</strong>
        <div class="mt-1 flex items-center gap-2.5">
          <code class="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap rounded-[10px] border border-text-dim bg-surface px-3.5 py-2.5 text-[13px] tracking-[0.02em] select-all">{createdKey.key}</code>
          <Button class="page-action" type="button" onclick={() => copyText(createdKey!.key, tr('API key'))}><svg class="icon"><use href="#i-copy" /></svg> {tr('Copy')}</Button>
        </div>
      </div>
      <div class="modal-actions">
        <Button class="page-action primary" type="button" onclick={() => keyDialog?.close()}>{tr('Done')}</Button>
      </div>
    {:else}
      <div class="modal-fields">
        <label class="field provd-wide">{tr('Key name')}<Input type="text" required placeholder={tr('e.g. staging-app')} bind:value={newName} /></label>
        <div class="field provd-wide api-key-create-scope">
          <ApiKeyScopeFields modelRulesId="new-api-key-model-rules" {tr} providers={providerOptions} bind:selectedProviderIds={newProviders} bind:modelRules={newModels} />
        </div>
      </div>
      <div class="modal-actions">
        <Button class="page-action" type="button" data-close-modal>{tr('Cancel')}</Button>
        <Button class="page-action primary" type="submit" disabled={busy || !newName.trim()}>{tr('Create key')}</Button>
      </div>
    {/if}
  </form>
</dialog>

<!-- Edit key -->
<dialog class="modal modal-wide w-[min(94vw,680px)]! max-h-[calc(100dvh-32px)]! overflow-visible! max-[640px]:w-[min(94vw,480px)]! max-[640px]:max-h-[calc(100dvh-20px)]!" bind:this={editDialog} aria-labelledby="edit-dialog-title">
  <form class="modal-card max-h-[calc(100dvh-32px)]! gap-4! overflow-hidden! rounded-[inherit] [grid-template-rows:auto_minmax(0,1fr)_auto]! px-6! py-[22px]! max-[640px]:max-h-[calc(100dvh-20px)]! max-[640px]:gap-3! max-[640px]:px-4! max-[640px]:py-4!" onsubmit={submitEdit}>
    <div class="modal-heading">
      <div>
        <h2 id="edit-dialog-title">{tr('Edit API key')}</h2>
        <p>{editTarget ? tr('Adjust name, scope and state for "{name}".', { name: editTarget.name }) : ''}</p>
      </div>
      <Button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</Button>
    </div>
    <div class="modal-fields grid-cols-[minmax(0,1fr)_minmax(0,1fr)]! gap-3.5! min-h-0 overflow-y-auto pt-px pr-[3px] pb-1 pl-px content-start max-[640px]:grid-cols-1! max-[640px]:gap-2.5!">
      <label class="field col-span-full min-w-0 m-0 rounded-xl border border-line bg-surface-sunken p-3.5 max-[640px]:p-3">
        {tr('Key name')}
        <Input type="text" required bind:value={editName} />
      </label>
      <div class="col-span-full min-w-0 m-0 rounded-xl border border-line bg-surface-sunken p-3.5 max-[640px]:p-3 max-[640px]:col-span-1 [&_.scope-fields]:grid-cols-2! max-[640px]:[&_.scope-fields]:grid-cols-1! [&_.api-key-scope-models]:col-span-full! [&_.api-key-scope-providers]:col-span-full! [&_.scope-rules]:min-h-[106px]!">
        <ApiKeyScopeFields
          {tr}
          providers={providerOptions}
          modelRulesId="edit-api-key-model-rules"
          bind:selectedProviderIds={editProviders}
          bind:modelRules={editModels}
        />
      </div>
      <label class="col-span-full flex min-w-0 m-0 items-center gap-[11px] rounded-xl border border-line bg-surface-sunken p-3.5 cursor-pointer max-[640px]:p-3 focus-within:outline-2 focus-within:outline-accent focus-within:outline-offset-2">
        <Input type="checkbox" bind:checked={editEnabled} />
        <span>
          <strong>{tr('Key enabled')}</strong>
        </span>
      </label>
    </div>
    <div class="modal-actions pt-3 border-t border-line">
      <Button class="page-action" type="button" data-close-modal>{tr('Cancel')}</Button>
      <Button class="page-action primary" type="submit" disabled={busy}>{tr('Save')}</Button>
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
      <Button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</Button>
    </div>
    <div class="modal-actions">
      <Button class="page-action" type="button" data-close-modal>{tr('Cancel')}</Button>
      <Button class="page-action danger" type="button" onclick={runConfirm} disabled={busy}>{confirmMode === 'delete' ? tr('Delete permanently') : tr('Disable key')}</Button>
    </div>
  </div>
</dialog>

{/if}

<style>
  /* The row's copy cluster is the detail trigger. It is a real button so the
     whole clickable area is keyboard reachable and announced; the status pill
     and action buttons beside it stay separate controls. */
  :global(.key-card-trigger) {
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
  :global(.key-card-trigger:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    border-radius: 8px;
  }
</style>
