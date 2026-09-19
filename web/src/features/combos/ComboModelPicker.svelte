<script lang="ts">
  import { afterUpdate, onDestroy } from 'svelte';
  import { Check, ChevronsUpDown, LoaderCircle } from '@lucide/svelte';
  import { Portal } from '@ark-ui/svelte/portal';
  import { Combobox, createListCollection } from '@ark-ui/svelte/combobox';
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
  let inputValue = '';
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
  $: collection = createListCollection({
    items: visibleModels.map((model) => ({ label: model, value: model })),
  });
  $: selectedValues = target.model ? [target.model] : [];

  afterUpdate(() => {
    if (!active || !focused) {
      if (!focused && inputValue !== target.model) {
        inputValue = target.model;
      }
      if (activeProviderId || controller || searchTimer || models.length || state !== 'idle') {
        activeProviderId = '';
        clearCatalog();
      }
      return;
    }
    if (target.providerId === activeProviderId) return;

    activeProviderId = target.providerId;
    searchQuery = '';
    inputValue = target.model;
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
        inputValue = models[0];
      }
    } catch (cause) {
      if (generation !== requestGeneration || controller !== requestController || !active || !focused || target.providerId !== providerId) return;
      state = 'error';
      error = localizedError(cause, 'Could not load model catalog.', tr);
    } finally {
      if (controller === requestController) controller = undefined;
    }
  }

  function searchModels(value: string): void {
    inputValue = value;
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

  function handleInputValueChange(details: { inputValue: string; reason?: string }): void {
    inputValue = details.inputValue;
    if (details.reason === 'input-change') {
      searchModels(details.inputValue);
    } else if (details.reason === 'item-select') {
      searchQuery = '';
    } else if (details.reason === 'interact-outside') {
      searchQuery = '';
      inputValue = target.model;
    }
  }

  function handleValueChange(details: { value: string[] }): void {
    const model = details.value[0] ?? '';
    inputValue = model;
    searchQuery = '';
    onModelChange(target.key, model);
  }

  function activate(): void {
    focused = true;
    onActivate(target.key);
  }

  function handleInputFocus(event: FocusEvent & { currentTarget: HTMLInputElement }): void {
    event.currentTarget.select();
    activate();
  }

  function deactivate(): void {
    focused = false;
    onDeactivate(target.key);
  }

  function handleOpenChange(details: { open: boolean }): void {
    if (details.open) activate();
    else deactivate();
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

<div class="combo-model-field">
  <Combobox.Root
    {collection}
    value={selectedValues}
    {inputValue}
    onInputValueChange={handleInputValueChange}
    onValueChange={handleValueChange}
    onOpenChange={handleOpenChange}
    disabled={!target.providerId}
    openOnClick
    closeOnSelect
    lazyMount
    unmountOnExit
    positioning={{ sameWidth: true, fitViewport: true }}
    class="combo-model-picker"
  >
    <Combobox.Label class="combo-field-label">{tr('Upstream model')}</Combobox.Label>
    <Combobox.Control class="combo-combobox-control">
      <Combobox.Input class="ark-field-input combo-combobox-input" placeholder={tr(target.providerId ? 'Search saved models' : 'Choose a provider first.')} autocomplete="off" onfocus={handleInputFocus} />
      <Combobox.Trigger class="combo-combobox-trigger" aria-label={tr('Search saved models')}>
        <ChevronsUpDown size={14} />
      </Combobox.Trigger>
    </Combobox.Control>
    <Portal>
      <Combobox.Positioner class="ark-select-positioner">
        <Combobox.Content class="ark-select-content combo-combobox-content">
          <Combobox.List class="ark-select-item-group">
            {#each collection.items as item (item.value)}
              <Combobox.Item class="ark-select-item" {item}>
                <Combobox.ItemText class="ark-select-item-text">{item.label}</Combobox.ItemText>
                <Combobox.ItemIndicator class="ark-select-item-indicator"><Check size={14} /></Combobox.ItemIndicator>
              </Combobox.Item>
            {:else}
              <Combobox.Empty class="ark-select-empty">
                {#if state === 'empty' && searchQuery}{tr('No matching saved models.')}
                {:else if state === 'empty'}{tr('No saved models for this provider. Import its model list first.')}
                {:else if state === 'idle' && searchQuery.length === 1}{tr('Type at least 2 characters to search saved models.')}
                {:else if state === 'idle'}{tr('Focus this field to load saved models.')}
                {:else if state === 'error'}{error}
                {:else}{tr('Loading saved models…')}{/if}
              </Combobox.Empty>
            {/each}
          </Combobox.List>
        </Combobox.Content>
      </Combobox.Positioner>
    </Portal>
  </Combobox.Root>

  {#if target.providerId && (state === 'loading' || state === 'empty' || state === 'error')}
    <div class="target-catalog-status" aria-live="polite">
      {#if state === 'loading'}<LoaderCircle size={13} class="spin" /><span>{tr('Loading saved models…')}</span>
      {:else if state === 'empty' && searchQuery}<span>{tr('No matching saved models.')}</span>
      {:else if state === 'empty'}<span>{tr('No saved models for this provider. Import its model list first.')}</span><button type="button" class="catalog-action" onclick={onOpenProviders}>{tr('Open providers')}</button>
      {:else if state === 'error'}<span class="catalog-error-text">{error}</span><button type="button" class="catalog-action" onclick={retry}>{tr('Retry loading models')}</button>
      {/if}
    </div>
  {/if}
</div>
