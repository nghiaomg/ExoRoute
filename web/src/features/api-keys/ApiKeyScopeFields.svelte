<script lang="ts">
  import Button from "../../components/Button.svelte";
  import { onDestroy, onMount } from 'svelte';
  import { ChevronsUpDown, X } from '@lucide/svelte';
  import Input from '../../components/Input.svelte';
  import type { Translate } from '../../lib/format';
  import type { Provider } from '../../lib/types';
  import {
    addableScopeProviders,
    MAX_VISIBLE_SCOPE_PROVIDERS,
    parseModelRules,
    toggleProviderScope,
  } from './scope';

  export let tr: Translate;
  export let providers: Array<Pick<Provider, 'id' | 'name'>> = [];
  export let selectedProviderIds: string[] = [];
  export let modelRules = '';
  export let modelRulesId = 'api-key-model-rules';
  export let loadingProviders = false;
  export let disabled = false;

  let query = '';
  let open = false;
  let activeIndex = 0;
  let containerRef: HTMLDivElement | null = null;
  let inputElement: HTMLInputElement | null = null;

  $: providersById = new Map(providers.map((provider) => [provider.id, provider]));
  $: addable = addableScopeProviders(providers, selectedProviderIds, query);
  $: ruleCount = parseModelRules(modelRules).length;
  // Keep the highlight inside the match list.
  $: activeIndex = Math.min(activeIndex, Math.max(0, addable.options.length - 1));

  function providerLabel(providerId: string): string {
    return providersById.get(providerId)?.name ?? providerId;
  }

  function toggleProvider(providerId: string, allowed: boolean): void {
    selectedProviderIds = toggleProviderScope(selectedProviderIds, providerId, allowed);
  }

  // Adding a provider clears the field so the next one can be typed straight
  // away; the chip row is the record of what the key may use.
  function addProvider(providerId: string): void {
    if (!providerId) return;
    query = '';
    open = false;
    toggleProvider(providerId, true);
    inputElement?.focus();
  }

  function handleInput(): void {
    open = true;
  }

  // Clicking the field shows the current matches, like the former
  // openOnClick combobox.
  function handleOpen(): void {
    if (!disabled) open = true;
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      if (!open) { open = true; return; }
      activeIndex = Math.min(activeIndex + 1, addable.options.length - 1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      if (!open) { open = true; return; }
      activeIndex = Math.max(activeIndex - 1, 0);
    } else if (event.key === 'Enter') {
      // The add field lives inside forms whose submit button must not fire.
      if (open) {
        event.preventDefault();
        const option = addable.options[activeIndex];
        if (option) addProvider(option.id);
      }
    } else if (event.key === 'Escape' && open) {
      // Swallow the key so hosting dialogs do not close with the panel.
      event.preventDefault();
      event.stopPropagation();
      open = false;
    }
  }

  // Capture phase: clicks anywhere outside the container close the panel,
  // and Escape closes it from any element with focus.
  function handleDocumentClick(event: MouseEvent): void {
    if (!open || !containerRef) return;
    if (!containerRef.contains(event.target as Node)) open = false;
  }

  function handleDocumentKeydown(event: KeyboardEvent): void {
    if (!open) return;
    if (event.key === 'Escape') open = false;
  }

  onMount(() => {
    document.addEventListener('click', handleDocumentClick, true);
    document.addEventListener('keydown', handleDocumentKeydown);
  });

  onDestroy(() => {
    document.removeEventListener('click', handleDocumentClick, true);
    document.removeEventListener('keydown', handleDocumentKeydown);
  });
</script>

