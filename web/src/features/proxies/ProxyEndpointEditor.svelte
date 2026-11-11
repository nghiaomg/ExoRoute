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

<section class="grid content-start gap-3 rounded-[13px] border border-line bg-surface-sunken p-3.5">
  <header class="flex min-w-0 items-center gap-2.5">
    <span class="grid h-[25px] w-[25px] flex-none place-items-center rounded-lg bg-db-accent-tint text-caption font-bold text-accent-deep">{index + 1}</span>
    <div class="grid min-w-0 flex-1 gap-0.5">
      <strong class="truncate text-caption text-ink">{tr('Endpoint {number}', { number: index + 1 })}</strong>
      <small class="text-muted">{tr(embedded ? 'Embedded proxy group' : 'HTTP proxy server')}</small>
    </div>
    <div class="flex gap-[3px]">
      <Button class="row-icon" type="button" aria-label={tr('Move endpoint up')} title={tr('Move up')} disabled={index === 0} onclick={() => onMove(-1)}><ArrowUp size={14}/></Button>
      <Button class="row-icon" type="button" aria-label={tr('Move endpoint down')} title={tr('Move down')} onclick={() => onMove(1)}><ArrowDown size={14}/></Button>
      <Button class="row-icon danger-hover" type="button" aria-label={tr('Remove endpoint')} title={tr('Remove endpoint')} onclick={onRemove}><Trash2 size={14}/></Button>
    </div>
  </header>
  <div class="flex items-center gap-1 self-start rounded-[10px] bg-surface p-[3px]">
    <button
      type="button"
      data-active={embedded ? 'false' : 'true'}
      onclick={() => update({ ref_proxy_id: null })}
      class="inline-flex min-h-[29px] cursor-pointer items-center gap-1.5 rounded-lg px-2.5 py-[5px] text-caption text-muted transition-colors hover:text-ink data-[active=true]:bg-db-tint-strong data-[active=true]:text-ink data-[active=true]:shadow-sm"
    >{tr('Proxy URL')}</button>
    <button
      type="button"
      data-active={embedded ? 'true' : 'false'}
      onclick={() => update({ url: null, username: null, secret: null, ref_proxy_id: options[0]?.id ?? null })}
      class="inline-flex min-h-[29px] cursor-pointer items-center gap-1.5 rounded-lg px-2.5 py-[5px] text-caption text-muted transition-colors hover:text-ink data-[active=true]:bg-db-tint-strong data-[active=true]:text-ink data-[active=true]:shadow-sm"
    ><GitBranch size={13}/>{tr('Embed proxy group')}</button>
  </div>
  {#if embedded}
    <div class="grid content-start gap-2">
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
    <p class="m-0 text-meta text-muted">{tr('The referenced proxy is expanded here in priority order. Cycles and excessive nesting are rejected.')}</p>
  {:else}
    <div class="grid grid-cols-2 gap-2.5 max-[640px]:grid-cols-1">
      <ArkField class="col-span-full" label={tr('Proxy URL')} value={endpoint.url ?? ''} placeholder={tr('10.0.0.1:8080 or https://proxy.example:8443')} disabled={false} onValueChange={(value) => update({ url: value, ref_proxy_id: null })} />
      <ArkField label={tr('Label (optional)')} value={endpoint.label ?? ''} placeholder={tr('Primary, backup…')} onValueChange={(value) => update({ label: value || null })} />
      <ArkField label={tr('Username (optional)')} value={endpoint.username ?? ''} placeholder={tr('Proxy username')} onValueChange={(value) => update({ username: value || null })} />
      <ArkField label={tr(endpoint.has_secret ? 'New password (leave blank to keep current)' : 'Password (optional)')} value={endpoint.secret ?? ''} type="password" placeholder={tr(endpoint.has_secret ? 'Saved securely' : 'Proxy password')} onValueChange={(value) => update({ secret: value || null })} />
    </div>
  {/if}
  <ArkCheckbox checked={endpoint.enabled} label={tr('Enable this endpoint')} onCheckedChange={(checked) => update({ enabled: checked === true })} />
</section>
