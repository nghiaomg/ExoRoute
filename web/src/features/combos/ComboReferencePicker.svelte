<script lang="ts">
  import { afterUpdate } from 'svelte';
  import { Check, ChevronsUpDown } from '@lucide/svelte';
  import { Portal } from '@ark-ui/svelte/portal';
  import { Combobox, createListCollection } from '@ark-ui/svelte/combobox';
  import type { GatewayCombo } from '../../lib/types';
  import type { Translate } from '../../lib/format';

  const MAX_VISIBLE_COMBOS = 30;

  /** Combos this row may nest; the caller excludes the edited combo and its descendants. */
  export let combos: GatewayCombo[];
  export let selectedComboId: string;
  export let targetKey: number;
  export let tr: Translate;
  export let onComboChange: (targetKey: number, comboId: string) => void;

  let inputValue = '';
  let syncedComboId: string | null = null;

  $: normalizedQuery = inputValue.trim().toLocaleLowerCase();
  $: matchingCombos = combos.filter((combo) =>
    !normalizedQuery
    || combo.name.toLocaleLowerCase().includes(normalizedQuery)
    || combo.id.toLocaleLowerCase().includes(normalizedQuery));
  $: visibleCombos = matchingCombos.slice(0, MAX_VISIBLE_COMBOS);
  $: collection = createListCollection({
    items: visibleCombos.map((combo) => ({ label: combo.name, value: combo.id })),
  });
  $: selectedValues = selectedComboId ? [selectedComboId] : [];

  afterUpdate(() => {
    if (selectedComboId === syncedComboId) return;
    syncedComboId = selectedComboId;
    inputValue = combos.find((combo) => combo.id === selectedComboId)?.name ?? '';
  });

  function handleInputValueChange(details: { inputValue: string; reason?: string }): void {
    if (details.reason === 'interact-outside') {
      inputValue = combos.find((combo) => combo.id === selectedComboId)?.name ?? '';
      return;
    }
    inputValue = details.inputValue;
  }

  function handleValueChange(details: { value: string[]; items: Array<{ label?: string; value?: string }> }): void {
    const comboId = details.value[0] ?? '';
    const combo = combos.find((option) => option.id === comboId);
    inputValue = combo?.name ?? details.items[0]?.label ?? '';
    onComboChange(targetKey, comboId);
  }

  function handleOpenChange(details: { open: boolean }): void {
    const selectedName = combos.find((combo) => combo.id === selectedComboId)?.name ?? '';
    if (details.open && inputValue === selectedName) {
      inputValue = '';
    } else if (!details.open && !inputValue) {
      inputValue = selectedName;
    }
  }
</script>

<Combobox.Root
  {collection}
  value={selectedValues}
  {inputValue}
  onInputValueChange={handleInputValueChange}
  onValueChange={handleValueChange}
  onOpenChange={handleOpenChange}
  openOnClick
  closeOnSelect
  lazyMount
  unmountOnExit
  positioning={{ sameWidth: true, fitViewport: true }}
  class="combo-reference-picker"
>
  <Combobox.Label class="combo-field-label">{tr('Combo target')}</Combobox.Label>
  <Combobox.Control class="combo-combobox-control">
    <Combobox.Input
      class="ark-field-input combo-combobox-input"
      placeholder={tr('Search combos by name or ID')}
      autocomplete="off"
      onfocus={(event) => event.currentTarget.select()}
    />
    <Combobox.Trigger class="combo-combobox-trigger" aria-label={tr('Search combos by name or ID')}>
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
            <Combobox.Empty class="ark-select-empty">{tr('No matching combos')}</Combobox.Empty>
          {/each}
        </Combobox.List>
        {#if matchingCombos.length > MAX_VISIBLE_COMBOS}
          <p class="combo-picker-limit-hint">{tr('Showing the first {count} provider matches. Type to narrow results.', { count: MAX_VISIBLE_COMBOS })}</p>
        {/if}
      </Combobox.Content>
    </Combobox.Positioner>
  </Portal>
</Combobox.Root>
