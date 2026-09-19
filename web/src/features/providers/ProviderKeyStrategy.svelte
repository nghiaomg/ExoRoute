<script lang="ts">
  import { ListOrdered, RefreshCw } from '@lucide/svelte';
  import ArkSelect from '../../components/ArkSelect.svelte';
  import { type Translate } from '../../lib/format';
  import type { ProviderKeyStrategy } from '../../lib/types';

  export let strategy: ProviderKeyStrategy = 'priority';
  export let disabled = false;
  export let tr: Translate;

  $: strategyOptions = [
    { label: tr('Ordered fallback'), value: 'priority' },
    { label: tr('Round robin'), value: 'round_robin' },
  ];
</script>

<div class="detail-form-section provider-key-strategy">
  <div>
    <ArkSelect
      label={tr('Key rotation strategy')}
      items={strategyOptions}
      value={strategy}
      disabled={disabled}
      onValueChange={(val) => {
        strategy = val === 'round_robin' ? 'round_robin' : 'priority';
      }}
    />
  </div>

  <div class="combo-strategy-callout" class:is-round-robin={strategy === 'round_robin'}>
    <span class="strategy-callout-icon">
      {#if strategy === 'round_robin'}<RefreshCw size={12} />{:else}<ListOrdered size={12} />{/if}
    </span>
    <span class="strategy-callout-text">
      {tr(strategy === 'round_robin'
        ? 'Each request starts at the next enabled key, then falls back to the following keys.'
        : 'Every request starts with the first enabled key; the following keys are fallbacks.')}
    </span>
  </div>
</div>

<style>
  .provider-key-strategy { gap: 10px; }
</style>
