<script lang="ts">
  import { Portal } from '@ark-ui/svelte/portal';
  import { Combobox, createListCollection } from '@ark-ui/svelte/combobox';
  import { ChevronsUpDown, X } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';
  import type { Provider } from '../../lib/types';
  import {
    addableScopeProviders,
    MAX_VISIBLE_SCOPE_PROVIDERS,
    parseModelRules,
    toggleProviderScope,
  } from './scope';

  export let tr: Translate;
  export let providers: Provider[] = [];
  export let selectedProviderIds: string[] = [];
  export let modelRules = '';
  export let loadingProviders = false;
  export let disabled = false;

  let query = '';

  $: providersById = new Map(providers.map((provider) => [provider.id, provider]));
  $: addable = addableScopeProviders(providers, selectedProviderIds, query);
  $: collection = createListCollection({
    items: addable.options.map((provider) => ({ label: provider.name, value: provider.id })),
  });
  $: ruleCount = parseModelRules(modelRules).length;

  function providerLabel(providerId: string): string {
    return providersById.get(providerId)?.name ?? providerId;
  }

  function toggleProvider(providerId: string, allowed: boolean): void {
    selectedProviderIds = toggleProviderScope(selectedProviderIds, providerId, allowed);
  }

  function handleInputValueChange(details: { inputValue: string }): void {
    query = details.inputValue ?? '';
  }

  // Adding a provider clears the field so the next one can be typed straight
  // away; the chip row is the record of what the key may use.
  function handleValueChange(details: { value: string[] }): void {
    const providerId = details.value[0] ?? '';
    query = '';
    if (providerId) toggleProvider(providerId, true);
  }
</script>

