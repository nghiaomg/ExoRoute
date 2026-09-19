<script lang="ts">
  import { ArrowDown, ArrowUp, ShieldCheck, Trash2, Zap } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';

  import type { ComboTargetActions, ComboTargetView } from './combo.types';
  import ComboModelPicker from './ComboModelPicker.svelte';
  import ComboProviderPicker from './ComboProviderPicker.svelte';

  export let view: ComboTargetView;
  export let actions: ComboTargetActions;
  export let active: boolean;
  export let tr: Translate;
</script>

<div class="combo-target-card" class:is-primary={view.index === 0}>
  <div class="target-card-header">
    <div class="target-card-meta">
      <span class="target-card-index">{String(view.index + 1).padStart(2, '0')}</span>
      {#if view.index === 0}
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
      <button
        type="button"
        class="target-icon-btn"
        aria-label={tr('Move target {index} up', { index: view.index + 1 })}
        title={tr('Move up')}
        disabled={view.index === 0}
        onclick={() => actions.onMove(view.index, -1)}
      >
        <ArrowUp size={13} />
      </button>
      <button
        type="button"
        class="target-icon-btn"
        aria-label={tr('Move target {index} down', { index: view.index + 1 })}
        title={tr('Move down')}
        disabled={view.index === view.count - 1}
        onclick={() => actions.onMove(view.index, 1)}
      >
        <ArrowDown size={13} />
      </button>
      {#if view.count > 1}
        <button
          type="button"
          class="target-icon-btn is-danger"
          aria-label={tr('Remove target {index}', { index: view.index + 1 })}
          title={tr('Remove target')}
          onclick={() => actions.onRemove(view.target.key)}
        >
          <Trash2 size={13} />
        </button>
      {/if}
    </div>
  </div>

  <div class="target-card-body">
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
  </div>
</div>
