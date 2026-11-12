<script lang="ts">
  import { X } from '@lucide/svelte';

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
    /** Kept for source compatibility: the native select renders in flow, so no portal is needed. */
    portalContainer?: HTMLElement;
    onValueChange?: (value: string) => void;
  }
  let { items = [], value = $bindable(''), placeholder = 'Select…', label = '', ariaLabel = '', groupLabel = '', disabled = false, clearable = false, clearLabel = 'Clear selection', noOptionsText = 'No options', class: className = '', portalContainer, onValueChange }: Props = $props();

  // Native select replacement for the former Ark Select. The leading empty
  // row mirrors the old toInnerValue empty-string semantics: an existing item
  // with an empty value doubles as the placeholder/clear row, picking the
  // empty row clears the selection, and it also keeps the select valid when
  // the current value matches no item.
  const emptyItem = $derived(items.find((item) => item.value === ''));
  const emptyOptionLabel = $derived(
    emptyItem?.label ?? placeholder ?? (clearable ? (clearLabel ?? noOptionsText ?? '—') : '—'),
  );

  function handleChange(event: Event & { currentTarget: EventTarget & HTMLSelectElement }): void {
    const next = event.currentTarget.value;
    // Read the event, not the reflected select state: a `bind:value` here
    // would update `value` before this handler runs and swallow the change.
    if (next !== value) { value = next; onValueChange?.(next); }
  }

  function clearSelection(): void {
    if (disabled) return;
    if (value !== '') { value = ''; onValueChange?.(''); }
  }
</script>

<div class="grid content-start gap-1.5">
  {#if label}<span class="text-xs font-semibold text-ink">{label}</span>{/if}
  <div class="relative flex w-full items-center">
    <select
      class={`min-h-[38px] w-full cursor-pointer appearance-none rounded-lg border border-line bg-db-tint py-1.5 pl-3 pr-9 text-dash-body text-ink outline-none transition-[border-color,box-shadow] duration-150 hover:border-text-dim! focus:border-accent! focus:ring-3! focus:ring-accent/15! disabled:cursor-default disabled:opacity-55 ${className}`}
      {disabled}
      aria-label={ariaLabel || label || undefined}
      value={value}
      onchange={handleChange}
    >
      <option value="">{emptyOptionLabel}</option>
      {#if groupLabel}
        <optgroup label={groupLabel}>
          {#each items as item (item.value)}
            {#if item !== emptyItem}<option value={item.value} disabled={item.disabled || undefined}>{item.label}</option>{/if}
          {/each}
        </optgroup>
      {:else}
        {#each items as item (item.value)}
          {#if item !== emptyItem}<option value={item.value} disabled={item.disabled || undefined}>{item.label}</option>{/if}
        {/each}
      {/if}
    </select>
    {#if clearable && value}
      <button
        type="button"
        class="grid shrink-0 cursor-pointer place-items-center border-0 bg-transparent p-1 text-text-dim hover:text-ink disabled:cursor-not-allowed disabled:opacity-55"
        aria-label={clearLabel}
        {disabled}
        onclick={clearSelection}
      >
        <X size={14} />
      </button>
    {/if}
  </div>
</div>
