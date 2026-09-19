<script lang="ts">
  import { ArrowDown, Layers3, ListOrdered, LoaderCircle, Plus, RefreshCw, Save } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import ArkField from '../../components/ArkField.svelte';
  import ArkSelect from '../../components/ArkSelect.svelte';
  import ComboTargetEditor from './ComboTargetEditor.svelte';
  import { MAX_COMBO_TARGETS, type ComboTargetActions, type ComboTargetDraft, type ComboTargetView } from './combo.types';
  import { api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { ComboProviderOption, GatewayCombo } from '../../lib/types';

  export let open = false;
  export let comboToEdit: GatewayCombo | null = null;
  export let tr: Translate;
  export let onClose: () => void;
  export let onSaved: () => void;
  export let onNavigate: (page: 'providers') => void;
  export let providers: ComboProviderOption[];

  let comboName = '';
  let strategy = 'priority';
  let comboTargets: ComboTargetDraft[] = [];
  let nextTargetKey = 1;
  let formError = '';
  let saving = false;
  let activeModelTargetKey: number | null = null;
  let pendingInitialProviderSelection = false;

  $: enabledProviderIds = new Set(providers.filter((provider) => provider.enabled).map((provider) => provider.id));
  $: hasEnabledProviders = enabledProviderIds.size > 0;
  $: validTargets = comboTargets.length > 0 && comboTargets.length <= MAX_COMBO_TARGETS && comboTargets.every((target) =>
    enabledProviderIds.has(target.providerId) && Boolean(target.model));
  $: if (open && pendingInitialProviderSelection) {
    const firstProviderId = providers.find((provider) => provider.enabled)?.id;
    if (firstProviderId) {
      comboTargets = comboTargets.map((target) => target.key === 1 && !target.providerId
        ? { ...target, providerId: firstProviderId }
        : target);
      pendingInitialProviderSelection = false;
    }
  }
  $: targetActions = ({
    onProviderChange: selectProvider,
    onModelChange: selectModel,
    onActivateModelPicker: activateModelPicker,
    onDeactivateModelPicker: deactivateModelPicker,
    onMove: moveTarget,
    onRemove: removeTarget,
    onOpenProviders: openProviders,
  }) satisfies ComboTargetActions;

  let wasOpen = false;
  $: if (open && !wasOpen) {
    wasOpen = true;
    initForm();
  } else if (!open && wasOpen) {
    wasOpen = false;
    activeModelTargetKey = null;
  }

  function initForm(): void {
    formError = '';
    activeModelTargetKey = null;
    if (comboToEdit) {
      comboName = comboToEdit.name;
      strategy = comboToEdit.strategy;
      nextTargetKey = comboToEdit.targets.length + 1;
      comboTargets = comboToEdit.targets.map((t, idx) => ({
        key: idx + 1,
        providerId: t.provider_id,
        model: t.model,
      }));
      pendingInitialProviderSelection = false;
    } else {
      comboName = '';
      strategy = 'priority';
      nextTargetKey = 2;
      const firstProviderId = providers.find((provider) => provider.enabled)?.id ?? '';
      pendingInitialProviderSelection = !firstProviderId;
      comboTargets = [{ key: 1, providerId: firstProviderId, model: '' }];
    }
  }

  function selectProvider(targetKey: number, providerId: string): void {
    if (targetKey === 1) pendingInitialProviderSelection = false;
    const existing = comboTargets.find((target) => target.key === targetKey);
    if (existing && existing.providerId === providerId) {
      return;
    }
    comboTargets = comboTargets.map((target) => target.key === targetKey ? { ...target, providerId, model: '' } : target);
  }

  function selectModel(targetKey: number, model: string): void {
    comboTargets = comboTargets.map((target) => target.key === targetKey ? { ...target, model } : target);
  }

  function addTarget(): void {
    if (comboTargets.length >= MAX_COMBO_TARGETS) return;
    comboTargets = [...comboTargets, { key: nextTargetKey++, providerId: '', model: '' }];
  }

  function removeTarget(targetKey: number): void {
    if (comboTargets.length < 2) return;
    if (activeModelTargetKey === targetKey) activeModelTargetKey = null;
    comboTargets = comboTargets.filter((target) => target.key !== targetKey);
  }

  function activateModelPicker(targetKey: number): void {
    activeModelTargetKey = targetKey;
  }

  function deactivateModelPicker(targetKey: number): void {
    if (activeModelTargetKey === targetKey) activeModelTargetKey = null;
  }

  function moveTarget(index: number, direction: -1 | 1): void {
    const destination = index + direction;
    if (destination < 0 || destination >= comboTargets.length) return;
    const ordered = [...comboTargets];
    [ordered[index], ordered[destination]] = [ordered[destination], ordered[index]];
    comboTargets = ordered;
  }

  function targetView(target: ComboTargetDraft, index: number): ComboTargetView {
    return {
      target,
      index,
      count: comboTargets.length,
      providers,
    };
  }

  function openProviders(): void {
    close();
    onNavigate('providers');
  }

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    formError = '';
    saving = true;
    try {
      const targets = comboTargets.map((target, index) => ({
        provider_id: target.providerId,
        model: target.model,
        priority: index + 1,
        enabled: true,
      }));
      if (comboToEdit) {
        await api.updateCombo(comboToEdit.id, {
          name: comboName.trim(),
          strategy,
          targets,
        });
      } else {
        await api.createCombo({
          name: comboName.trim(),
          strategy,
          accepted_protocols: ['chat_completions', 'responses', 'messages'],
          targets,
        });
      }
      saving = false;
      onClose();
      onSaved();
    } catch (error) {
      formError = localizedError(error, comboToEdit ? 'Could not save this combo.' : 'Could not save this combo.', tr);
    } finally {
      saving = false;
    }
  }

  function close(): void {
    pendingInitialProviderSelection = false;
    activeModelTargetKey = null;
    onClose();
  }