<div class="scope-fields">
  <div class="scope-field">
    <span class="scope-label">{tr('Allowed providers')}</span>
    <p class="scope-help">
      {#if selectedProviderIds.length}
        {tr('{count} allowed providers', { count: selectedProviderIds.length })}
      {:else}
        {tr('All providers')}
      {/if}
    </p>
    {#if loadingProviders}
      <p class="scope-help">{tr('Loading providers…')}</p>
    {:else if providers.length === 0}
      <p class="scope-help">{tr('No providers configured yet.')}</p>
    {:else}
      {#if selectedProviderIds.length}
        <ul class="scope-chips">
          {#each selectedProviderIds as providerId (providerId)}
            {@const known = providersById.get(providerId)}
            <li class="scope-chip" class:missing={!known}>
              <span class="scope-chip-name">{providerLabel(providerId)}</span>
              {#if !known}<code>{tr('Deleted')}</code>{/if}
              <button
                type="button"
                class="scope-chip-remove"
                aria-label={tr('Remove {name}', { name: providerLabel(providerId) })}
                title={tr('Remove {name}', { name: providerLabel(providerId) })}
                {disabled}
                onclick={() => toggleProvider(providerId, false)}
              >
                <X size={12} />
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      <Combobox.Root
        {collection}
        value={[]}
        inputValue={query}
        onInputValueChange={handleInputValueChange}
        onValueChange={handleValueChange}
        openOnClick
        openOnChange
        closeOnSelect
        inputBehavior="autohighlight"
        lazyMount
        unmountOnExit
        {disabled}
        positioning={{ sameWidth: true, fitViewport: true }}
        class="scope-add"
      >
        <Combobox.Control class="scope-add-control">
          <Combobox.Input
            class="ark-field-input scope-add-input"
            placeholder={tr('Search providers by name or ID')}
            aria-label={tr('Add a provider')}
            autocomplete="off"
          />
          <Combobox.Trigger class="scope-add-trigger" aria-label={tr('Search providers by name or ID')}>
            <ChevronsUpDown size={14} />
          </Combobox.Trigger>
        </Combobox.Control>
        <Portal>
          <Combobox.Positioner class="ark-select-positioner">
            <Combobox.Content class="ark-select-content">
              <Combobox.List class="ark-select-item-group">
                {#each collection.items as item (item.value)}
                  <Combobox.Item class="ark-select-item" {item}>
                    <Combobox.ItemText class="ark-select-item-text">{item.label}</Combobox.ItemText>
                  </Combobox.Item>
                {:else}
                  <Combobox.Empty class="ark-select-empty">{tr('No matching providers')}</Combobox.Empty>
                {/each}
              </Combobox.List>
              {#if addable.truncated}
                <p class="scope-limit-hint">
                  {tr('Showing the first {count} provider matches. Type to narrow results.', {
                    count: MAX_VISIBLE_SCOPE_PROVIDERS,
                  })}
                </p>
              {/if}
            </Combobox.Content>
          </Combobox.Positioner>
        </Portal>
      </Combobox.Root>
    {/if}
    <p class="scope-help">{tr('Leave empty to allow every provider.')}</p>
  </div>

  <div class="scope-field">
    <label class="scope-label" for="api-key-model-rules">{tr('Allowed models')}</label>
    <p class="scope-help">
      {#if ruleCount}
        {tr('{count} model rules', { count: ruleCount })}
      {:else}
        {tr('All models')}
      {/if}
    </p>
    <textarea
      id="api-key-model-rules"
      class="ark-field-input scope-rules"
      rows="4"
      placeholder={'gpt-4*\nclaude-3-5-sonnet'}
      bind:value={modelRules}
      {disabled}
    ></textarea>
    <p class="scope-help">
      {tr('One rule per line. A rule matches a model ID exactly; end it with * to match a prefix, for example gpt-4*.')}
    </p>
  </div>
</div>

<style>
  .scope-fields {
    display: grid;
    gap: 14px;
  }
  .scope-field {
    display: grid;
    gap: 6px;
    min-width: 0;
  }
  .scope-label {
    color: var(--ink);
    font: 600 12px var(--font-sans);
  }
  .scope-help {
    margin: 0;
    color: var(--muted);
    font-size: 12px;
    line-height: 1.45;
  }
  .scope-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .scope-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 100%;
    padding: 4px 5px 4px 9px;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--paper);
  }
  .scope-chip.missing {
    border-color: rgba(180, 83, 9, 0.35);
    background: rgba(249, 115, 22, 0.08);
  }
  .scope-chip-name {
    overflow: hidden;
    color: var(--ink);
    font: 500 12px var(--font-sans);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .scope-chip code {
    color: var(--muted);
    font: 600 10.5px var(--font-mono);
  }
  .scope-chip.missing code {
    color: #b45309;
  }
  .scope-chip-remove {
    display: grid;
    place-items: center;
    width: 19px;
    height: 19px;
    flex: 0 0 19px;
    padding: 0;
    color: var(--muted);
    border: 0;
    border-radius: 999px;
    background: transparent;
    cursor: pointer;
  }
  .scope-chip-remove:hover:not(:disabled) {
    color: #c2410c;
    background: rgba(249, 115, 22, 0.12);
  }
  .scope-chip-remove:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }
  /* Ark renders these parts itself, so the scoped compiler cannot see them. */
  :global(.scope-add) {
    display: grid;
    gap: 4px;
    min-width: 0;
  }
  :global(.scope-add-control) {
    position: relative;
    display: flex;
    align-items: center;
    min-width: 0;
  }
  :global(.scope-add-input) {
    height: 36px;
    padding-right: 32px;
    font-size: 12.5px;
  }
  :global(.scope-add-trigger) {
    position: absolute;
    right: 8px;
    display: grid;
    place-items: center;
    padding: 2px;
    color: var(--muted);
    border: 0;
    background: transparent;
    cursor: pointer;
  }
  :global(.scope-add-trigger:hover:not(:disabled)) {
    color: #c2410c;
  }
  .scope-limit-hint {
    margin: 2px 7px 4px;
    color: var(--muted);
    font-size: 12px;
    line-height: 1.4;
  }
  .scope-rules {
    height: auto;
    min-height: 84px;
    padding: 9px 11px;
    font-family: var(--font-mono);
    resize: vertical;
  }
</style>
