<script lang="ts">
  import Button from "../../components/Button.svelte";
  import { ArrowDown, ArrowUp, GitBranch, Trash2 } from '@lucide/svelte';
  import ArkCheckbox from '../../components/ArkCheckbox.svelte';
  import ArkField from '../../components/ArkField.svelte';
  import SelectSearch from '../../components/SelectSearch.svelte';
  import type { Translate } from '../../lib/format';
  import type { ProxyEndpoint, ProxyProviderOption } from '../../lib/types';

  interface Props {
    endpoint: ProxyEndpoint;
    index: number;
    options: ProxyProviderOption[];
    tr: Translate;
    optionsLoading: boolean;
    optionsQuery: string;
    hasMoreOptions: boolean;
    onOptionsSearch: (query: string) => void;
    onOptionsMore: () => void;
    onChange: (endpoint: ProxyEndpoint) => void;
    onMove: (direction: -1 | 1) => void;
    onRemove: () => void;
  }
  let { endpoint, index, options, tr, optionsLoading, optionsQuery, hasMoreOptions, onOptionsSearch, onOptionsMore, onChange, onMove, onRemove }: Props = $props();

  let embedded = $derived(Boolean(endpoint.ref_proxy_id));
  function update(patch: Partial<ProxyEndpoint>): void {
    onChange({ ...endpoint, ...patch, endpoint_id: endpoint.endpoint_id || crypto.randomUUID() });
  }
</script>

<section class="proxy-endpoint-editor">
  <header class="proxy-endpoint-head">
    <span class="proxy-endpoint-number">{index + 1}</span>
    <div><strong>{tr('Endpoint {number}', { number: index + 1 })}</strong><small>{tr(embedded ? 'Embedded proxy group' : 'HTTP proxy server')}</small></div>
    <div class="proxy-endpoint-tools">
      <Button class="row-icon" type="button" aria-label={tr('Move endpoint up')} title={tr('Move up')} disabled={index === 0} onclick={() => onMove(-1)}><ArrowUp size={14}/></Button>
      <Button class="row-icon" type="button" aria-label={tr('Move endpoint down')} title={tr('Move down')} onclick={() => onMove(1)}><ArrowDown size={14}/></Button>
      <Button class="row-icon danger-hover" type="button" aria-label={tr('Remove endpoint')} title={tr('Remove endpoint')} onclick={onRemove}><Trash2 size={14}/></Button>
    </div>
  </header>
  <div class="proxy-endpoint-mode">
    <Button type="button" stateClasses={{ active: !embedded }} onclick={() => update({ ref_proxy_id: null })}>{tr('Proxy URL')}</Button>
    <Button type="button" stateClasses={{ active: embedded }} onclick={() => update({ url: null, username: null, secret: null, ref_proxy_id: options[0]?.id ?? null })}><GitBranch size={13}/>{tr('Embed proxy group')}</Button>
  </div>
  {#if embedded}
    <div class="proxy-group-picker">
      <SelectSearch
        items={options.map((option) => ({ label: option.name, value: option.id, keywords: option.id, hint: option.enabled ? undefined : tr('disabled') }))}
        value={endpoint.ref_proxy_id ?? ''}
        label={tr('Embedded proxy')}
        placeholder={tr('Search proxy groups')}
        noOptionsText={tr('No proxy groups found')}
        clearable={false}
        onSearch={onOptionsSearch}
        onValueChange={(value) => update({ ref_proxy_id: value || null })}
      />
      {#if hasMoreOptions}
        <Button class="secondary-button compact" type="button" disabled={optionsLoading} onclick={onOptionsMore}>{tr(optionsLoading ? 'Loading…' : 'Load more')}</Button>
      {/if}
    </div>
    <p class="form-note">{tr('The referenced proxy is expanded here in priority order. Cycles and excessive nesting are rejected.')}</p>
  {:else}
    <div class="proxy-endpoint-fields">
      <ArkField label={tr('Proxy URL')} value={endpoint.url ?? ''} placeholder={tr('10.0.0.1:8080 or https://proxy.example:8443')} disabled={false} onValueChange={(value) => update({ url: value, ref_proxy_id: null })} />
      <ArkField label={tr('Label (optional)')} value={endpoint.label ?? ''} placeholder={tr('Primary, backup…')} onValueChange={(value) => update({ label: value || null })} />
      <ArkField label={tr('Username (optional)')} value={endpoint.username ?? ''} placeholder={tr('Proxy username')} onValueChange={(value) => update({ username: value || null })} />
      <ArkField label={tr(endpoint.has_secret ? 'New password (leave blank to keep current)' : 'Password (optional)')} value={endpoint.secret ?? ''} type="password" placeholder={tr(endpoint.has_secret ? 'Saved securely' : 'Proxy password')} onValueChange={(value) => update({ secret: value || null })} />
    </div>
  {/if}
  <ArkCheckbox checked={endpoint.enabled} label={tr('Enable this endpoint')} onCheckedChange={(checked) => update({ enabled: checked === true })} />
</section>
