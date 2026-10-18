<script lang="ts" context="module">
  export interface ComboboxOption {
    label: string;
    value: string;
    /** Short muted suffix rendered after the label, e.g. the provider prefix. */
    hint?: string;
    /** Extra text the filter also matches, e.g. the full id behind a short label. */
    keywords?: string;
  }
</script>

<script lang="ts">
  import { Portal } from '@ark-ui/svelte/portal';
  import { Combobox, createListCollection } from '@ark-ui/svelte/combobox';
  import { Check, ChevronsUpDown, X } from '@lucide/svelte';

  interface Props {
    items: ComboboxOption[];
    value?: string;
    placeholder?: string;
    label?: string;
    disabled?: boolean;
    /** Compact pill trigger styling; the input stays the searchable field. */
    pill?: boolean;
    /** Accessible name for the pill input when no visible label is shown. */
    ariaLabel?: string;
    /** Show the clear affordance next to the chevron; off when a value is required. */
    clearable?: boolean;
    /** Open the list on click as well as on typing; Ark leaves this off. */
    openOnClick?: boolean;
    /** Text shown when the filter matches nothing. */
    noOptionsText?: string;
    /**
     * Search the caller's own data source instead of filtering `items` here.
     * Called as the user types and with an empty query when the dropdown opens,
     * so `items` must already hold the matches for the reported query.
     */
    onSearch?: (query: string) => void;
    class?: string;
    onValueChange?: (value: string) => void;
  }

  export let items: ComboboxOption[] = [];
  export let value = '';
  export let placeholder = 'Select…';
  export let label = '';
  export let disabled = false;
  export let pill = false;
  export let ariaLabel = '';
  export let clearable = true;
  export let openOnClick = true;
  export let noOptionsText = 'No options';
  // `class` is a reserved word: declare an alias and re-export it under the
  // attribute name, the standard legacy-Svelte rename pattern.
  export let className = '';
  export { className as class };
  export let onValueChange: ((value: string) => void) | undefined = undefined;
  export let onSearch: ((query: string) => void) | undefined = undefined;

  // Ark performs no filtering on its own: the dropdown renders whatever the
  // caller's collection holds, so the search text narrows it here. The hint
  // and keywords stay searchable even when only the short label is shown.
  // With `onSearch` the caller owns the filtering and `items` is passed through.
  let filterQuery = '';

  function searchText(item: ComboboxOption): string {
    return `${item.label} ${item.hint ?? ''} ${item.keywords ?? ''}`.toLowerCase();
  }

  $: query = filterQuery.trim().toLowerCase();
  $: filteredItems = onSearch || !query ? items : items.filter((item) => searchText(item).includes(query));

  $: collection = createListCollection({
    items: filteredItems,
    itemToString: (item: ComboboxOption) => item.label,
    itemToValue: (item: ComboboxOption) => item.value,
  });

  function handleInputValueChange(details: { inputValue: string; reason?: string }): void {
    filterQuery = details.inputValue ?? '';
    // Only typing is a search: selecting or clearing an item also rewrites the
    // input text, and searching for the selected label would be meaningless.
    if (onSearch && details.reason === 'input-change') onSearch(filterQuery);
  }

  // Reopening shows the full list again; the input keeps the selected label as
  // plain text the user can replace.
  function handleOpenChange(details: { open: boolean }): void {
    if (details.open) {
      filterQuery = '';
      onSearch?.('');
    }
  }

  function handleValueChange(details: { value: string[] }): void {
    const next = details.value[0] ?? '';
    if (next !== value) {
      value = next;
      onValueChange?.(next);
    }
  }

  function clearSelection(event: MouseEvent): void {
    event.preventDefault();
    filterQuery = '';
    value = '';
    onValueChange?.('');
  }
</script>

<div
  class={`ark-combobox-root ${pill ? 'ark-combobox-pill' : ''} ${!clearable ? 'ark-combobox-no-clear' : ''} ${className}`}
>
  <Combobox.Root
    {collection}
    value={value ? [value] : []}
    onValueChange={handleValueChange}
    onInputValueChange={handleInputValueChange}
    onOpenChange={handleOpenChange}
    {disabled}
    inputBehavior="autohighlight"
    selectionBehavior="replace"
    {openOnClick}
    positioning={{ sameWidth: true, fitViewport: true }}
    lazyMount
    unmountOnExit
    class="ark-combobox-body"
  >
    <!-- The label must sit inside the root: it reads the combobox from Ark's context. -->
    {#if label && !pill}
      <Combobox.Label class="ark-combobox-label">{label}</Combobox.Label>
    {/if}
    <Combobox.Control class="ark-combobox-control">
      <Combobox.Input
        class="ark-combobox-input"
        {placeholder}
        aria-label={pill ? ariaLabel || label || placeholder : undefined}
      />
      <div class="ark-combobox-indicators">
        {#if value && clearable}
          <button
            class="ark-combobox-clear"
            type="button"
            aria-label="Clear selection"
            on:click={clearSelection}
          >
            <X size={13} />
          </button>
        {/if}
        <span class="ark-combobox-chevron" aria-hidden="true">
          <ChevronsUpDown size={14} />
        </span>
      </div>
    </Combobox.Control>
    <Portal>
      <Combobox.Positioner class="ark-combobox-positioner">
        <Combobox.Content class="ark-combobox-content">
          <Combobox.Empty>
            <div class="ark-combobox-empty">{noOptionsText}</div>
          </Combobox.Empty>
          {#each collection.items as item (item.value)}
            <Combobox.Item
              class="ark-combobox-item"
              item={item}
              title={item.value === item.label ? undefined : item.value}
            >
              <Combobox.ItemText class="ark-combobox-item-text">{item.label}</Combobox.ItemText>
              {#if item.hint}<span class="ark-combobox-item-hint">{item.hint}</span>{/if}
              <Combobox.ItemIndicator class="ark-combobox-item-indicator">
                <Check size={14} />
              </Combobox.ItemIndicator>
            </Combobox.Item>
          {/each}
        </Combobox.Content>
      </Combobox.Positioner>
    </Portal>
  </Combobox.Root>
</div>