<div class="scope-fields">
  <div class="scope-field api-key-scope-providers wide">
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
              <Button
                type="button"
                class="scope-chip-remove"
                aria-label={tr('Remove {name}', { name: providerLabel(providerId) })}
                title={tr('Remove {name}', { name: providerLabel(providerId) })}
                {disabled}
                onclick={() => toggleProvider(providerId, false)}
              >
                <X size={12} />
              </Button>
            </li>
          {/each}
        </ul>
      {/if}
      <div class="scope-add" bind:this={containerRef}>
        <div class="scope-add-control">
          <Input
            variant="combobox"
            class="scope-add-input"
            bind:element={inputElement}
            placeholder={tr('Search providers by name or ID')}
            ariaLabel={tr('Add a provider')}
            autocomplete="off"
            role="combobox"
            aria-expanded={open}
            aria-haspopup="listbox"
            {disabled}
            bind:value={query}
            oninput={handleInput}
            onclick={handleOpen}
            onkeydown={handleKeydown}
          />
          <button
            type="button"
            class="scope-add-trigger"
            aria-label={tr('Search providers by name or ID')}
            aria-expanded={open}
            {disabled}
            tabindex="-1"
            onclick={() => { open = !open; if (open) inputElement?.focus(); }}
          >
            <ChevronsUpDown size={14} />
          </button>
        </div>
        {#if open}
          <div class="scope-add-panel" role="listbox" aria-label={tr('Add a provider')}>
            {#each addable.options as option, index (option.id)}
              <button
                type="button"
                role="option"
                aria-selected={index === activeIndex}
                class="scope-add-option" class:active={index === activeIndex}
                onmousedown={(event) => event.preventDefault()}
                onclick={() => addProvider(option.id)}
                onmouseenter={() => (activeIndex = index)}
              >
                <span class="scope-add-option-name">{option.name}</span>
                <span class="scope-add-option-id">{option.id}</span>
              </button>
            {:else}
              <div class="scope-add-empty">{tr('No matching providers')}</div>
            {/each}
            {#if addable.truncated}
              <p class="scope-limit-hint">
                {tr('Showing the first {count} provider matches. Type to narrow results.', {
                  count: MAX_VISIBLE_SCOPE_PROVIDERS,
                })}
              </p>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
    <p class="scope-help">{tr('Leave empty to allow every provider.')}</p>
  </div>

  <div class="scope-field api-key-scope-models">
    <label class="scope-label" for={modelRulesId}>{tr('Allowed models')}</label>
    <p class="scope-help">{ruleCount ? tr('{count} model rules', { count: ruleCount }) : tr('All models')}</p>
    <textarea
      id={modelRulesId}
      class="scope-rules"
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
  :global(.api-key-create-scope .scope-fields) { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); }
  .api-key-scope-providers.wide,
  .api-key-scope-models { grid-column: 1 / -1; }
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
  /* Ink-framed chip, matching `.strategy-badge` / `.protocol-pill`. Dark mode
     flattens the frame globally (styles/dark-mode.css). */
  .scope-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 100%;
    padding: 4px 5px 4px 9px;
    border: var(--nb-border);
    border-radius: 999px;
    background: var(--paper);
  }
  .scope-chip.missing {
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
  :global(.scope-chip-remove) {
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
  :global(.scope-chip-remove:hover:not(:disabled)) {
    color: #c2410c;
    background: rgba(249, 115, 22, 0.12);
  }
  :global(.scope-chip-remove:disabled) {
    cursor: not-allowed;
    opacity: 0.5;
  }
  .scope-add {
    position: relative;
    display: grid;
    gap: 4px;
    min-width: 0;
  }
  .scope-add-control {
    position: relative;
    display: flex;
    align-items: center;
    min-width: 0;
  }
  .scope-add :global(.scope-add-input) {
    height: 36px;
    padding-right: 32px;
    font-size: 12.5px;
  }
  .scope-add-trigger {
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
  .scope-add-trigger:hover:not(:disabled) {
    color: #c2410c;
  }
  /* In-flow suggestion panel; anchors to the add field's own wrapper. */
  .scope-add-panel {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    z-index: 99999;
    max-height: 240px;
    margin-top: 4px;
    padding: 4px;
    overflow-y: auto;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--surface-popover, var(--surface));
    box-shadow: 0 10px 24px rgb(0 0 0 / 0.14);
  }
  .scope-add-option {
    display: flex;
    min-height: 32px;
    width: 100%;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 10px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--ink);
    font-size: 12px;
    line-height: 1.3;
    text-align: left;
    cursor: pointer;
  }
  .scope-add-option.active,
  .scope-add-option:hover {
    background: var(--surface-sunken);
  }
  .scope-add-option-name {
    overflow: hidden;
    min-width: 0;
    flex: 1 1 auto;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .scope-add-option-id {
    overflow: hidden;
    max-width: 45%;
    flex: 0 0 auto;
    color: var(--muted);
    font-size: 10px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .scope-add-empty {
    padding: 10px;
    color: var(--muted);
    font-size: 12.5px;
    text-align: center;
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
