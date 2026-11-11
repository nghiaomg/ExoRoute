<script lang="ts">
  import Input from '../../components/Input.svelte';
  import { afterUpdate, onDestroy } from 'svelte';
  import { Check, ChevronsUpDown } from '@lucide/svelte';
  import { Portal } from '@ark-ui/svelte/portal';
  import { Combobox, createListCollection } from '@ark-ui/svelte/combobox';
  import { api } from '../../lib/api';
  import type { Translate } from '../../lib/format';

  export let providerId: string;
  export let value: string;
  /** Shown while disabled when no provider is chosen yet; defaults to the provider prompt. */
  export let disabledPlaceholder = '';
  /** Popups must portal into the hosting modal dialog, or the top layer makes them inert. */
  export let portalContainer: HTMLElement | undefined = undefined;
  export let tr: Translate;
  export let onChange: (model: string) => void;

  const PAGE_SIZE = 50;
  let models: string[] = [];
  let inputValue = '';
  let query = '';
  let state: 'idle' | 'loading' | 'ready' | 'empty' | 'error' = 'idle';
  let error = '';
  let activeProvider = '';
  let focused = false;
  let generation = 0;
  let controller: AbortController | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;

  $: visibleModels = value && !models.includes(value) ? [value, ...models].slice(0, PAGE_SIZE) : models;
  $: collection = createListCollection({ items: visibleModels.map((model) => ({ label: model, value: model })) });
  $: selectedValues = value ? [value] : [];

  afterUpdate(() => {
    if (!focused) {
      if (inputValue !== value) inputValue = value;
      if (activeProvider || controller || timer || models.length || state !== 'idle') clearCatalog();
      return;
    }
    if (providerId === activeProvider) return;
    activeProvider = providerId;
    query = '';
    inputValue = value;
    clearCatalog();
    if (providerId) void loadModels(providerId, '');
  });

  function cancelPending(): void {
    generation += 1;
    if (timer) clearTimeout(timer);
    timer = undefined;
    controller?.abort();
    controller = undefined;
  }

  function clearCatalog(): void {
    cancelPending();
    models = [];
    error = '';
    state = 'idle';
  }

  async function loadModels(id: string, search: string): Promise<void> {
    if (!focused || !id || providerId !== id) return;
    cancelPending();
    const requestGeneration = generation;
    const requestController = new AbortController();
    controller = requestController;
    state = 'loading';
    error = '';
    try {
      const catalog = await api.searchProviderModels(id, { limit: PAGE_SIZE, q: search, signal: requestController.signal });
      if (requestGeneration !== generation || controller !== requestController || !focused || providerId !== id) return;
      models = Array.isArray(catalog?.models) ? catalog.models.slice(0, PAGE_SIZE) : [];
      state = models.length ? 'ready' : 'empty';
    } catch {
      if (requestGeneration !== generation || controller !== requestController || !focused || providerId !== id) return;
      error = tr('Could not load model catalog.');
      state = 'error';
    } finally {
      if (controller === requestController) controller = undefined;
    }
  }

  function searchModels(next: string): void {
    inputValue = next;
    query = next.trim();
    cancelPending();
    models = [];
    error = '';
    if (!providerId || !focused) { state = 'idle'; return; }
    if (query.length === 1) { state = 'idle'; return; }
    state = 'loading';
    const id = providerId;
    timer = setTimeout(() => { timer = undefined; void loadModels(id, query); }, 200);
  }

  function inputChanged(details: { inputValue: string; reason?: string }): void {
    inputValue = details.inputValue;
    if (details.reason === 'input-change') searchModels(details.inputValue);
    else if (details.reason === 'item-select') query = '';
    else if (details.reason === 'interact-outside') { query = ''; inputValue = value; }
  }

  function selectChanged(details: { value: string[] }): void {
    const selected = details.value[0] ?? '';
    inputValue = selected;
    query = '';
    onChange(selected);
  }

  function activate(): void { focused = true; }
  function deactivate(): void { focused = false; }

  function openChanged(details: { open: boolean }): void {
    if (details.open) activate();
    else deactivate();
  }

  function handleInputFocus(event: FocusEvent & { currentTarget: HTMLInputElement }): void {
    event.currentTarget.select();
    activate();
  }

  onDestroy(cancelPending);
</script>

<div class="combo-model-field target-model-field">
  <Combobox.Root
    {collection}
    value={selectedValues}
    {inputValue}
    onInputValueChange={inputChanged}
    onValueChange={selectChanged}
    onOpenChange={openChanged}
    disabled={!providerId}
    openOnClick
    closeOnSelect
    lazyMount
    unmountOnExit
    positioning={{ sameWidth: true, fitViewport: true }}
    class="combo-target-model-picker"
  >
    <Combobox.Control class="combo-combobox-control">
      <Input variant="combobox"
        class="ark-field-input combo-combobox-input"
        placeholder={providerId ? tr('Search saved models') : (disabledPlaceholder || tr('Choose a provider first.'))}
        aria-label={tr('Target model')}
        autocomplete="off"
        onfocus={handleInputFocus}
      />
      <Combobox.Trigger class="combo-combobox-trigger" aria-label={tr('Search saved models')}>
        <ChevronsUpDown size={14} />
      </Combobox.Trigger>
    </Combobox.Control>
    <Portal container={portalContainer}>
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
                {#if state === 'empty' && query}{tr('No matching saved models.')}
                {:else if state === 'empty'}{tr('No saved models for this provider. Import its model list first.')}
                {:else if state === 'idle' && query.length === 1}{tr('Type at least 2 characters to search saved models.')}
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
</div>
