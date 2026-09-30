<script lang="ts" context="module">
  export interface ComboboxOption {
    label: string;
    value: string;
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
    /** Text shown when the filter matches nothing. */
    noOptionsText?: string;
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
  export let noOptionsText = 'No options';
  // `class` is a reserved word: declare an alias and re-export it under the
  // attribute name, the standard legacy-Svelte rename pattern.
  export let className = '';
  export { className as class };
  export let onValueChange: ((value: string) => void) | undefined = undefined;

  // Ark performs no filtering on its own: the dropdown renders whatever the
  // caller's collection holds, so the search text narrows it here.
  let filterQuery = '';

  $: filteredItems = filterQuery.trim().toLowerCase()
    ? items.filter((item) => item.label.toLowerCase().includes(filterQuery.trim().toLowerCase()))
    : items;

  $: collection = createListCollection({
    items: filteredItems,
    itemToString: (item: ComboboxOption) => item.label,
    itemToValue: (item: ComboboxOption) => item.value,
  });

  function handleInputValueChange(details: { inputValue: string }): void {
    filterQuery = details.inputValue ?? '';
  }

  // Reopening shows the full list again; the input keeps the selected label as
  // plain text the user can replace.
  function handleOpenChange(details: { open: boolean }): void {
    if (details.open) filterQuery = '';
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

<div class={`ark-combobox-root ${pill ? 'ark-combobox-pill' : ''} ${className}`}>
  {#if label && !pill}
    <Combobox.Label class="ark-combobox-label">{label}</Combobox.Label>
  {/if}
  <Combobox.Root
    {collection}
    value={value ? [value] : []}
    onValueChange={handleValueChange}
    onInputValueChange={handleInputValueChange}
    onOpenChange={handleOpenChange}
    {disabled}
    inputBehavior="autohighlight"
    selectionBehavior="replace"
    positioning={{ sameWidth: true, fitViewport: true }}
    lazyMount
    unmountOnExit
  >
    <Combobox.Control class="ark-combobox-control">
      <Combobox.Input
        class="ark-combobox-input"
        {placeholder}
        aria-label={pill ? ariaLabel || label || placeholder : undefined}
      />
      <div class="ark-combobox-indicators">
        {#if value}
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
            <Combobox.Item class="ark-combobox-item" item={item}>
              <Combobox.ItemText class="ark-combobox-item-text">{item.label}</Combobox.ItemText>
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
