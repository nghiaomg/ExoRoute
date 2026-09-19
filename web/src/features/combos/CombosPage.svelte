<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Layers3, ListOrdered, LoaderCircle, Plus, RefreshCw, ShieldCheck } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import GatewayError from '../../components/GatewayError.svelte';
  import InlineLoading from '../../components/InlineLoading.svelte';
  import PageHeading from '../../components/PageHeading.svelte';
  import PageSearch from '../../components/PageSearch.svelte';
  import { api } from '../../lib/api';
  import type { FeatureActionRequest } from '../../lib/navigation';
  import type { Translate } from '../../lib/format';
  import { localizedError } from '../../lib/errors';
  import type { ComboProviderOption, GatewayCombo } from '../../lib/types';
  import ComboCard from './ComboCard.svelte';
  import ComboEditorDialog from './ComboEditorDialog.svelte';

  export let tr: Translate;
  export let actionRequest: FeatureActionRequest | null = null;
  export let onNavigate: (page: 'providers') => void;
  export let onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;
  export let onProviderCountChange: (count: number) => void;

  let combos: GatewayCombo[] = [];
  let comboToDelete: GatewayCombo | null = null;
  let providers: ComboProviderOption[] = [];
  let providersLoaded = false;
  let query = '';
  let loading = true;
  let errorMessage = '';
  let deletingId = '';
  let editorOpen = false;
  let editingCombo: GatewayCombo | null = null;
  let generation = 0;
  let handledActionId = 0;
  let providerOptionsController: AbortController | undefined;

  $: providerNames = Object.fromEntries(providers.map((provider) => [provider.id, provider.name]));
  $: filteredCombos = combos.filter((combo) => {
    const q = query.toLowerCase().trim();
    if (!q) return true;
    if (`${combo.name} ${combo.strategy}`.toLowerCase().includes(q)) return true;
    return combo.targets.some((t) => (t.model?.toLowerCase().includes(q) || (providerNames[t.provider_id] ?? t.provider_id).toLowerCase().includes(q)));
  });
  $: totalCombos = combos.length;
  $: totalTargets = combos.reduce((sum, c) => sum + c.targets.length, 0);
  $: fallbackProtectedCombos = combos.filter((c) => c.targets.length >= 2).length;
  $: roundRobinCount = combos.filter((c) => c.strategy === 'round_robin').length;
  $: if (actionRequest?.page === 'combos' && actionRequest.id !== handledActionId) {
    handledActionId = actionRequest.id;
    editingCombo = null;
    editorOpen = true;
  }

  async function load(): Promise<void> {
    const requestGeneration = ++generation;
    providerOptionsController?.abort();
    const optionsController = providersLoaded ? undefined : new AbortController();
    providerOptionsController = optionsController;
    loading = true;
    errorMessage = '';
    onConnectionChange('loading');
    try {
      const [comboRows, providerRows] = await Promise.all([
        api.combos(),
        providersLoaded ? Promise.resolve(providers) : loadProviderOptions(optionsController?.signal),
      ]);
      if (requestGeneration !== generation) return;
      combos = comboRows;
      providers = providerRows;
      providersLoaded = true;
      onProviderCountChange(providerRows.length);
      onConnectionChange('loaded');
    } catch (error) {
      if (requestGeneration !== generation) return;
      errorMessage = localizedError(error, 'Something went wrong while loading this page.', tr);
      onConnectionChange('error');
    } finally {
      if (requestGeneration === generation) loading = false;
      if (providerOptionsController === optionsController) providerOptionsController = undefined;
    }
  }

  async function loadProviderOptions(signal?: AbortSignal): Promise<ComboProviderOption[]> {
    const allProviders: ComboProviderOption[] = [];
    let cursor: string | undefined;
    do {
      const page = await api.comboProviderOptions(cursor, signal);
      allProviders.push(...page.providers);
      cursor = page.next_cursor ?? undefined;
    } while (cursor);
    return allProviders.sort((left, right) => left.name.localeCompare(right.name) || left.id.localeCompare(right.id));
  }

  function removeCombo(combo: GatewayCombo): void {
    comboToDelete = combo;
  }

  async function confirmRemoveCombo(): Promise<void> {
    if (!comboToDelete) return;
    const combo = comboToDelete;
    deletingId = combo.id;
    try {
      await api.deleteCombo(combo.id);
      combos = combos.filter((item) => item.id !== combo.id);
      comboToDelete = null;
    } catch (error) {
      errorMessage = localizedError(error, 'Could not delete this combo.', tr);
    } finally {
      deletingId = '';
    }
  }

  function openCreate(): void {
    errorMessage = '';
    editingCombo = null;
    editorOpen = true;
  }

  function openEdit(combo: GatewayCombo): void {
    errorMessage = '';
    editingCombo = combo;
    editorOpen = true;
  }

  onMount(() => { void load(); });
  onDestroy(() => {
    generation += 1;
    providerOptionsController?.abort();
  });
