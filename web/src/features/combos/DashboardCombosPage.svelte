<script lang="ts">
  import Button from "../../components/Button.svelte";
  import StatsCard from "../../components/StatsCard.svelte";
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import Input from '../../components/Input.svelte';
  import type { DashboardPage } from '../../lib/navigation';
  import type { Locale } from '../../lib/i18n';
  import type { Translate } from '../../lib/format';
  import { formatCount } from '../../lib/format';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import SelectOptions from '../../components/SelectOptions.svelte';
  import ComboReferencePicker from './ComboReferencePicker.svelte';
  import ComboTargetModelSelect from './ComboTargetModelSelect.svelte';
  import type { ComboProviderOption, GatewayCombo, RouteBreakdown } from '../../lib/types';
  import '../../styles/dashboard/pages/combos.css';

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

  function setTargetKind(target: TargetDraft, kind: TargetKind): void {
    if (target.kind === kind) return;
    target.kind = kind;
    // The two shapes store different fields; reset the ones the new kind ignores.
    target.providerId = '';
    target.model = '';
    target.comboId = '';
  }

  function addTarget(): void {
    cfTargets = [...cfTargets, newTargetDraft()];
  }

  function removeTarget(index: number): void {
    cfTargets = cfTargets.filter((_, i) => i !== index);
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
      <StatsCard icon="#i-code" value={combos.length} title={tr('Combos')} trend={`${enabledCombos} ${tr('enabled')}, ${combos.length - enabledCombos} ${tr('disabled')}`} actionLabel={tr('View all')} onAction={ () => (strategyFilter = 'all') } />
      <StatsCard icon="#i-layers" value={totalTargets} title={tr('Targets')} trend={tr('Across {count} providers', { count: providerOptions.length })} actionLabel={tr('View providers')} onAction={ () => onNavigate('providers') } />
      <StatsCard icon="#i-clipboard" value={routeBreakdownAvailable ? formatCount([...routeStats.values()].reduce((n, row) => n + row.requests, 0), locale) : '—'} title={tr('Requests Routed')} trend={routeWindowTrend} actionLabel={tr('View requests')} onAction={ () => onNavigate('requests') } />
      <StatsCard icon="#i-alert" value={routeBreakdownAvailable ? formatCount([...routeStats.values()].reduce((n, row) => n + row.failures, 0), locale) : '—'} title={tr('Failures')} trend={routeWindowTrend} actionLabel={tr('Inspect failures')} onAction={ () => onNavigate('requests') } />
    </section>

    <section class="course-card surface combo-board" aria-labelledby="combos-heading">
      <div class="card-title-row">
        <h2 id="combos-heading">{tr('Combos')}</h2>
      </div>
      <p class="card-subtitle">{combos.length} {tr('combos')} · {totalTargets} {tr('targets')} · {strategies} {tr('strategies')}</p>
      <div class="page-toolbar combo-toolbar">
        <Input class="combo-search" type="search" placeholder={tr('Search combos...')} aria-label={tr('Search combos')} bind:value={search} />
        <SelectOptions
          class="combo-select"
          ariaLabel={tr('Filter by strategy')}
          bind:value={strategyFilter}
          items={[{ label: tr('All strategies'), value: 'all' }, { label: tr('Priority'), value: 'priority' }, { label: tr('Round robin'), value: 'round_robin' }]}
        />
      </div>
      <div class="combo-list" aria-live="polite">
        {#each filteredCombos as combo (combo.id)}
          {@const usage = comboUsage(combo)}
          {@const pct = successPct(combo)}
          <article class="combo-card" data-status={combo.enabled !== false ? 'enabled' : 'disabled'} data-strategy={combo.strategy}>
            <div class="combo-head">
              <span class="course-icon"><svg class="icon"><use href={combo.strategy === 'round_robin' ? '#i-layers' : '#i-code'} /></svg></span>
              <div class="course-copy">
                <strong>{combo.name} <span class="prov-type">{strategyLabel(combo.strategy)}</span></strong>
                <span>
                  {#if usage}
                    {formatCount(usage.requests, locale)} {tr('requests')}{pct !== null ? ` · ${tr('{count}% success', { count: pct })}` : ''}
                  {:else}
                    {routeBreakdownAvailable && !routeBreakdownTruncated && !routeStatisticsUnavailable ? `${tr('no traffic yet')} · ${combo.targets.length} ${tr('targets')}` : `${combo.targets.length} ${tr('targets')}`}
                  {/if}
                </span>
              </div>
              <label class="switch" title={tr('Enable combo')}>
                <Input type="checkbox" class="combo-toggle" checked={combo.enabled !== false} onchange={() => toggleCombo(combo)} />
                <span class="switch-track"><span class="switch-thumb"></span></span>
              </label>
            </div>
            <div class="combo-targets">
              {#each combo.targets.slice(0, 4) as target, i (i)}
                <span class="combo-target">{targetLabel(target)}</span>
              {/each}
              {#if combo.targets.length > 5}
                <span class="combo-target combo-target-more" title={combo.targets.slice(4, -1).map((target) => targetLabel(target)).join(' · ')}>{tr('+{count} more', { count: combo.targets.length - 5 })}</span>
              {/if}
              {#if combo.targets.length > 1}
                <span class="combo-target fallback">{targetLabel(combo.targets[combo.targets.length - 1])}</span>
              {/if}
            </div>
            <div class="combo-foot">
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
  <form class="modal-card" onsubmit={(e) => { e.preventDefault(); void saveCombo(); }}>
    <div class="modal-heading">
      <div>
        <h2 id="combo-dialog-title">{editingCombo ? tr('Edit combo') : tr('New combo')}</h2>
        <p>{tr('Chain provider targets with a routing strategy.')}</p>
      </div>
      <Button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</Button>
    </div>
    <div class="modal-fields combo-fields">
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

    <div class="targets-editor">
      <div class="targets-head">
        <strong>{tr('Targets')}</strong>
        <span class="targets-hint">{tr('Tries each target in order on failure.')}</span>
      </div>
      <div class="target-rows">
        {#each cfTargets as target, i (target.key)}
          <div class="target-row" class:is-combo-mode={target.kind === 'combo'}>
            <SelectOptions
              class="target-kind"
              ariaLabel={tr('Target type')}
              placeholder={tr('Target type')}
              value={target.kind}
              items={[{ label: tr('Provider'), value: 'provider' }, { label: tr('Combo'), value: 'combo' }]}
              onValueChange={(value) => setTargetKind(target, value as TargetKind)}
            />
            {#if target.kind === 'combo'}
              <div class="combo-model-field target-combo-field">
                <ComboReferencePicker
                  combos={nestableCombos}
                  selectedComboId={target.comboId}
                  targetKey={target.key}
                  hideLabel
                  {tr}
                  onComboChange={(key, comboId) => {
                    const draft = cfTargets.find((t) => t.key === key);
                    if (draft) draft.comboId = comboId;
                  }}
                />
              </div>
            {:else}
              <SelectOptions
                class="target-provider"
                portalContainer={comboDialog}
                ariaLabel={tr('Target provider')}
                disabled={target.kind !== 'provider'}
                value={target.providerId}
                items={providerOptions.map((option) => ({ label: `${option.name}${option.enabled ? '' : ` (${tr('disabled')})`}`, value: option.id, disabled: !option.enabled && option.id !== target.providerId }))}
                onValueChange={(value) => {
                  if (!providerOptions.some((option) => option.id === value && option.enabled)) return;
                  target.providerId = value;
                  target.model = '';
                }}
              />
              <ComboTargetModelSelect
                providerId={target.kind === 'provider' ? target.providerId : ''}
                value={target.model}
                disabledPlaceholder={target.kind === '' ? tr('Choose a target type first.') : tr('Choose a provider first.')}
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
      <Button class="page-action" type="button" onclick={addTarget}>
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
      <Button class="page-action danger" type="button" onclick={confirmDelete} disabled={deleteBusy}>{deleteBusy ? tr('Deleting…') : tr('Delete combo')}</Button>
    </div>
  </div>
</dialog>

<style>
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

  .combo-retry-setting :global(.input-control) {
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
