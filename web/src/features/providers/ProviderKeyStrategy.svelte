<script lang="ts">
  import { Check, LoaderCircle, ListOrdered, RefreshCw } from '@lucide/svelte';
  import ArkSelect from '../../components/ArkSelect.svelte';
  import { api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { Provider, ProviderKeyStrategy } from '../../lib/types';

  export let provider: Provider;
  export let tr: Translate;
  export let onUpdated: (provider: Provider) => void;

  let providerId = '';
  let strategy: ProviderKeyStrategy = 'priority';
  let saving = false;
  let notice = '';
  let noticeTone: 'success' | 'error' = 'success';

  $: if (provider && provider.id !== providerId) {
    providerId = provider.id;
    strategy = provider.key_strategy ?? 'priority';
    notice = '';
    noticeTone = 'success';
  }

  $: strategyOptions = [
    { label: tr('Ordered fallback'), value: 'priority' },
    { label: tr('Round robin'), value: 'round_robin' },
  ];

  async function save(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!provider || saving) return;

    const targetProvider = provider;
    const savedStrategy = strategy;
    saving = true;
    notice = '';
    try {
      await api.updateProviderKeyStrategy(targetProvider.id, { strategy: savedStrategy });
      notice = tr('Key rotation strategy saved.');
      noticeTone = 'success';
      onUpdated({ ...targetProvider, key_strategy: savedStrategy });
    } catch (error) {
      notice = localizedError(error, 'Could not update the key rotation strategy.', tr);
      noticeTone = 'error';
    } finally {
      saving = false;
    }
  }
</script>

<form class="detail-form-section provider-key-strategy" onsubmit={save}>
  <div>
    <ArkSelect
      label={tr('Key rotation strategy')}
      items={strategyOptions}
      value={strategy}
      disabled={saving}
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

  {#if notice}
    <div class="detail-notice-banner" class:error={noticeTone === 'error'} role={noticeTone === 'error' ? 'alert' : 'status'}>
      {notice}
    </div>
  {/if}

  <div class="provider-key-strategy-actions">
    <button type="submit" class="primary-button compact" disabled={saving}>
      {#if saving}<LoaderCircle size={13} class="spin" />{:else}<Check size={13} />{/if}
      {tr('Save key strategy')}
    </button>
  </div>
</form>

<style>
  .provider-key-strategy { gap: 10px; }
  .provider-key-strategy-actions { display: flex; justify-content: flex-end; }
</style>
