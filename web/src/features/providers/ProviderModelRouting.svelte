<script lang="ts">
  import { onDestroy } from 'svelte';
  import { LoaderCircle, RefreshCw, Save, X } from '@lucide/svelte';
  import ArkSelect from '../../components/ArkSelect.svelte';
  import { api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
import { labelProtocol } from '../../lib/labels';
import { localizedError } from '../../lib/errors';
  import type { Provider, ProviderModelRoutingEntry, UpstreamProtocol } from '../../lib/types';

  const ROUTING_PAGE_SIZE = 50;
  const KNOWN_UPSTREAM_PROTOCOLS: UpstreamProtocol[] = [
    'chat_completions', 'responses', 'messages', 'google_generate_content',
  ];

  export let provider: Provider;
  export let tr: Translate;

  let models: ProviderModelRoutingEntry[] = [];
  let supportedProtocols: UpstreamProtocol[] = [];
  let draftProtocols: Record<string, string> = {};
  let pageCursors: Array<string | null> = [null];
  let nextCursor: string | null = null;
  let state: 'idle' | 'loading' | 'ready' | 'error' = 'idle';
  let errorMessage = '';
  let statusMessage = '';
  let rowError = '';
  let savingModel = '';
  let generation = 0;
  let saveSequence = 0;
  let loadedProviderId = '';
  let controller: AbortController | undefined;

  $: isOpencode = (() => {
    const id = provider?.id?.toLowerCase();
    const adapterId = provider?.adapter_id?.toLowerCase();
    return id === 'opencode-go' || id === 'opencode_go' || id === 'opencode-zen' || id === 'opencode_zen'
      || adapterId === 'opencode_go' || adapterId === 'opencode_zen' || adapterId === 'opencode-go' || adapterId === 'opencode-zen';
  })();
  $: supportsModelRouting = provider?.capabilities?.model_protocol_routing === true && !isOpencode;
  $: selectorOptions = [
    { label: tr('Use default mapping'), value: '' },
    ...supportedProtocols.map((protocol) => ({ label: labelProtocol(protocol, tr), value: protocol })),
  ];
  $: if (supportsModelRouting && loadedProviderId !== provider.id) {
    loadedProviderId = provider.id;
    pageCursors = [null];
    models = [];
    draftProtocols = {};
    supportedProtocols = provider.capabilities?.supported_upstream_protocols ?? [];
    statusMessage = '';
    rowError = '';
    savingModel = '';
    saveSequence += 1;
    void loadPage(null);
  } else if (!supportsModelRouting && loadedProviderId) {
    generation += 1;
    controller?.abort();
    controller = undefined;
    loadedProviderId = '';
    models = [];
    draftProtocols = {};
    supportedProtocols = [];
    state = 'idle';
    savingModel = '';
    saveSequence += 1;
  }

  function isCurrentRequest(requestGeneration: number, providerId: string): boolean {
    return requestGeneration === generation && provider.id === providerId && supportsModelRouting;
  }

  function allowedProtocols(values: UpstreamProtocol[] | undefined): UpstreamProtocol[] {
    return [...new Set((values ?? []).filter((value): value is UpstreamProtocol =>
      KNOWN_UPSTREAM_PROTOCOLS.includes(value),
    ))];
  }

  async function loadPage(cursor: string | null): Promise<void> {
    const providerId = provider.id;
    const requestGeneration = ++generation;
    controller?.abort();
    const requestController = new AbortController();
    controller = requestController;
    state = 'loading';
    errorMessage = '';
    statusMessage = '';
    rowError = '';
    try {
      const page = await api.providerModelRouting(providerId, {
        cursor: cursor ?? undefined,
        limit: ROUTING_PAGE_SIZE,
        signal: requestController.signal,
      });
      if (controller !== requestController || !isCurrentRequest(requestGeneration, providerId)) return;
      models = page.models.slice(0, ROUTING_PAGE_SIZE);
      nextCursor = page.next_cursor ?? null;
      supportedProtocols = allowedProtocols(
        page.supported_protocols ?? provider.capabilities?.supported_upstream_protocols,
      );
      draftProtocols = Object.fromEntries(models.map((entry) => [entry.model, entry.override_protocol ?? '']));
      state = 'ready';
    } catch (error) {
      if (controller !== requestController || !isCurrentRequest(requestGeneration, providerId)) return;
      state = 'error';
      errorMessage = localizedError(error, 'Could not load model routing.', tr);
    } finally {
      if (controller === requestController) controller = undefined;
    }
  }

  function draftFor(entry: ProviderModelRoutingEntry): string {
    return Object.prototype.hasOwnProperty.call(draftProtocols, entry.model)
      ? draftProtocols[entry.model]
      : entry.override_protocol ?? '';
  }

  function changeDraft(model: string, protocol: string): void {
    draftProtocols = { ...draftProtocols, [model]: protocol };
    statusMessage = '';
    rowError = '';
  }

  function isSupportedProtocol(value: string): value is UpstreamProtocol {
    return supportedProtocols.includes(value as UpstreamProtocol);
  }

  function isDirty(entry: ProviderModelRoutingEntry): boolean {
    return draftFor(entry) !== (entry.override_protocol ?? '');
  }

  async function saveOverride(entry: ProviderModelRoutingEntry, protocol: UpstreamProtocol | null): Promise<void> {
    if (savingModel || (protocol !== null && !isSupportedProtocol(protocol))) return;
    const providerId = provider.id;
    const requestGeneration = generation;
    const requestSequence = ++saveSequence;
    savingModel = entry.model;
    rowError = '';
    statusMessage = '';
    try {
      await api.updateProviderModelRouting(providerId, entry.model, protocol);
      if (!isCurrentRequest(requestGeneration, providerId)) return;
      const successMessage = protocol === null ? tr('Protocol override cleared.') : tr('Protocol override saved.');
      const reloadGeneration = generation + 1;
      await loadPage(pageCursors[pageCursors.length - 1] ?? null);
      if (isCurrentRequest(reloadGeneration, providerId)) statusMessage = successMessage;
    } catch (error) {
      if (!isCurrentRequest(requestGeneration, providerId)) return;
      rowError = localizedError(error, 'Could not update model routing.', tr);
    } finally {
      if (requestSequence === saveSequence) savingModel = '';
    }
  }

  async function retry(): Promise<void> {
    await loadPage(pageCursors[pageCursors.length - 1] ?? null);
  }

  async function nextPage(): Promise<void> {
    if (!nextCursor || state === 'loading' || savingModel) return;
    const cursor = nextCursor;
    pageCursors = [...pageCursors, cursor];
    await loadPage(cursor);
  }

  async function previousPage(): Promise<void> {
    if (pageCursors.length <= 1 || state === 'loading' || savingModel) return;
    const previous = pageCursors[pageCursors.length - 2] ?? null;
    pageCursors = pageCursors.slice(0, -1);
    await loadPage(previous);
  }

  onDestroy(() => {
    generation += 1;
    saveSequence += 1;
    controller?.abort();
  });
</script>

{#if supportsModelRouting}
  <section class="provider-model-section provider-model-routing">
    <div class="provider-model-section-heading">
      <div>
        <h3>{tr('Model protocol routing')}</h3>
        <p>{tr('Set a default upstream protocol for each model. Combo target overrides take precedence.')}</p>
      </div>
      <button class="secondary-button compact" disabled={state === 'loading' || Boolean(savingModel)} onclick={retry}>
        {#if state === 'loading'}<LoaderCircle size={13} class="spin" />{tr('Loading routes…')}
        {:else}<RefreshCw size={13} />{tr('Refresh routes')}{/if}
      </button>
    </div>

    {#if errorMessage}<div class="form-error" role="alert">{errorMessage}<button class="secondary-button compact" onclick={retry}>{tr('Retry loading routes')}</button></div>{/if}
    {#if statusMessage}<p class="model-routing-status" role="status">{statusMessage}</p>{/if}
    {#if rowError}<p class="model-routing-error" role="alert">{rowError}</p>{/if}

    {#if state === 'loading' && !models.length}
      <div class="inline-loading"><LoaderCircle size={15} class="spin" /><span>{tr('Loading routes…')}</span></div>
    {:else if state === 'ready' && !models.length}
      <div class="provider-model-empty">{tr('No saved models are available for protocol routing.')}</div>
    {:else if state === 'ready'}
      <div class="model-routing-table-scroll" role="region" aria-label={tr('Model protocol routing')}>
        <table class="model-routing-table">
          <thead><tr><th>{tr('Model')}</th><th>{tr('Effective protocol')}</th><th>{tr('Protocol override')}</th><th>{tr('Actions')}</th></tr></thead>
          <tbody>
            {#each models as entry (entry.model)}
              {@const draft = draftFor(entry)}
              {@const validDraft = !draft || isSupportedProtocol(draft)}
              {@const dirty = isDirty(entry)}
              <tr>
                <td><code>{entry.model}</code></td>
                <td>
                  {#if entry.routing_configured && entry.effective_upstream_protocol}
                    <strong>{labelProtocol(entry.effective_upstream_protocol, tr)}</strong>
                    <small>{entry.override_protocol ? tr('Saved override') : tr('Default model mapping')}</small>
                  {:else if entry.override_protocol}
                    <strong class="model-routing-unknown">{labelProtocol(entry.override_protocol, tr)}</strong>
                    <small>{tr('Saved override')}</small>
                  {:else}
                    <strong class="model-routing-unknown">{tr('Protocol unknown')}</strong>
                    <small>{tr('Choose a protocol to enable this model.')}</small>
                  {/if}
                </td>
                <td>
                  <ArkSelect
                    value={draft}
                    items={selectorOptions}
                    label={tr('Protocol override for {model}', { model: entry.model })}
                    disabled={Boolean(savingModel)}
                    onValueChange={(value) => changeDraft(entry.model, value)}
                  />
                  {#if entry.override_protocol && !isSupportedProtocol(entry.override_protocol)}
                    <small class="model-routing-warning">{tr('The saved protocol is no longer supported. Choose a supported value or clear the override.')}</small>
                  {/if}
                </td>
                <td class="model-routing-actions">
                  <button class="secondary-button compact" disabled={!dirty || !validDraft || Boolean(savingModel)} onclick={() => saveOverride(entry, draft ? draft as UpstreamProtocol : null)}>
                    {#if savingModel === entry.model}<LoaderCircle size={12} class="spin" />{:else}<Save size={12} />{/if}{tr('Save')}
                  </button>
                  {#if entry.override_protocol}
                    <button class="secondary-button compact" disabled={Boolean(savingModel)} title={tr('Clear override')} onclick={() => saveOverride(entry, null)}><X size={12} />{tr('Clear override')}</button>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <div class="model-routing-pagination">
        <button class="secondary-button compact" disabled={Boolean(savingModel) || pageCursors.length <= 1} onclick={previousPage}>{tr('Previous')}</button>
        <span>{tr('Page {current}', { current: pageCursors.length })}</span>
        <button class="secondary-button compact" disabled={Boolean(savingModel) || !nextCursor} onclick={nextPage}>{tr('Next')}</button>
      </div>
    {/if}
  </section>
{/if}
