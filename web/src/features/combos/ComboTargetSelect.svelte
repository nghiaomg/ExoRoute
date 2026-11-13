<script lang="ts">
  import SelectSearch from '../../components/SelectSearch.svelte';
  import type { ComboProviderOption, GatewayCombo } from '../../lib/types';
  import type { Translate } from '../../lib/format';

  const MAX_VISIBLE_TARGETS = 40;

  export let providers: ComboProviderOption[];
  /** Combos this row may nest; the caller excludes the edited combo and its descendants. */
  export let combos: GatewayCombo[];
  export let kind: '' | 'provider' | 'combo';
  export let providerId: string;
  export let comboId: string;
  export let targetKey: number;
  export let tr: Translate;
  export let onTargetChange: (
    targetKey: number,
    kind: 'provider' | 'combo',
    providerId: string,
    comboId: string,
  ) => void;

  // One searchable field replaces the former kind select plus provider select:
  // picking a provider makes the row a provider target, picking a combo entry
  // (hinted "combo") makes it a nested-combo target.
  let searchQuery = '';

  interface TargetItem {
    label: string;
    value: string;
    keywords?: string;
    hint?: string;
  }

  const PROVIDER_PREFIX = 'provider:';
  const COMBO_PREFIX = 'combo:';

  $: normalizedQuery = searchQuery.trim().toLocaleLowerCase();
  $: providerItems = providers
    .filter((provider) => provider.enabled)
    .map((provider) => ({
      label: provider.name,
      value: `${PROVIDER_PREFIX}${provider.id}`,
      keywords: provider.id,
    }));
  // Keep a disabled provider that the row already references visible, so the
  // selection never displays as an empty field after a provider is disabled.
  $: disabledSelectedItem = kind === 'provider' && providerId && providers.some((provider) => provider.id === providerId && !provider.enabled)
    ? [{
        label: `${providers.find((provider) => provider.id === providerId)?.name ?? providerId} (${tr('disabled')})`,
        value: `${PROVIDER_PREFIX}${providerId}`,
        keywords: providerId,
      }]
    : [];
  $: comboItems = combos.map((combo) => ({
    label: combo.name,
    value: `${COMBO_PREFIX}${combo.id}`,
    keywords: combo.id,
    hint: tr('combo'),
  }));
  $: allItems = [...providerItems, ...disabledSelectedItem, ...comboItems];
  $: matchingItems = allItems.filter((item) =>
    !normalizedQuery
    || item.label.toLocaleLowerCase().includes(normalizedQuery)
    || (item.keywords ?? '').toLocaleLowerCase().includes(normalizedQuery));
  $: pickerItems = matchingItems.slice(0, MAX_VISIBLE_TARGETS);
  $: selectedValue = kind === 'combo' && comboId
    ? `${COMBO_PREFIX}${comboId}`
    : kind === 'provider' && providerId
      ? `${PROVIDER_PREFIX}${providerId}`
      : '';

  function handleSearch(query: string): void {
    searchQuery = query;
  }

  function handleValueChange(value: string): void {
    if (value.startsWith(COMBO_PREFIX)) {
      onTargetChange(targetKey, 'combo', '', value.slice(COMBO_PREFIX.length));
    } else if (value.startsWith(PROVIDER_PREFIX)) {
      onTargetChange(targetKey, 'provider', value.slice(PROVIDER_PREFIX.length), '');
    }
  }
</script>

<div class="combo-target-select combo-picker">
  <SelectSearch
    items={pickerItems}
    value={selectedValue}
    ariaLabel={tr('Search providers and combos')}
    placeholder={tr('Search providers and combos')}
    clearable={false}
    noOptionsText={tr('No matching providers or combos')}
    onSearch={handleSearch}
    onValueChange={handleValueChange}
  />
  {#if matchingItems.length > MAX_VISIBLE_TARGETS}
    <p class="combo-picker-limit-hint">{tr('Showing the first {count} provider matches. Type to narrow results.', { count: MAX_VISIBLE_TARGETS })}</p>
  {/if}
</div>

<style>
  /* Keep the compact combo-row face of the neighbouring pickers while
     rendering through the shared SelectSearch control. */
  .combo-picker :global(input),
  .combo-picker :global(button[aria-haspopup='listbox']) {
    height: 32px;
    min-height: 32px;
    padding-top: 0;
    padding-bottom: 0;
    padding-left: 10px;
    padding-right: 32px;
    border-radius: 7px;
    font-size: 12px;
  }
</style>
