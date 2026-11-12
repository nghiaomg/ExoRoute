<script lang="ts">
  import SelectSearch from '../../components/SelectSearch.svelte';
  import type { ComboProviderOption } from '../../lib/types';
  import type { Translate } from '../../lib/format';

  const MAX_VISIBLE_PROVIDERS = 30;

  export let providers: ComboProviderOption[];
  export let selectedProviderId: string;
  export let targetKey: number;
  export let tr: Translate;
  export let onProviderChange: (targetKey: number, providerId: string) => void;

  // SelectSearch reports each open ('' query) and keystroke; the same
  // name-or-id match and the 30-result cap of the former Ark combobox carry
  // over, with the limit hint rendered below the field.
  let searchQuery = '';

  $: normalizedQuery = searchQuery.trim().toLocaleLowerCase();
  $: matchingProviders = providers.filter((provider) => provider.enabled && (
    !normalizedQuery
    || provider.name.toLocaleLowerCase().includes(normalizedQuery)
    || provider.id.toLocaleLowerCase().includes(normalizedQuery)
  ));
  $: pickerItems = matchingProviders.slice(0, MAX_VISIBLE_PROVIDERS).map((provider) => ({
    label: provider.name,
    value: provider.id,
    keywords: provider.id,
  }));

  function handleSearch(query: string): void {
    searchQuery = query;
  }

  function handleValueChange(providerId: string): void {
    if (providerId) onProviderChange(targetKey, providerId);
  }
</script>

<div class="combo-provider-picker combo-picker">
  <SelectSearch
    items={pickerItems}
    value={selectedProviderId}
    label={tr('Provider')}
    ariaLabel={tr('Search providers by name or ID')}
    placeholder={tr('Search providers by name or ID')}
    clearable={false}
    noOptionsText={tr('No matching providers')}
    onSearch={handleSearch}
    onValueChange={handleValueChange}
  />
  {#if matchingProviders.length > MAX_VISIBLE_PROVIDERS}
    <p class="combo-picker-limit-hint">{tr('Showing the first {count} provider matches. Type to narrow results.', { count: MAX_VISIBLE_PROVIDERS })}</p>
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
