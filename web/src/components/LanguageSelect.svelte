<script lang="ts">
  import { Portal } from '@ark-ui/svelte/portal';
  import { Select, createListCollection } from '@ark-ui/svelte/select';
  import { Check, ChevronsUpDown, Languages } from '@lucide/svelte';
  import { localeOptions, type Locale } from '../lib/i18n';
  import type { Translate } from '../lib/format';

  export let locale: Locale;
  export let tr: Translate;
  export let onLocaleChange: (locale: Locale) => void;

  const languages = createListCollection({
    items: localeOptions as unknown as Array<{ value: Locale; label: string }>,
  });

  $: selectedValues = [locale];

  function handleValueChange(details: { value: string[] }): void {
    const candidate = details.value[0];
    const next = localeOptions.some((option) => option.value === candidate)
      ? (candidate as Locale)
      : undefined;
    if (next && next !== locale) {
      onLocaleChange(next);
    }
  }
</script>

<Select.Root
  class="language-select-root"
  collection={languages}
  value={selectedValues}
  onValueChange={handleValueChange}
  positioning={{ placement: 'bottom-end', sameWidth: false, gutter: 6, fitViewport: true }}
  lazyMount
  unmountOnExit
>
  <Select.Control class="language-select-control">
    <Select.Trigger class="language-select-trigger" aria-label={tr('Language')} title={tr('Language')}>
      <Languages size={14} class="language-select-icon" aria-hidden="true" />
      <Select.ValueText class="language-select-value-text" placeholder={tr('Language')} />
      <span class="language-select-indicators">
        <Select.Indicator class="language-select-indicator">
          <ChevronsUpDown size={12} />
        </Select.Indicator>
      </span>
    </Select.Trigger>
  </Select.Control>
  <Portal>
    <Select.Positioner class="language-select-positioner">
      <Select.Content class="language-select-content">
        <Select.ItemGroup class="language-select-item-group">
          <Select.ItemGroupLabel class="language-select-item-group-label">{tr('Language')}</Select.ItemGroupLabel>
          {#each languages.items as item (item.value)}
            <Select.Item class="language-select-item" {item}>
              <Select.ItemText class="language-select-item-text">{item.label}</Select.ItemText>
              <Select.ItemIndicator class="language-select-item-indicator">
                <Check size={14} />
              </Select.ItemIndicator>
            </Select.Item>
          {/each}
        </Select.ItemGroup>
      </Select.Content>
    </Select.Positioner>
  </Portal>
  <Select.HiddenSelect />
</Select.Root>
