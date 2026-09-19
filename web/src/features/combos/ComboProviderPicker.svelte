<script lang="ts">
  import { afterUpdate } from 'svelte';
  import { Check, ChevronsUpDown } from '@lucide/svelte';
  import { Portal } from '@ark-ui/svelte/portal';
  import { Combobox, createListCollection } from '@ark-ui/svelte/combobox';
  import type { ComboProviderOption } from '../../lib/types';
  import type { Translate } from '../../lib/format';

  const MAX_VISIBLE_PROVIDERS = 30;

  export let providers: ComboProviderOption[];
  export let selectedProviderId: string;
  export let targetKey: number;
  export let tr: Translate;
  export let onProviderChange: (targetKey: number, providerId: string) => void;

  let inputValue = '';
  let syncedProviderId: string | null = null;

  $: normalizedQuery = inputValue.trim().toLocaleLowerCase();
  $: matchingProviders = providers.filter((provider) => provider.enabled && (
    !normalizedQuery
    || provider.name.toLocaleLowerCase().includes(normalizedQuery)
    || provider.id.toLocaleLowerCase().includes(normalizedQuery)
  ));
  $: visibleProviders = matchingProviders.slice(0, MAX_VISIBLE_PROVIDERS);
  $: collection = createListCollection({
    items: visibleProviders.map((provider) => ({ label: provider.name, value: provider.id })),
  });
  $: selectedValues = selectedProviderId ? [selectedProviderId] : [];

  afterUpdate(() => {
    if (selectedProviderId === syncedProviderId) return;
    syncedProviderId = selectedProviderId;
    inputValue = providers.find((provider) => provider.id === selectedProviderId)?.name ?? '';
  });

  function handleInputValueChange(details: { inputValue: string; reason?: string }): void {
    if (details.reason === 'interact-outside') {
      inputValue = providers.find((provider) => provider.id === selectedProviderId)?.name ?? '';
      return;
    }
    inputValue = details.inputValue;
  }

  function handleValueChange(details: { value: string[]; items: Array<{ label?: string; value?: string }> }): void {
    const providerId = details.value[0] ?? '';
    const provider = providers.find((option) => option.id === providerId);
    inputValue = provider?.name ?? details.items[0]?.label ?? '';
    onProviderChange(targetKey, providerId);
  }

  function handleOpenChange(details: { open: boolean; reason?: string }): void {
    const selectedName = providers.find((provider) => provider.id === selectedProviderId)?.name ?? '';
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
  class="combo-provider-picker"
>
  <Combobox.Label class="combo-field-label">{tr('Provider')}</Combobox.Label>
  <Combobox.Control class="combo-combobox-control">
    <Combobox.Input class="ark-field-input combo-combobox-input" placeholder={tr('Search providers by name or ID')} autocomplete="off" onfocus={(event) => event.currentTarget.select()} />
    <Combobox.Trigger class="combo-combobox-trigger" aria-label={tr('Search providers by name or ID')}>
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
            <Combobox.Empty class="ark-select-empty">{tr('No matching providers')}</Combobox.Empty>
          {/each}
        </Combobox.List>
        {#if matchingProviders.length > MAX_VISIBLE_PROVIDERS}
          <p class="combo-picker-limit-hint">{tr('Showing the first {count} provider matches. Type to narrow results.', { count: MAX_VISIBLE_PROVIDERS })}</p>
        {/if}
      </Combobox.Content>
    </Combobox.Positioner>
  </Portal>
</Combobox.Root>
