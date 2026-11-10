<script lang="ts">
  import Button from "../../components/Button.svelte";
  import { ArrowDown, ArrowUp, Layers3, Power, PowerOff, Server, ShieldCheck, Trash2, Zap } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';

  import type { ComboTargetActions, ComboTargetView } from './combo.types';
  import ComboModelPicker from './ComboModelPicker.svelte';
  import ComboProviderPicker from './ComboProviderPicker.svelte';
  import ComboReferencePicker from './ComboReferencePicker.svelte';

  export let view: ComboTargetView;
  export let actions: ComboTargetActions;
  export let active: boolean;
  export let tr: Translate;
</script>

<div class="combo-target-card" class:is-primary={view.index === 0} class:is-disabled={!view.target.enabled}>
  <div class="target-card-header">
    <div class="target-card-meta">
      <span class="target-card-index">{String(view.index + 1).padStart(2, '0')}</span>
      {#if !view.target.enabled}
        <span class="target-role-badge is-disabled">
          <PowerOff size={12} />
          <span>{tr('Disabled')}</span>
        </span>
      {:else if view.index === 0}
        <span class="target-role-badge is-primary">
          <Zap size={12} />
          <span>{tr('Primary target')}</span>
        </span>
      {:else}
        <span class="target-role-badge is-fallback">
          <ShieldCheck size={12} />
          <span>{tr('Fallback {index}', { index: view.index })}</span>
        </span>
      {/if}
    </div>

    <div class="target-card-actions">
      <Button
        type="button"
        class="target-icon-btn is-toggle"
        stateClasses={{ 'is-off': !view.target.enabled }}
        aria-label={tr(view.target.enabled ? 'Disable target {index}' : 'Enable target {index}', { index: view.index + 1 })}
        title={tr(view.target.enabled ? 'Disable target' : 'Enable target')}
        onclick={() => actions.onToggleEnabled(view.target.key)}
      >
        {#if view.target.enabled}
          <Power size={13} />
        {:else}
          <PowerOff size={13} />
        {/if}
      </Button>
      <Button
        type="button"
        class="target-icon-btn"
        aria-label={tr('Move target {index} up', { index: view.index + 1 })}
        title={tr('Move up')}
        disabled={view.index === 0}
        onclick={() => actions.onMove(view.index, -1)}
      >
        <ArrowUp size={13} />
      </Button>
      <Button
        type="button"
        class="target-icon-btn"
        aria-label={tr('Move target {index} down', { index: view.index + 1 })}
        title={tr('Move down')}
        disabled={view.index === view.count - 1}
        onclick={() => actions.onMove(view.index, 1)}
      >
        <ArrowDown size={13} />
      </Button>
      {#if view.count > 1}
        <Button
          type="button"
          class="target-icon-btn is-danger"
          aria-label={tr('Remove target {index}', { index: view.index + 1 })}
          title={tr('Remove target')}
          onclick={() => actions.onRemove(view.target.key)}
        >
          <Trash2 size={13} />
        </Button>
      {/if}
    </div>
  </div>

  <div class="target-card-body">
    <div class="target-kind-toggle" role="group" aria-label={tr('Target type')}>
      <Button
        type="button"
        class="target-kind-btn"
        stateClasses={{ 'is-active': view.target.kind === 'provider' }}
        aria-pressed={view.target.kind === 'provider'}
        onclick={() => actions.onKindChange(view.target.key, 'provider')}
      >
        <Server size={12} />
        <span>{tr('Provider target')}</span>
      </Button>
      <Button
        type="button"
        class="target-kind-btn is-combo"
        stateClasses={{ 'is-active': view.target.kind === 'combo' }}
        aria-pressed={view.target.kind === 'combo'}
        onclick={() => actions.onKindChange(view.target.key, 'combo')}
      >
        <Layers3 size={12} />
        <span>{tr('Combo target')}</span>
      </Button>
    </div>

    {#if view.target.kind === 'combo'}
      <div class="target-field-col">
        <ComboReferencePicker
          combos={view.combos}
          selectedComboId={view.target.comboId}
          targetKey={view.target.key}
          {tr}
          onComboChange={actions.onComboChange}
        />
      </div>
      {#if !view.target.comboId}
        <p class="form-help">{tr('Choose a combo to nest as this target.')}</p>
      {/if}
    {:else}
      <div class="target-card-grid">
        <div class="target-field-col">
          <ComboProviderPicker
            providers={view.providers}
            selectedProviderId={view.target.providerId}
            targetKey={view.target.key}
            {tr}
            onProviderChange={actions.onProviderChange}
          />
        </div>
        <div class="target-field-col">
          <ComboModelPicker
            target={view.target}
            {active}
            {tr}
            onModelChange={actions.onModelChange}
            onActivate={actions.onActivateModelPicker}
            onDeactivate={actions.onDeactivateModelPicker}
            onOpenProviders={actions.onOpenProviders}
          />
        </div>
      </div>
    {/if}
  </div>
</div>
