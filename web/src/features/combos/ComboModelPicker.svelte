<script lang="ts">
  import Button from "../../components/Button.svelte";
  import SelectSearch from '../../components/SelectSearch.svelte';
  import { afterUpdate, onDestroy } from 'svelte';
  import { LoaderCircle } from '@lucide/svelte';
  import { api } from '../../lib/api';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { ComboTargetDraft } from './combo.types';

  const MODEL_PAGE_SIZE = 50;
  const MODEL_SEARCH_DEBOUNCE_MS = 200;
  const MIN_MODEL_SEARCH_LENGTH = 2;

  export let target: ComboTargetDraft;
  export let active: boolean;
  export let tr: Translate;
  export let onModelChange: (targetKey: number, model: string) => void;
  export let onActivate: (targetKey: number) => void;
  export let onDeactivate: (targetKey: number) => void;
  export let onOpenProviders: () => void;

  let models: string[] = [];
  let hasMore = false;
  let searchQuery = '';
  let state: 'idle' | 'loading' | 'ready' | 'empty' | 'error' = 'idle';
  let error = '';
  let activeProviderId = '';
  let focused = false;
  let requestGeneration = 0;
  let controller: AbortController | undefined;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  $: visibleModels = target.model && !models.includes(target.model)
    ? [target.model, ...models].slice(0, MODEL_PAGE_SIZE)
    : models.slice(0, MODEL_PAGE_SIZE);
  $: pickerItems = visibleModels.map((model) => ({ label: model, value: model }));
  // The panel's empty row carries the same state message the old Ark
  // Combobox.Empty rendered while the catalog loads, is filtered to nothing,
  // or fails.
  $: noOptionsText = noOptionsMessage();

  function noOptionsMessage(): string {
    if (state === 'empty' && searchQuery) return tr('No matching saved models.');
    if (state === 'empty') return tr('No saved models for this provider. Import its model list first.');
    if (state === 'idle' && searchQuery.length === 1) return tr('Type at least 2 characters to search saved models.');
    if (state === 'idle') return tr('Focus this field to load saved models.');
    if (state === 'error') return error;
    return tr('Loading saved models…');
  }

  afterUpdate(() => {
    if (!active || !focused) {
      if (activeProviderId || controller || searchTimer || models.length || state !== 'idle') {
        activeProviderId = '';
        clearCatalog();
      }
      return;
    }
    if (target.providerId === activeProviderId) return;

    activeProviderId = target.providerId;
    searchQuery = '';
    clearCatalog();
    if (target.providerId) void loadModels(target.providerId, '');
  });

  function cancelPending(): void {
    requestGeneration += 1;
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = undefined;
    controller?.abort();
    controller = undefined;
  }

  function clearCatalog(): void {
    cancelPending();
    models = [];
    hasMore = false;
    error = '';
    state = 'idle';
  }

  async function loadModels(providerId: string, query: string): Promise<void> {
    if (!active || !focused || !providerId || target.providerId !== providerId) return;
    cancelPending();
    const generation = requestGeneration;
    const requestController = new AbortController();
    controller = requestController;
    state = 'loading';
    error = '';
    try {
      const catalog = await api.searchProviderModels(providerId, {
        limit: MODEL_PAGE_SIZE,
        q: query,
        signal: requestController.signal,
      });
      if (generation !== requestGeneration || controller !== requestController || !active || !focused || target.providerId !== providerId) return;

      models = Array.isArray(catalog?.models) ? catalog.models.slice(0, MODEL_PAGE_SIZE) : [];
      hasMore = catalog?.has_more === true;
      state = models.length ? 'ready' : 'empty';
      if (!query && !hasMore && models.length === 1 && !target.model) {
        onModelChange(target.key, models[0]);
      }
    } catch (cause) {
      if (generation !== requestGeneration || controller !== requestController || !active || !focused || target.providerId !== providerId) return;
      state = 'error';
      error = localizedError(cause, 'Could not load model catalog.', tr);
    } finally {
      if (controller === requestController) controller = undefined;
    }
  }

  // SelectSearch reports every open ('' query) and keystroke; the old
  // debounce, minimum length, and abort semantics are unchanged.
  function handleSearch(value: string): void {
    searchQuery = value.trim();
    cancelPending();
    models = [];
    hasMore = false;
    error = '';

    const providerId = target.providerId;
    if (!providerId || !active || !focused) {
      state = 'idle';
      return;
    }
    if (searchQuery.length > 0 && searchQuery.length < MIN_MODEL_SEARCH_LENGTH) {
      state = 'idle';
      return;
    }

    state = 'loading';
    searchTimer = setTimeout(() => {
      searchTimer = undefined;
      void loadModels(providerId, searchQuery);
    }, MODEL_SEARCH_DEBOUNCE_MS);
  }

  function handleValueChange(model: string): void {
    searchQuery = '';
    onModelChange(target.key, model);
  }

  function activate(): void {
    focused = true;
    onActivate(target.key);
  }

  function deactivate(): void {
    focused = false;
    onDeactivate(target.key);
  }

  // The shared SelectSearch owns its open state; activation follows DOM focus
  // so the parent keeps exactly one loading picker and cleanup still runs when
  // focus leaves the field.
  function handleFocusIn(): void {
    if (!focused) activate();
  }

  function handleFocusOut(event: FocusEvent): void {
    if (!focused) return;
    const next = event.relatedTarget as Node | null;
    const container = event.currentTarget as HTMLElement;
    if (next && container.contains(next)) return;
    deactivate();
  }

  function retry(): void {
    if (target.providerId && searchQuery.length !== 1) {
      const providerId = target.providerId;
      const query = searchQuery;
      focused = true;
      activeProviderId = providerId;
      onActivate(target.key);
      if (searchTimer) clearTimeout(searchTimer);
      searchTimer = setTimeout(() => {
        searchTimer = undefined;
        void loadModels(providerId, query);
      }, 0);
    }
  }

  onDestroy(() => {
    cancelPending();
    if (focused) onDeactivate(target.key);
  });
