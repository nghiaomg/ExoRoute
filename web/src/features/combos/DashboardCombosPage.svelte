<script lang="ts">
  import Button from "../../components/Button.svelte";
  import StatsCard from "../../components/StatsCard.svelte";
  import { onDestroy, onMount } from 'svelte';
  import { GripVertical } from '@lucide/svelte';
  import { api } from '../../lib/api';
  import Input from '../../components/Input.svelte';
  import type { DashboardPage } from '../../lib/navigation';
  import type { Locale } from '../../lib/i18n';
  import type { Translate } from '../../lib/format';
  import { formatCount } from '../../lib/format';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import SelectOptions from '../../components/SelectOptions.svelte';
  import ComboTargetModelSelect from './ComboTargetModelSelect.svelte';
  import ComboTargetSelect from './ComboTargetSelect.svelte';
  import type { ComboProviderOption, GatewayCombo, RouteBreakdown } from '../../lib/types';

  interface Props {
    tr: Translate;
    locale: Locale;
    actionRequest?: { id: number; page: DashboardPage } | null;
    onNavigate: (page: DashboardPage) => void;
    onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;
    onProviderCountChange: (count: number) => void;
  }

  let { tr: tr, locale, actionRequest, onNavigate, onConnectionChange, onProviderCountChange: _onProviderCountChange }: Props = $props();

  type TargetKind = '' | 'provider' | 'combo';

  interface TargetDraft {
    key: number;
    kind: TargetKind;
    providerId: string;
    model: string;
    comboId: string;
    enabled: boolean;
  }

  let combos = $state<GatewayCombo[]>([]);
  let providerOptions = $state<ComboProviderOption[]>([]);
  let routeStats = $state(new Map<string, RouteBreakdown>());
  let routeBreakdownTruncated = $state(false);
  let routeBreakdownAvailable = $state(false);
  let routeStatisticsUnavailable = $state(false);
  let routeBreakdownComplete = $state(false);
  let loading = $state(true);
  const routeWindowTrend = $derived(
    `${tr('Last 24 hours')} · ${tr('attempts')}${routeStatisticsUnavailable || !routeBreakdownAvailable || routeBreakdownTruncated || !routeBreakdownComplete ? ` · ${tr('Statistics are incomplete')}` : ''}`,
  );
  let errorMessage = $state('');
  let generation = 0;

  let search = $state('');
  let strategyFilter = $state('all');

  // Combo dialog
  let comboDialog = $state<HTMLDialogElement>();
  let editingCombo = $state<GatewayCombo | null>(null);
  let cfName = $state('');
  let cfStrategy = $state('priority');
  let cfRetryUntilSuccess = $state(false);
  let cfAlias = $state('');
  let cfTargets = $state<TargetDraft[]>([]);
  let nextTargetKey = 1;
  let cfBusy = $state(false);
  let cfError = $state('');

  // Delete dialog
  let deleteDialog = $state<HTMLDialogElement>();
  let deleteTarget = $state<GatewayCombo | null>(null);
  let deleteBusy = $state(false);

  const providerNameById = $derived(new Map(providerOptions.map((p) => [p.id, p.name])));
  // A combo cannot nest itself; deeper cycles are rejected by the backend.
  const nestableCombos = $derived(combos.filter((c) => c.id !== editingCombo?.id));

  const filteredCombos = $derived.by(() => {
    const q = search.trim().toLowerCase();
    return combos.filter((c) => {
      if (strategyFilter !== 'all' && c.strategy !== strategyFilter) return false;
      if (!q) return true;
      return c.name.toLowerCase().includes(q) || c.targets.some((t) => t.model.toLowerCase().includes(q));
    });
  });

  const enabledCombos = $derived(combos.filter((c) => c.enabled !== false).length);
  const totalTargets = $derived(combos.reduce((n, c) => n + c.targets.length, 0));
  const strategies = $derived(new Set(combos.map((c) => c.strategy)).size);

  function strategyLabel(strategy: string): string {
    return strategy === 'round_robin' ? 'round-robin' : strategy;
  }

  function comboUsage(combo: GatewayCombo): RouteBreakdown | null {
    return routeStats.get(combo.name) ?? null;
  }

  function successPct(combo: GatewayCombo): number | null {
    const usage = comboUsage(combo);
    if (!usage || !usage.requests) return null;
    return Math.round(usage.success_rate);
  }

  async function load(): Promise<void> {
    const gen = ++generation;
    loading = true;
    errorMessage = '';
    onConnectionChange('loading');
    try {
      const [comboRows, options, statistics] = await Promise.all([
        api.combos(),
        loadAllProviderOptions(),
        api.statistics('1d').catch(() => null),
      ]);
      if (gen !== generation) return;
      combos = comboRows;
      providerOptions = options;
      const map = new Map<string, RouteBreakdown>();
      for (const row of statistics?.route_breakdown ?? []) map.set(row.route_alias, row);
      routeStats = map;
      routeBreakdownTruncated = statistics?.route_breakdown_truncated ?? false;
      routeBreakdownAvailable = statistics !== null && !statistics.route_breakdown_incomplete;
      routeStatisticsUnavailable = statistics === null;
      routeBreakdownComplete = statistics !== null && statistics.complete && statistics.aggregation_current;
      onConnectionChange('loaded');
    } catch (error) {
      if (gen !== generation) return;
      errorMessage = error instanceof Error ? error.message : tr('Could not load combos.');
      onConnectionChange('error');
    } finally {
      if (gen === generation) loading = false;
    }
  }

  async function loadAllProviderOptions(): Promise<ComboProviderOption[]> {
    const all: ComboProviderOption[] = [];
    let cursor: string | undefined;
    for (let i = 0; i < 20; i++) {
      const page = await api.comboProviderOptions(cursor);
      all.push(...page.providers);
      if (!page.next_cursor) break;
      cursor = page.next_cursor;
    }
    return all;
  }

  function openComboDialog(combo: GatewayCombo | null): void {
    editingCombo = combo;
    cfError = '';
    nextTargetKey = 1;
    if (combo) {
      cfName = combo.name;
      cfStrategy = combo.strategy;
      cfRetryUntilSuccess = combo.retry_until_success === true;
      cfAlias = combo.name;
      cfTargets = combo.targets.map((t) => ({
        key: nextTargetKey++,
        kind: t.combo_id ? 'combo' : 'provider',
        providerId: t.provider_id,
        model: t.model,
        comboId: t.combo_id ?? '',
        enabled: t.enabled,
      }));
    } else {
      cfName = '';
      cfStrategy = 'priority';
      cfRetryUntilSuccess = false;
      cfAlias = '';
      cfTargets = [newTargetDraft()];
    }
    comboDialog?.showModal();
  }

  function newTargetDraft(): TargetDraft {
    return { key: nextTargetKey++, kind: '', providerId: '', model: '', comboId: '', enabled: true };
  }

  /** The unified target picker decides the kind: a combo entry makes the row
   * a nested-combo target, anything else is a provider target. The fields the
   * kind ignores reset so a row never carries both forms. Re-selecting the
   * row's current provider keeps its model. */
  function applyTargetSelection(
    key: number,
    kind: 'provider' | 'combo',
    providerId: string,
    comboId: string,
  ): void {
    const draft = cfTargets.find((target) => target.key === key);
    if (!draft) return;
    if (draft.kind === kind && kind === 'provider' && draft.providerId === providerId) return;
    draft.kind = kind;
    draft.providerId = providerId;
    draft.model = '';
    draft.comboId = comboId;
  }

  function addTarget(): void {
    cfTargets = [...cfTargets, newTargetDraft()];
  }

  function removeTarget(index: number): void {
    cfTargets = cfTargets.filter((_, i) => i !== index);
  }

  function reorderTarget(from: number, to: number): void {
    if (from === to || from < 0 || to < 0 || from >= cfTargets.length || to >= cfTargets.length) return;
    const next = [...cfTargets];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    cfTargets = next;
  }

  function moveTarget(index: number, direction: -1 | 1): void {
    reorderTarget(index, index + direction);
  }

  // Drag-and-drop reordering. The row is the draggable (so the drag image is
  // the whole row, and the browser retargets dragstart to it), but a drag may
  // only start after the pointer went down on the handle — the arm flag — so
  // text selection inside the pickers keeps working. Rows reorder live under
  // the pointer and every row keeps its stable key. The handle also reorders
  // with the arrow keys so the same capability exists without a pointer.
  let dragKey = $state<number | null>(null);
  let dragArmedKey = $state<number | null>(null);

  function handleDragStart(event: DragEvent, key: number): void {
    if (dragArmedKey !== key) {
      event.preventDefault();
      return;
    }
    dragArmedKey = null;
    dragKey = key;
    if (event.dataTransfer) {
      event.dataTransfer.setData('text/plain', String(key));
      event.dataTransfer.effectAllowed = 'move';
    }
  }

  function handleDragOver(event: DragEvent, index: number): void {
    if (dragKey === null) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
    const from = cfTargets.findIndex((target) => target.key === dragKey);
    if (from >= 0 && from !== index) reorderTarget(from, index);
  }

  function handleDragEnd(): void {
    dragKey = null;
    dragArmedKey = null;
  }

  function handleDragKeydown(event: KeyboardEvent, index: number): void {
    if (event.key === 'ArrowUp') {
      event.preventDefault();
      moveTarget(index, -1);
    } else if (event.key === 'ArrowDown') {
      event.preventDefault();
      moveTarget(index, 1);
    }
  }

  async function saveCombo(): Promise<void> {
    const name = cfName.trim();
    if (!name || cfBusy) return;
    const targets = cfTargets
      .filter((t) => (t.kind === 'provider' && t.providerId && t.model.trim()) || (t.kind === 'combo' && t.comboId))
      .map((t, index) =>
        t.kind === 'combo'
          ? { combo_id: t.comboId, provider_id: '', model: '', priority: index + 1, enabled: t.enabled }
          : { provider_id: t.providerId, model: t.model.trim(), priority: index + 1, enabled: t.enabled },
      );
    if (!targets.length) {
      cfError = tr('Add at least one target with a provider and model, or a nested combo.');
      return;
    }
    cfBusy = true;
    cfError = '';
    try {
      if (editingCombo) {
        await api.updateCombo(editingCombo.id, { name, strategy: cfStrategy, retry_until_success: cfRetryUntilSuccess, enabled: editingCombo.enabled !== false, targets });
        showDashboardToast(tr('Combo "{name}" updated', { name }));
      } else {
        await api.createCombo({
          name,
          strategy: cfStrategy,
          retry_until_success: cfRetryUntilSuccess,
          accepted_protocols: ['chat_completions', 'responses', 'messages'],
          enabled: true,
          targets,
        });
        showDashboardToast(tr('Combo "{name}" created', { name }));
      }
      comboDialog?.close();
      await load();
    } catch (error) {
      cfError = error instanceof Error ? error.message : tr('Could not save combo.');
    } finally {
      cfBusy = false;
    }
  }

  async function toggleCombo(combo: GatewayCombo): Promise<void> {
    const next = !(combo.enabled !== false);
    combos = combos.map((c) => (c.id === combo.id ? { ...c, enabled: next } : c));
    try {
      await api.updateCombo(combo.id, { enabled: next });
      showDashboardToast(next ? tr('{combo} enabled', { combo: combo.name }) : tr('{combo} disabled', { combo: combo.name }));
    } catch (error) {
      combos = combos.map((c) => (c.id === combo.id ? { ...c, enabled: !next } : c));
      showDashboardToast(error instanceof Error ? error.message : tr('Update failed'));
    }
  }

  async function duplicateCombo(combo: GatewayCombo): Promise<void> {
    try {
      await api.createCombo({
        name: `${combo.name}-copy`,
        strategy: combo.strategy,
        retry_until_success: combo.retry_until_success === true,
        accepted_protocols: combo.accepted_protocols?.length ? combo.accepted_protocols : ['chat_completions', 'responses', 'messages'],
        enabled: true,
        targets: combo.targets.map((t) => ({ ...t })),
      });
      showDashboardToast(tr('Duplicated as "{name}"', { name: `${combo.name}-copy` }));
      await load();
    } catch (error) {
      showDashboardToast(error instanceof Error ? error.message : tr('Could not duplicate combo'));
    }
  }

  function openDelete(combo: GatewayCombo): void {
    deleteTarget = combo;
    deleteDialog?.showModal();
  }

  async function confirmDelete(): Promise<void> {
    if (!deleteTarget || deleteBusy) return;
    deleteBusy = true;
    try {
      await api.deleteCombo(deleteTarget.id);
      combos = combos.filter((c) => c.id !== deleteTarget?.id);
      showDashboardToast(tr('Combo "{name}" deleted', { name: deleteTarget.name }));
      deleteDialog?.close();
    } catch (error) {
      showDashboardToast(error instanceof Error ? error.message : tr('Could not delete combo'));
    } finally {
      deleteBusy = false;
    }
  }

  function targetLabel(target: { provider_id: string; model: string; combo_id?: string | null }): string {
    if (target.combo_id) return tr('combo · {id}', { id: target.combo_id });
    const provider = providerNameById.get(target.provider_id) ?? target.provider_id;
    return `${provider} · ${target.model || '—'}`;
  }

  $effect(() => {
    if (actionRequest && actionRequest.page === 'combos') openComboDialog(null);
  });

  onMount(() => { void load(); });
  onDestroy(() => { generation += 1; });
