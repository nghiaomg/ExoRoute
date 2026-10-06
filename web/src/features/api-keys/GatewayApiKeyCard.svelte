<script lang="ts">
  import { KeyRound, Trash2 } from '@lucide/svelte';
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import { formatDate, type Translate } from '../../lib/format';
  import type { GatewayApiKey } from '../../lib/types';

  export let apiKey: GatewayApiKey;
  export let tr: Translate;
  export let locale: Locale;
  export let deleting = false;
  export let onDetails: (key: GatewayApiKey) => void;
  export let onRevoke: (key: GatewayApiKey) => void;
</script>

<div
  class="api-key-card"
  role="button"
  tabindex="0"
  onkeydown={(event) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      onDetails(apiKey);
    }
  }}
  onclick={() => onDetails(apiKey)}
>
  <div class="api-key-card-heading"><span class="api-key-card-icon"><KeyRound size={18} /></span><div><h2>{apiKey.name}</h2><code>{apiKey.id}</code></div><span class="state-label" class:enabled={apiKey.enabled}><i></i>{tr(apiKey.enabled ? 'Enabled' : 'Disabled')}</span></div>
  <div class="api-key-card-dates">
    <div><span>{tr('Created')}</span><strong>{formatDate(apiKey.created_at, locale)}</strong></div>
    <div><span>{tr('Last used')}</span><strong>{apiKey.last_used_at ? formatDate(apiKey.last_used_at, locale) : tr('Never used')}</strong></div>
    <div><span>{tr('API requests')}</span><strong>{(apiKey.request_count ?? 0).toLocaleString(getIntlLocale(locale))}</strong></div>
  </div>
  <div class="api-key-card-scope">
    {#if apiKey.scope_valid === false}
      <span class="scope-chip invalid">{tr('Invalid scope')}</span>
    {:else}
      <span class="scope-chip">
        {apiKey.allowed_provider_ids?.length
          ? tr('{count} allowed providers', { count: apiKey.allowed_provider_ids.length })
          : tr('All providers')}
      </span>
      <span class="scope-chip">
        {apiKey.allowed_models?.length
          ? tr('{count} model rules', { count: apiKey.allowed_models.length })
          : tr('All models')}
      </span>
    {/if}
  </div>
  <button class="secondary-button compact api-key-revoke" disabled={deleting} onclick={(event) => { event.stopPropagation(); onRevoke(apiKey); }}><Trash2 size={14} />{tr('Revoke {name}', { name: apiKey.name })}</button>
</div>

<style>
  .api-key-card-scope {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .scope-chip {
    padding: 3px 9px;
    color: var(--muted);
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--paper);
    font: 600 11px var(--font-sans);
  }
  .scope-chip.invalid {
    color: #b45309;
    border-color: rgba(180, 83, 9, 0.28);
    background: rgba(249, 115, 22, 0.08);
  }
</style>
