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
  <button class="secondary-button compact api-key-revoke" disabled={deleting} onclick={(event) => { event.stopPropagation(); onRevoke(apiKey); }}><Trash2 size={14} />{tr('Revoke {name}', { name: apiKey.name })}</button>
</div>
