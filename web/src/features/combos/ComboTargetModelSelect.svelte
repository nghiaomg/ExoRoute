<script lang="ts">
  import SelectSearch from '../../components/SelectSearch.svelte';
  import { afterUpdate, onDestroy } from 'svelte';
  import { api } from '../../lib/api';
  import type { Translate } from '../../lib/format';

  export let providerId: string;
  export let value: string;
  /** Shown while disabled when no provider is chosen yet; defaults to the provider prompt. */
  export let disabledPlaceholder = '';
  /** Kept for source compatibility: the native panel renders in flow, so no portal is needed. */
  export let portalContainer: HTMLElement | undefined = undefined;
  void portalContainer;
  export let tr: Translate;
  export let onChange: (model: string) => void;

  const PAGE_SIZE = 50;
  let models: string[] = [];
  let query = '';
  let state: 'idle' | 'loading' | 'ready' | 'empty' | 'error' = 'idle';
  let error = '';
  let activeProvider = '';
  let focused = false;
  let generation = 0;
  let controller: AbortController | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;

  $: visibleModels = value && !models.includes(value) ? [value, ...models].slice(0, PAGE_SIZE) : models;
  $: pickerItems = visibleModels.map((model) => ({ label: model, value: model }));
  $: noOptionsText = noOptionsMessage();

  function noOptionsMessage(): string {
    if (state === 'empty' && query) return tr('No matching saved models.');
    if (state === 'empty') return tr('No saved models for this provider. Import its model list first.');
    if (state === 'idle' && query.length === 1) return tr('Type at least 2 characters to search saved models.');
    if (state === 'idle') return tr('Focus this field to load saved models.');
    if (state === 'error') return error;
    return tr('Loading saved models…');
  }

  afterUpdate(() => {
    if (!focused) {
      if (activeProvider || controller || timer || models.length || state !== 'idle') clearCatalog();
      return;
    }
    if (providerId === activeProvider) return;
    activeProvider = providerId;
    query = '';
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

  // SelectSearch reports each open ('' query) and keystroke; the debounce,
  // minimum length, and abort semantics of the former Ark combobox are kept.
  function searchModels(next: string): void {
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

  function handleValueChange(model: string): void {
    query = '';
    onChange(model);
  }

  // The shared SelectSearch owns its open state; activation follows DOM focus.
  function activate(): void { focused = true; }
  function deactivate(): void { focused = false; }

  function handleFocusIn(): void {
    activate();
  }

  function handleFocusOut(event: FocusEvent): void {
    if (!focused) return;
    const next = event.relatedTarget as Node | null;
    const container = event.currentTarget as HTMLElement;
    if (next && container.contains(next)) return;
    deactivate();
  }

  onDestroy(cancelPending);
</script>

<div class="combo-model-field target-model-field combo-picker" onfocusin={handleFocusIn} onfocusout={handleFocusOut}>
  <SelectSearch
    items={pickerItems}
    {value}
    ariaLabel={tr('Target model')}
    placeholder={providerId ? tr('Search saved models') : (disabledPlaceholder || tr('Choose a provider first.'))}
    disabled={!providerId}
    clearable={false}
    {noOptionsText}
    onSearch={searchModels}
    onValueChange={handleValueChange}
  />
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