</script>

<!-- A plain click on the handle disarms the row-drag gate. -->
<svelte:window onmouseup={() => (dragArmedKey = null)} />

<div class="page-heading">
  <div class="heading-copy">
    <h1>{tr('Combos')}</h1>
    <p>{tr('Route models across providers with fallback chains and strategies.')}</p>
  </div>
  <div class="heading-actions">
    <Button class="action-button" type="button" onclick={() => showDashboardToast(tr('Import is available in the legacy combo editor'))}>
      <span class="button-icon"><svg class="icon"><use href="#i-import" /></svg></span>
      <span class="button-label">{tr('Import')}</span>
    </Button>
    <Button class="action-button primary" type="button" onclick={() => openComboDialog(null)}>
      <span class="button-icon"><svg class="icon"><use href="#i-plus" /></svg></span>
      <span class="button-label">{tr('New combo')}</span>
    </Button>
  </div>
</div>

{#if errorMessage}
  <section class="course-card surface" aria-label={tr('Load error')}>
    <div class="card-title-row"><h2>{tr('Could not load combos')}</h2></div>
    <p class="card-subtitle">{errorMessage}</p>
    <div class="modal-actions" style="padding-top:14px">
      <Button class="page-action primary" type="button" onclick={load}>{tr('Retry')}</Button>
    </div>
  </section>
{:else if loading}
  <section class="stats-grid" aria-label={tr('Loading')}>
    {#each [0, 1, 2, 3] as i (i)}<StatsCard skeleton />{/each}
  </section>
{:else}
  <div class="page-surface">
    <section class="stats-grid" aria-label={tr('Combo overview')}>
      <StatsCard index={1} eyebrow={tr('Routing')} badgeTone="chip" badge={String(enabledCombos)} value={combos.length} title={tr('Combos')} trend={`${enabledCombos} ${tr('enabled')}, ${combos.length - enabledCombos} ${tr('disabled')}`} />
      <StatsCard index={2} eyebrow={tr('Routing')} value={totalTargets} title={tr('Targets')} trend={tr('Across {count} providers', { count: providerOptions.length })} />
      <StatsCard index={3} eyebrow={tr('Traffic')} value={routeBreakdownAvailable ? formatCount([...routeStats.values()].reduce((n, row) => n + row.requests, 0), locale) : '—'} title={tr('Requests Routed')} trend={routeWindowTrend} />
      <StatsCard index={4} eyebrow={tr('Reliability')} value={routeBreakdownAvailable ? formatCount([...routeStats.values()].reduce((n, row) => n + row.failures, 0), locale) : '—'} title={tr('Failures')} trend={routeWindowTrend} />
    </section>

    <section class="course-card surface flex flex-col" aria-labelledby="combos-heading">
      <div class="card-title-row">
        <h2 id="combos-heading">{tr('Combos')}</h2>
      </div>
      <p class="card-subtitle">{combos.length} {tr('combos')} · {totalTargets} {tr('targets')} · {strategies} {tr('strategies')}</p>
      <div class="page-toolbar mt-3 items-center">
        <Input class="w-[min(280px,100%)] flex-[1_1_200px]" type="search" placeholder={tr('Search combos...')} aria-label={tr('Search combos')} bind:value={search} />
        <SelectOptions
          class="combo-select"
          ariaLabel={tr('Filter by strategy')}
          bind:value={strategyFilter}
          items={[{ label: tr('All strategies'), value: 'all' }, { label: tr('Priority'), value: 'priority' }, { label: tr('Round robin'), value: 'round_robin' }]}
        />
      </div>
      <div class="mt-3.5 grid grid-cols-[repeat(3,minmax(0,1fr))] gap-2.5 max-[1100px]:grid-cols-2 max-nav:grid-cols-1 [&>.empty-state]:col-span-full [&>.empty-state]:w-full" aria-live="polite">
        {#each filteredCombos as combo (combo.id)}
          {@const usage = comboUsage(combo)}
          {@const pct = successPct(combo)}
          <article class="flex min-w-0 flex-col rounded-[14px] border border-line bg-surface p-4 transition-[border-color,box-shadow] duration-200 hover:border-text-dim/50 hover:shadow-sm data-[status=disabled]:opacity-62" data-status={combo.enabled !== false ? 'enabled' : 'disabled'} data-strategy={combo.strategy}>
            <div class="flex min-w-0 items-center gap-[11px]">
              <span class="course-icon shrink-0"><svg class="icon"><use href={combo.strategy === 'round_robin' ? '#i-layers' : '#i-code'} /></svg></span>
              <div class="min-w-0 flex-1">
                <div class="flex min-w-0 items-center gap-2">
                  <strong class="truncate text-sm font-semibold text-ink">{combo.name}</strong>
                  <span class="shrink-0 rounded-full bg-surface-sunken px-2 py-0.5 text-[11px] font-medium leading-none text-muted">{strategyLabel(combo.strategy)}</span>
                </div>
                <span class="mt-0.5 block truncate text-xs text-muted">
                  {#if usage}
                    {formatCount(usage.requests, locale)} {tr('requests')}{pct !== null ? ` · ${tr('{count}% success', { count: pct })}` : ''}
                  {:else}
                    {routeBreakdownAvailable && !routeBreakdownTruncated && !routeStatisticsUnavailable ? `${tr('no traffic yet')} · ${combo.targets.length} ${tr('targets')}` : `${combo.targets.length} ${tr('targets')}`}
                  {/if}
                </span>
              </div>
              <label class="switch shrink-0" title={tr('Enable combo')}>
                <Input type="checkbox" class="combo-toggle" checked={combo.enabled !== false} onchange={() => toggleCombo(combo)} />
                <span class="switch-track"><span class="switch-thumb"></span></span>
              </label>
            </div>
            <ul class="mt-3 flex flex-col gap-1" aria-label={tr('Targets')}>
              {#each combo.targets.slice(0, 4) as target, i (i)}
                <li class="flex min-w-0 items-center gap-1.5 text-xs">
                  {#if target.combo_id}
                    <span class="shrink-0 rounded-[5px] bg-db-accent-tint px-1.5 py-px text-[10px] font-medium leading-[1.4] text-accent-strong">{tr('combo')}</span>
                    <span class="truncate font-medium text-ink">{target.combo_id}</span>
                  {:else}
                    <span class="truncate text-muted">{targetLabel(target)}</span>
                  {/if}
                </li>
              {/each}
              {#if combo.targets.length > 4}
                <li>
                  <span
                    class="inline-flex items-center rounded-full border border-line px-2 py-0.5 text-[11px] font-medium leading-none text-muted"
                    title={combo.targets.slice(4).map((target) => targetLabel(target)).join(' · ')}
                  >{tr('+{count} more', { count: combo.targets.length - 4 })}</span>
                </li>
              {/if}
            </ul>
            <div class="mt-auto flex items-center justify-between gap-2.5 border-t border-line pt-[11px]">
              <div class="course-actions">
                <Button class="icon-action" type="button" title={tr('Edit combo')} onclick={() => openComboDialog(combo)}><svg class="icon"><use href="#i-edit" /></svg></Button>
                <Button class="icon-action" type="button" title={tr('Duplicate')} onclick={() => duplicateCombo(combo)}><svg class="icon"><use href="#i-copy" /></svg></Button>
                <Button class="icon-action danger" type="button" title={tr('Delete combo')} onclick={() => openDelete(combo)}><svg class="icon"><use href="#i-trash" /></svg></Button>
              </div>
            </div>
          </article>
        {:else}
          <div class="empty-state">{tr('No combos match the current filters.')}</div>
        {/each}
      </div>
    </section>
  </div>
{/if}

<!-- Create / edit combo -->
<dialog class="modal modal-wide" bind:this={comboDialog} aria-labelledby="combo-dialog-title" onclose={() => { cfError = ''; }}>
  <form class="modal-card combo-dialog" onsubmit={(e) => { e.preventDefault(); void saveCombo(); }}>
    <div class="modal-heading">
      <div>
        <h2 id="combo-dialog-title">{editingCombo ? tr('Edit combo') : tr('New combo')}</h2>
        <p>{tr('Chain provider targets with a routing strategy.')}</p>
      </div>
      <Button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</Button>
    </div>
    <div class="modal-fields grid-cols-2! max-nav:grid-cols-1!">
      <label class="field prov-field-wide">
        {tr('Combo name')}
        <Input bind:value={cfName} maxlength={60} required placeholder={tr('e.g. smart-default')} />
      </label>
      <label class="field">
        {tr('Strategy')}
        <SelectOptions
          ariaLabel={tr('Strategy')}
          bind:value={cfStrategy}
          items={[{ label: tr('Priority (first healthy wins)'), value: 'priority' }, { label: tr('Round robin'), value: 'round_robin' }]}
        />
      </label>
      <label class="field">
        {tr('Served model alias')}
        <Input bind:value={cfAlias} maxlength={120} placeholder={tr('e.g. auto or fast/*')} disabled />
      </label>
      <label class="combo-retry-setting">
        <Input type="checkbox" bind:checked={cfRetryUntilSuccess} />
        <span>
          <strong>{tr('Retry until success')}</strong>
          <small>{tr('After all targets fail, restart the combo indefinitely. A provider may have processed an earlier attempt. HTTP 400 and 413 stop retries.')}</small>
        </span>
      </label>
    </div>
    <p class="form-note">{tr('The combo name is the model alias clients request.')}</p>

    <div class="grid gap-2.5 rounded-[13px] border border-line bg-db-tint p-3.5">
      <div class="flex items-baseline gap-2.5">
        <strong class="text-xs font-semibold text-ink">{tr('Targets')}</strong>
        <span class="text-muted text-xs">{tr('Tries each target in order on failure.')}</span>
      </div>
      <div class="combo-target-rows grid gap-2 max-nav:overflow-x-auto" role="list">
        {#each cfTargets as target, i (target.key)}
          <div
            role="listitem"
            class="combo-target-row {target.kind === 'combo'
              ? 'grid-cols-[26px_minmax(240px,1fr)_auto_32px]'
              : 'grid-cols-[26px_minmax(200px,0.8fr)_minmax(220px,1fr)_auto_32px]'} grid gap-2 items-center min-w-0 max-nav:min-w-[680px] [&>div]:min-w-0"
            class:is-dragging={dragKey === target.key}
            draggable="true"
            ondragstart={(event) => handleDragStart(event, target.key)}
            ondragover={(event) => handleDragOver(event, i)}
            ondrop={(event) => event.preventDefault()}
            ondragend={handleDragEnd}
          >
            <button
              type="button"
              class="target-drag-handle"
              aria-label={tr('Reorder targets')}
              title={tr('Drag to reorder targets')}
              onmousedown={() => (dragArmedKey = target.key)}
              onkeydown={(event) => handleDragKeydown(event, i)}
            >
              <GripVertical size={13} />
            </button>
            <ComboTargetSelect
              providers={providerOptions}
              combos={nestableCombos}
              kind={target.kind}
              providerId={target.providerId}
              comboId={target.comboId}
              targetKey={target.key}
              {tr}
              onTargetChange={applyTargetSelection}
            />
            {#if target.kind !== 'combo'}
              <ComboTargetModelSelect
                providerId={target.kind === 'provider' ? target.providerId : ''}
                value={target.model}
                disabledPlaceholder={target.kind === '' ? tr('Choose a target first.') : tr('Choose a provider first.')}
                portalContainer={comboDialog}
                {tr}
                onChange={(model) => (target.model = model)}
              />
            {/if}
            <label class="switch" title={tr('Target enabled')}>
              <Input type="checkbox" bind:checked={target.enabled} />
              <span class="switch-track"><span class="switch-thumb"></span></span>
            </label>
            <Button class="icon-action danger" type="button" title={tr('Remove target')} onclick={() => removeTarget(i)}><svg class="icon"><use href="#i-trash" /></svg></Button>
          </div>
        {/each}
      </div>
      <Button class="page-action w-full justify-center" type="button" onclick={addTarget}>
        <svg class="icon"><use href="#i-plus" /></svg>
        {tr('Add target')}
      </Button>
    </div>

    {#if cfError}<p class="form-error" role="alert">{cfError}</p>{/if}
    <div class="modal-actions">
      <Button class="page-action" type="button" data-close-modal>{tr('Cancel')}</Button>
      <Button class="page-action primary" type="submit" disabled={cfBusy}>{cfBusy ? tr('Saving…') : tr('Save combo')}</Button>
    </div>
  </form>
</dialog>

<!-- Delete confirm -->
<dialog class="modal" bind:this={deleteDialog} aria-labelledby="combo-delete-title">
  <div class="modal-card">
    <div class="modal-heading">
      <div>
        <h2 id="combo-delete-title">{tr('Delete combo?')}</h2>
        <p>{tr('Requests routed through {name} will fail until a replacement exists.', { name: deleteTarget?.name ?? tr('this combo') })}</p>
      </div>
      <Button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</Button>
    </div>
    <div class="modal-actions">
      <Button class="page-action" type="button" data-close-modal>{tr('Cancel')}</Button>
      <Button class="page-action danger border-db-accent-line! text-db-danger! hover:bg-db-accent-tint!" type="button" onclick={confirmDelete} disabled={deleteBusy}>{deleteBusy ? tr('Deleting…') : tr('Delete combo')}</Button>
    </div>
  </div>
</dialog>

<style>
  .target-drag-handle {
    display: grid;
    place-items: center;
    width: 26px;
    height: 32px;
    padding: 0;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--muted);
    cursor: grab;
  }

  .target-drag-handle:hover {
    color: var(--ink);
  }

  .target-drag-handle:active {
    cursor: grabbing;
  }

  .target-drag-handle:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .combo-target-row.is-dragging {
    opacity: 0.45;
  }

  .combo-retry-setting {
    grid-column: 1 / -1;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px;
    border: 1px solid #e8edef;
    border-radius: 10px;
    background: #fbfcfc;
    cursor: pointer;
  }

  .combo-retry-setting :global(input[type="checkbox"]) {
    margin: 2px 0 0;
    flex: 0 0 auto;
  }

  .combo-retry-setting span {
    display: grid;
    gap: 4px;
  }

  .combo-retry-setting strong {
    color: #303b48;
    font-size: var(--type-body);
  }

  .combo-retry-setting small {
    color: var(--muted);
    line-height: 1.45;
  }
</style>
