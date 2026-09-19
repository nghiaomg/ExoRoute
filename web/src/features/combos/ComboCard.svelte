<script lang="ts">
  import { ArrowDown, ArrowRight, Check, Copy, Layers3, ListOrdered, Pencil, Radio, RefreshCw, Trash2 } from '@lucide/svelte';
  import type { GatewayCombo } from '../../lib/types';
  import type { Translate } from '../../lib/format';
  import { labelProtocol, labelStrategy } from '../../lib/labels';

  export let combo: GatewayCombo;
  export let providerNames: Record<string, string>;
  export let tr: Translate;
  export let deleting = false;
  export let onDelete: (combo: GatewayCombo) => void;
  export let onEdit: ((combo: GatewayCombo) => void) | undefined = undefined;

  let copied = false;
  let copyTimeout: ReturnType<typeof setTimeout> | undefined;

  async function copyModelName(event: MouseEvent): Promise<void> {
    event.stopPropagation();
    try {
      await navigator.clipboard.writeText(combo.name);
      copied = true;
      if (copyTimeout) clearTimeout(copyTimeout);
      copyTimeout = setTimeout(() => { copied = false; }, 1500);
    } catch {
      // Clipboard can be unavailable in restricted environments
    }
  }

  function handleCardClick(): void {
    onEdit?.(combo);
  }
</script>

<div
  class="combo-card"
  role="button"
  tabindex="0"
  onclick={handleCardClick}
  onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleCardClick(); } }}
>
  <div class="combo-card-top">
    <span class="combo-icon"><Layers3 size={18} /></span>
    <div class="combo-title">
      <div class="combo-name-row">
        <h3>{combo.name}</h3>
        <button
          type="button"
          class="combo-copy-btn"
          aria-label={tr(copied ? 'Model name copied' : 'Copy model name')}
          title={tr(copied ? 'Model name copied' : 'Copy model name')}
          onclick={copyModelName}
        >
          {#if copied}
            <Check size={12} class="copied-check" />
          {:else}
            <Copy size={12} />
          {/if}
        </button>
      </div>
      <span class="combo-id">{combo.id}</span>
    </div>
    <span class="strategy-badge" class:is-round-robin={combo.strategy === 'round_robin'}>
      {#if combo.strategy === 'round_robin'}
        <RefreshCw size={12} />
      {:else}
        <ListOrdered size={12} />
      {/if}
      {labelStrategy(combo.strategy, tr)}
    </span>
    <div class="combo-card-actions">
      {#if onEdit}
        <button
          type="button"
          class="row-icon combo-edit-btn"
          aria-label={tr('Edit combo')}
          title={tr('Edit combo')}
          onclick={(e) => { e.stopPropagation(); onEdit(combo); }}
        >
          <Pencil size={14} />
        </button>
      {/if}
      <button
        type="button"
        class="row-icon danger-hover combo-delete"
        aria-label={tr('Delete combo “{name}”? This cannot be undone.', { name: combo.name })}
        title={tr('Delete combo “{name}”? This cannot be undone.', { name: combo.name })}
        disabled={deleting}
        onclick={(e) => { e.stopPropagation(); onDelete(combo); }}
      >
        <Trash2 size={14} />
      </button>
    </div>
  </div>

  <div class="combo-meta">
    <span class="meta-target-count">
      <Layers3 size={13} />
      {#if combo.targets.length <= 1}
        {combo.targets.length} {tr('target')}
      {:else}
        <strong>1 {tr('Primary')}</strong> + {combo.targets.length - 1} {tr('Fallback {index}', { index: '' }).replace('{index}', '').trim()}
      {/if}
    </span>
    <span class="meta-protocols">
      <Radio size={13} />
      {combo.accepted_protocols.map((protocol) => labelProtocol(protocol, tr)).join(' · ') || tr('Any protocol')}
    </span>
  </div>

  <div class="target-pipeline">
    {#each combo.targets as target, index}
      {#if index > 0}
        <div class="pipeline-connector">
          <span class="connector-line"></span>
          <span class="connector-badge">
            <ArrowDown size={11} />
            <span>{tr(combo.strategy === 'round_robin' ? 'Round robin' : 'Failover')}</span>
          </span>
        </div>
      {/if}
      <div class="pipeline-target-item" class:is-primary={index === 0}>
        <div class="target-step-indicator">
          <span class="step-tag" class:primary={index === 0} class:fallback={index > 0}>
            {index === 0 ? tr('Primary') : tr('Fallback {index}', { index })}
          </span>
        </div>
        <div class="target-details">
          <div class="target-provider-model">
            <strong class="target-provider-name">{providerNames[target.provider_id] ?? target.provider_id}</strong>
            <span class="target-arrow"><ArrowRight size={13} /></span>
            <code class="target-model-chip">{target.model}</code>
          </div>
        </div>
      </div>
    {/each}
    {#if !combo.targets.length}
      <div class="no-targets">{tr('No targets configured')}</div>
    {/if}
  </div>
</div>
