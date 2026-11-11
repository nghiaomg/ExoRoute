<script lang="ts">
  import Button from "../../components/Button.svelte";
  import { AlertTriangle, Check, LoaderCircle, Save, X } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import ArkField from '../../components/ArkField.svelte';
  import { ApiError } from '../../lib/api';
  import { proxyApi } from '../../lib/api/proxies';
  import { localizedError } from '../../lib/errors';
  import type { Translate } from '../../lib/format';
  import type { GatewayProxy } from '../../lib/types';
  import { blankProxy, parseBulkProxyEndpoints } from './proxy.state';

  interface Props { tr: Translate; buttonClass?: string; onImported: (created: number) => void; }
  let { tr, buttonClass = 'secondary-button', onImported }: Props = $props();

  let open = $state(false);
  let text = $state('');
  let namePrefix = $state('Proxy pool');
  let busy = $state(false);
  let error = $state('');
  let completed = $state<number | null>(null);
  const parsed = $derived(parseBulkProxyEndpoints(text, tr));

  async function importAll(): Promise<void> {
    if (busy || parsed.errors.length || parsed.endpoints.length === 0) return;
    busy = true;
    error = '';
    completed = null;
    try {
      const proxies: Array<Omit<GatewayProxy, 'id' | 'created_at' | 'updated_at' | 'provider_count'>> = parsed.endpoints.map((endpoint, index) => ({
        ...blankProxy(`${namePrefix.trim() || tr('Proxy')} ${String(index + 1).padStart(3, '0')}`),
        endpoints: [{ ...endpoint, priority: 0 }],
      }));
      const result = await proxyApi.bulkCreateProxies(proxies);
      completed = result.created;
      onImported(result.created);
    } catch (cause) {
      const partial = cause instanceof ApiError
        ? (cause as ApiError & { created?: number; created_ids?: string[] })
        : null;
      if (partial?.created) {
        completed = partial.created;
        onImported(partial.created);
      }
      error = localizedError(cause, 'Bulk proxy import failed.', tr);
    } finally {
      busy = false;
    }
  }

  function close(): void {
    if (busy) return;
    open = false;
    text = '';
    error = '';
    completed = null;
  }
</script>

  <Button class={buttonClass} type="button" onclick={() => open = true}><span class="button-icon"><svg class="icon"><use href="#i-import" /></svg></span><span class="button-label">{tr('Bulk add')}</span></Button>

<ArkDialog bind:open wide title={tr('Bulk add proxies')} kicker={tr('Import proxy endpoints')} closeLabel={tr('Close dialog')} preventClose={busy} onClose={close}>
  <form class="modal-form gap-3.5" onsubmit={(event) => { event.preventDefault(); void importAll(); }}>
    <p class="modal-description">{tr('Paste one proxy address per line. Bare host:port uses HTTP; http:// and https:// are also accepted. Lines beginning with # are ignored.')}</p>
    <ArkField label={tr('Proxy name prefix')} bind:value={namePrefix} maxlength={80} disabled={busy} placeholder={tr('Proxy pool')} />
    <label class="field">{tr('Proxy list')}<textarea class="w-full min-h-[170px] resize-y rounded-[10px] border border-line bg-surface p-3 font-mono! text-xs! leading-relaxed text-ink placeholder:text-muted focus:outline-none focus:border-accent" bind:value={text} rows="9" placeholder={'10.0.0.1:8080\nhttps://proxy.example:8443 # backup'} spellcheck="false" disabled={busy}></textarea></label>
    {#if parsed.errors.length}
      <div class="flex gap-2 rounded-[10px] border border-db-danger/40 bg-db-danger/10 p-2.5 text-caption text-db-danger" role="alert"><AlertTriangle size={14}/><ul class="m-0 grid content-start gap-1 pl-[18px]">{#each parsed.errors as item (item)}<li>{item}</li>{/each}</ul></div>
    {:else}
      <p class="m-0 text-meta text-muted">{tr('{count} valid endpoints will create individual proxy groups.', { count: parsed.endpoints.length })}</p>
    {/if}
    {#if error}<div class="form-error" role="alert">{error}</div>{/if}
    {#if completed !== null}<div class="flex items-center gap-[7px] text-caption text-db-success" role="status"><Check size={14}/>{tr('Created {count} proxies.', { count: completed })}</div>{/if}
    <div class="modal-actions"><Button class="secondary-button" type="button" onclick={close}>{tr('Cancel')}</Button><Button class="primary-button" type="submit" disabled={busy || !parsed.endpoints.length || parsed.errors.length > 0}>{#if busy}<LoaderCircle size={14} class="spin"/>{tr('Importing…')}{:else}<Save size={14}/>{tr('Create {count} proxies', { count: parsed.endpoints.length })}{/if}</Button></div>
  </form>
</ArkDialog>
