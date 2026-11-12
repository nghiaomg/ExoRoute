<script lang="ts">
  import SelectSearch from '../../components/SelectSearch.svelte';
  import type { GatewayCombo } from '../../lib/types';
  import type { Translate } from '../../lib/format';

  const MAX_VISIBLE_COMBOS = 30;

  /** Combos this row may nest; the caller excludes the edited combo and its descendants. */
  export let combos: GatewayCombo[];
  export let selectedComboId: string;
  export let targetKey: number;
  /** Compact rows render the picker without its field label. */
  export let hideLabel = false;
  /** Kept for source compatibility: the native panel renders in flow, so no portal is needed. */
  export let portalContainer: HTMLElement | undefined = undefined;
  void portalContainer;
  export let tr: Translate;
  export let onComboChange: (targetKey: number, comboId: string) => void;

  // SelectSearch reports each open ('' query) and keystroke; the same
  // name-or-id match and the 30-result cap of the former Ark combobox carry
  // over, with the limit hint rendered below the field.
  let searchQuery = '';

  $: normalizedQuery = searchQuery.trim().toLocaleLowerCase();
  $: matchingCombos = combos.filter((combo) =>
    !normalizedQuery
    || combo.name.toLocaleLowerCase().includes(normalizedQuery)
    || combo.id.toLocaleLowerCase().includes(normalizedQuery));
  $: pickerItems = matchingCombos.slice(0, MAX_VISIBLE_COMBOS).map((combo) => ({
    label: combo.name,
    value: combo.id,
    keywords: combo.id,
  }));

  function handleSearch(query: string): void {
    searchQuery = query;
  }

  function handleValueChange(comboId: string): void {
    if (comboId) onComboChange(targetKey, comboId);
  }
</script>

<div class="combo-reference-picker combo-picker">
  <SelectSearch
    items={pickerItems}
    value={selectedComboId}
    label={hideLabel ? '' : tr('Combo target')}
    ariaLabel={tr('Combo target')}
    placeholder={tr('Search combos by name or ID')}
    clearable={false}
    noOptionsText={tr('No matching combos')}
    onSearch={handleSearch}
    onValueChange={handleValueChange}
  />
  {#if matchingCombos.length > MAX_VISIBLE_COMBOS}
    <p class="combo-picker-limit-hint">{tr('Showing the first {count} provider matches. Type to narrow results.', { count: MAX_VISIBLE_COMBOS })}</p>
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