</script>

<div class="combos-view">
  <PageHeading title={tr('Combos')} subtitle={tr('Choose which providers serve a model and how requests fall back.')} {tr}>
    <PageSearch bind:value={query} pageLabel={tr('Combos')} {tr} />
    <button class="primary-button" onclick={openCreate}><Plus size={17} />{tr('Create combo')}</button>
  </PageHeading>

  {#if !loading && combos.length > 0}
    <div class="combos-summary-strip">
      <div class="summary-metric-card">
        <span class="metric-icon mint"><Layers3 size={15} /></span>
        <div class="metric-body">
          <span class="metric-count">{totalCombos}</span>
          <span class="metric-name">{tr('Active Combos')}</span>
        </div>
      </div>
      <div class="summary-metric-card">
        <span class="metric-icon orange"><ShieldCheck size={15} /></span>
        <div class="metric-body">
          <span class="metric-count">{totalTargets}</span>
          <span class="metric-name">{tr('Total Targets')}</span>
        </div>
      </div>
      <div class="summary-metric-card">
        <span class="metric-icon blue"><ShieldCheck size={15} /></span>
        <div class="metric-body">
          <span class="metric-count">{fallbackProtectedCombos}</span>
          <span class="metric-name">{tr('Fallback Protected')}</span>
        </div>
      </div>
      <div class="summary-metric-card">
        <span class="metric-icon purple">
          {#if roundRobinCount > 0}<RefreshCw size={14} />{:else}<ListOrdered size={14} />{/if}
        </span>
        <div class="metric-body">
          <span class="metric-count">{totalCombos - roundRobinCount} : {roundRobinCount}</span>
          <span class="metric-name">{tr('Priority')} / {tr('Round robin')}</span>
        </div>
      </div>
    </div>
  {/if}

  {#if errorMessage}<GatewayError message={errorMessage} {tr} onRetry={load} />
  {:else if loading}<InlineLoading label={'Loading {page}…'} {tr} vars={{ page: tr('Combos').toLowerCase() }} />
  {:else if filteredCombos.length}
    <div class="combo-list">{#each filteredCombos as combo (combo.id)}<ComboCard {combo} {providerNames} {tr} deleting={deletingId === combo.id} onDelete={removeCombo} onEdit={openEdit} />{/each}</div>
  {:else if combos.length}
    <EmptyState icon="search" title={tr('No matching combos')} description={tr('Try a different search, or clear the filter.')} />
  {:else}
    <EmptyState icon="combos" title={tr('No combos configured')} description={tr('Create a combo to map a model name to one or more provider targets.')} action={tr('Create your first combo')} onclick={openCreate} />
  {/if}

  <ComboEditorDialog open={editorOpen} comboToEdit={editingCombo} {providers} {tr} onClose={() => { editorOpen = false; editingCombo = null; }} onSaved={load} {onNavigate} />

  <ArkDialog
    open={comboToDelete !== null}
    role="alertdialog"
    closeLabel={tr('Close dialog')}
    title={tr('Are you absolutely sure?')}
    kicker={tr('EXOROUTE CONTROL PLANE')}
    onClose={() => { comboToDelete = null; }}
  >
    <div class="modal-form">
      <p class="modal-description">
        {tr('Delete combo “{name}”? This cannot be undone.', { name: comboToDelete?.name ?? '' })}
      </p>
      <div class="modal-actions" style="margin-top: 16px;">
        <button type="button" class="secondary-button" disabled={Boolean(deletingId)} onclick={() => { comboToDelete = null; }}>
          {tr('Cancel')}
        </button>
        <button
          type="button"
          class="primary-button danger"
          disabled={Boolean(deletingId)}
          onclick={confirmRemoveCombo}
        >
          {#if deletingId}<LoaderCircle size={14} class="spin" />{tr('Deleting…')}{:else}{tr('Delete combo')}{/if}
        </button>
      </div>
    </div>
  </ArkDialog>
</div>