</script>

<div class="combo-model-field combo-picker" onfocusin={handleFocusIn} onfocusout={handleFocusOut}>
  <SelectSearch
    items={pickerItems}
    value={target.model}
    label={tr('Upstream model')}
    ariaLabel={tr('Search saved models')}
    placeholder={tr(target.providerId ? 'Search saved models' : 'Choose a provider first.')}
    disabled={!target.providerId}
    clearable={false}
    {noOptionsText}
    onSearch={handleSearch}
    onValueChange={handleValueChange}
  />

  {#if target.providerId && (state === 'loading' || state === 'empty' || state === 'error')}
    <div class="target-catalog-status" aria-live="polite">
      {#if state === 'loading'}<LoaderCircle size={13} class="spin" /><span>{tr('Loading saved models…')}</span>
      {:else if state === 'empty' && searchQuery}<span>{tr('No matching saved models.')}</span>
      {:else if state === 'empty'}<span>{tr('No saved models for this provider. Import its model list first.')}</span><Button type="button" class="catalog-action" onclick={onOpenProviders}>{tr('Open providers')}</Button>
      {:else if state === 'error'}<span class="catalog-error-text">{error}</span><Button type="button" class="catalog-action" onclick={retry}>{tr('Retry loading models')}</Button>
      {/if}
    </div>
  {/if}
</div>

<style>
  /* Keep the compact combo-row face of the former Ark combobox input while
     rendering through the shared SelectSearch control. */
  .combo-picker :global(input),
  .combo-picker :global(button[aria-haspopup='listbox']) {
    height: 35px;
    min-height: 35px;
    padding-top: 0;
    padding-bottom: 0;
    padding-right: 32px;
    border-radius: 7px;
    font-size: 12.5px;
  }
</style>
