<script lang="ts">
  import { Portal } from '@ark-ui/svelte/portal';
  import { Select, createListCollection } from '@ark-ui/svelte/select';
  import { Check, ChevronDown, X } from '@lucide/svelte';

  export interface SelectOption { label: string; value: string; disabled?: boolean; }
  interface Props {
    items: SelectOption[];
    value?: string;
    placeholder?: string;
    label?: string;
    ariaLabel?: string;
    groupLabel?: string;
    disabled?: boolean;
    clearable?: boolean;
    clearLabel?: string;
    noOptionsText?: string;
    class?: string;
    portalContainer?: HTMLElement;
    onValueChange?: (value: string) => void;
  }
  let { items = [], value = $bindable(''), placeholder = 'Select…', label = '', ariaLabel = '', groupLabel = '', disabled = false, clearable = false, clearLabel = 'Clear selection', noOptionsText = 'No options', class: className = '', portalContainer, onValueChange }: Props = $props();
  let collection = $derived(createListCollection({ items }));
  function toInnerValue(current: string | undefined): string[] {
    if (current) return [current];
    return items.some((item) => item.value === '') && current === '' ? [''] : [];
  }
  let innerValue = $state<string[]>(toInnerValue(value));
  $effect(() => {
    const expected = toInnerValue(value);
    if (innerValue.length !== expected.length || innerValue[0] !== expected[0]) innerValue = expected;
  });
  function handleValueChange(details: { value: string[] }): void {
    innerValue = details.value;
    const next = details.value[0] ?? '';
    if (next !== value) { value = next; onValueChange?.(next); }
  }
</script>

<Select.Root {collection} bind:value={innerValue} onValueChange={handleValueChange} {disabled} class={'ark-select-root ' + className} positioning={{ sameWidth: true, fitViewport: true }} lazyMount unmountOnExit>
  {#if label}<Select.Label class="ark-select-label">{label}</Select.Label>{/if}
  <Select.Control class="ark-select-control">
    <Select.Trigger class="ark-select-trigger" aria-label={ariaLabel || undefined}>
      <Select.ValueText class="ark-select-value-text" {placeholder}>
        {#snippet children()}{items.find((item) => item.value === value)?.label || placeholder}{/snippet}
      </Select.ValueText>
    </Select.Trigger>
    <div class="ark-select-indicators">
      {#if clearable && value}<Select.ClearTrigger class="ark-select-clear-trigger" aria-label={clearLabel}><X size={14} /></Select.ClearTrigger>{/if}
      <Select.Indicator class="ark-select-indicator"><ChevronDown size={16} /></Select.Indicator>
    </div>
  </Select.Control>
  <Portal container={portalContainer}>
    <Select.Positioner class="ark-select-positioner">
      <Select.Content class="ark-select-content">
        <Select.ItemGroup class="ark-select-item-group">
          {#if groupLabel}<Select.ItemGroupLabel class="ark-select-item-group-label">{groupLabel}</Select.ItemGroupLabel>{/if}
          {#each collection.items as item (item.value)}
            <Select.Item class="ark-select-item" {item}>
              <Select.ItemText class="ark-select-item-text">{item.label}</Select.ItemText>
              <Select.ItemIndicator class="ark-select-item-indicator"><Check size={14} /></Select.ItemIndicator>
            </Select.Item>
          {:else}<div class="ark-select-empty">{noOptionsText}</div>{/each}
        </Select.ItemGroup>
      </Select.Content>
    </Select.Positioner>
  </Portal>
  <Select.HiddenSelect />
</Select.Root>