</script>

<ArkDialog
  {open}
  class="combo-editor-dialog"
  closeLabel={tr('Close dialog')}
  title={comboToEdit ? tr('Edit combo “{name}”', { name: comboToEdit.name }) : tr('Create a combo')}
  kicker={tr('EXOROUTE CONTROL PLANE')}
  wide
  onClose={close}
>
  <form class="modal-form combo-editor-form" onsubmit={submit}>
    <div class="combo-config-grid">
      <div class="combo-config-col">
        <ArkField
          label={tr('Combo name')}
          bind:value={comboName}
          required
          placeholder={tr('e.g. coding')}
          helperText={tr('Client requests address this combo using this name as the model parameter.')}
        />
      </div>

      <div class="combo-config-col">
        <ArkSelect
          label={tr('Routing strategy')}
          value={strategy}
          items={[
            { label: tr('Ordered fallback'), value: 'priority' },
            { label: tr('Round robin fallback'), value: 'round_robin' },
          ]}
          onValueChange={(val) => strategy = val}
        />
        <div class="combo-strategy-callout" class:is-round-robin={strategy === 'round_robin'}>
          <span class="strategy-callout-icon">
            {#if strategy === 'round_robin'}<RefreshCw size={12} />{:else}<ListOrdered size={12} />{/if}
          </span>
          <span class="strategy-callout-text">
            {tr(strategy === 'round_robin'
              ? 'Rotates primary target per request, then falls back to later targets.'
              : 'Tries targets in strict order. First healthy target serves requests.')}
          </span>
        </div>
      </div>
    </div>

    <div class="combo-targets-section-bar">
      <div class="targets-section-info">
        <div class="targets-section-title">
          <Layers3 size={15} />
          <strong>{tr('Pipeline Targets')}</strong>
          <span class="targets-count-pill">{comboTargets.length} / {MAX_COMBO_TARGETS}</span>
        </div>
        <span class="targets-section-desc">{tr('Requests evaluate targets from top to bottom')}</span>
      </div>
      <button
        type="button"
        class="secondary-button compact add-target-header-btn"
        disabled={saving || comboTargets.length >= MAX_COMBO_TARGETS}
        onclick={addTarget}
      >
        <Plus size={13} />
        {tr('Add fallback target')}
      </button>
    </div>

    <div class="combo-target-editor-list">
      {#each comboTargets as target, index (target.key)}
        {#if index > 0}
          <div class="editor-target-connector">
            <span class="connector-line"></span>
            <span class="connector-chip">
              <ArrowDown size={11} />
              <span>{tr(strategy === 'round_robin' ? 'Round robin fallback' : 'Fallback failover')}</span>
            </span>
          </div>
        {/if}
        <ComboTargetEditor
          view={targetView(target, index)}
          actions={targetActions}
          active={open && activeModelTargetKey === target.key}
          {tr}
        />
      {/each}
    </div>

    {#if comboTargets.length < MAX_COMBO_TARGETS}
      <button
        type="button"
        class="add-target-dashed-btn"
        disabled={saving}
        onclick={addTarget}
      >
        <Plus size={14} />
        <span>{tr('Add another fallback target')}</span>
      </button>
    {:else}
      <p class="form-help text-center">{tr('Maximum {count} targets reached.', { count: MAX_COMBO_TARGETS })}</p>
    {/if}

    {#if formError}
      <div class="form-error" role="alert">{formError}</div>
    {/if}

    <div class="modal-actions">
      <button type="button" class="secondary-button" onclick={close}>{tr('Cancel')}</button>
      <button class="primary-button" disabled={saving || !hasEnabledProviders || !validTargets}>
        {#if saving}
          <LoaderCircle size={15} class="spin" />
        {:else if comboToEdit}
          <Save size={15} />
        {:else}
          <Plus size={15} />
        {/if}
        {tr(comboToEdit ? 'Save changes' : 'Create combo')}
      </button>
    </div>

    {#if !hasEnabledProviders}
      <p class="form-help text-center">{tr('Add and enable a provider before creating a combo.')}</p>
    {/if}
  </form>
</ArkDialog>
