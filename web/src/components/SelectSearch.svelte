<script lang="ts" module>
  export interface SelectSearchOption {
    label: string;
    value: string;
    /** Short muted suffix rendered after the label, e.g. the provider prefix. */
    hint?: string;
    /** Extra text the filter also matches, e.g. the full id behind a short label. */
    keywords?: string;
  }

  let instanceCounter = 0;
</script>

<script lang="ts">
  import { Check, ChevronDown, X } from '@lucide/svelte';
  import { onDestroy, onMount } from 'svelte';

  interface Props {
    items: SelectSearchOption[];
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
    clearLabel?: string;
    /** Open the list on click as well as on typing. */
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

  let {
    items = [],
    value = $bindable(''),
    placeholder = 'Select…',
    label = '',
    disabled = false,
    pill = false,
    ariaLabel = '',
    clearable = true,
    clearLabel = 'Clear selection',
    openOnClick = true,
    noOptionsText = 'No options',
    class: className = '',
    onValueChange,
    onSearch,
  }: Props = $props();

  const listboxId = `select-search-list-${++instanceCounter}`;
  const inputId = `${listboxId}-input`;

  let open = $state(false);
  let filterQuery = $state('');
  let activeIndex = $state(0);
  // Flip the panel above the input when there isn't ~260px of room below —
  // keeps it inside scrollable modals instead of clipping under the footer.
  let dropUp = $state(false);
  let containerRef: HTMLDivElement | null = null;
  let inputElement: HTMLInputElement | null = $state(null);

  function searchText(item: SelectSearchOption): string {
    return `${item.label} ${item.hint ?? ''} ${item.keywords ?? ''}`.toLowerCase();
  }

  const query = $derived(filterQuery.trim().toLowerCase());
  // The hint and keywords stay searchable even when only the short label is
  // shown. With `onSearch` the caller owns the filtering and `items` already
  // holds the matches for the reported query.
  const filteredItems = $derived(onSearch || !query ? items : items.filter((item) => searchText(item).includes(query)));
  const selectedItem = $derived(items.find((item) => item.value === value));

  // Keep the highlight inside the match list and keep the highlighted row visible.
  $effect(() => {
    if (activeIndex > filteredItems.length - 1) activeIndex = Math.max(0, filteredItems.length - 1);
    if (open) document.getElementById(`${listboxId}-${activeIndex}`)?.scrollIntoView?.({ block: 'nearest' });
  });

  // Opening moves focus into the search field (the display button hands over).
  $effect(() => {
    if (open) inputElement?.focus();
  });

  // Reopening shows the full list again and queries the caller's data source.
  function openPanel(resetQuery = true): void {
    if (disabled || open) return;
    open = true;
    if (resetQuery) filterQuery = '';
    activeIndex = Math.max(0, filteredItems.findIndex((item) => item.value === value));
    const rect = inputElement?.getBoundingClientRect();
    if (rect) {
      const spaceBelow = window.innerHeight - rect.bottom;
      const spaceAbove = rect.top;
      dropUp = spaceBelow < 260 && spaceAbove > spaceBelow;
    }
    onSearch?.('');
  }

  function closePanel(): void {
    if (!open) return;
    open = false;
    filterQuery = '';
  }

  function handleInput(event: Event & { currentTarget: EventTarget & HTMLInputElement }): void {
    filterQuery = event.currentTarget.value;
    // Typing always opens the panel (Ark's openOnChange parity) without
    // wiping the query the user just typed.
    if (!open) openPanel(false);
    // Only typing is a search: selecting or clearing an item also rewrites the
    // input text, and searching for the selected label would be meaningless.
    if (onSearch) onSearch(filterQuery);
  }

  function handleInputKeydown(event: KeyboardEvent): void {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      if (!open) { openPanel(); return; }
      activeIndex = Math.min(activeIndex + 1, filteredItems.length - 1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      if (!open) { openPanel(); return; }
      activeIndex = Math.max(activeIndex - 1, 0);
    } else if (event.key === 'Home') {
      event.preventDefault();
      activeIndex = 0;
    } else if (event.key === 'End') {
      event.preventDefault();
      activeIndex = Math.max(0, filteredItems.length - 1);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      if (!open) { openPanel(); return; }
      selectItem(filteredItems[activeIndex]);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      closePanel();
    } else if (event.key === 'Tab') {
      closePanel();
    }
  }

  function selectItem(item: SelectSearchOption | undefined): void {
    if (!item) return;
    open = false;
    filterQuery = '';
    if (item.value !== value) {
      value = item.value;
      onValueChange?.(item.value);
    }
  }

  function clearSelection(event: MouseEvent): void {
    event.preventDefault();
    if (disabled) return;
    filterQuery = '';
    value = '';
    onValueChange?.('');
  }

  // Capture phase: clicks anywhere outside the container close the panel,
  // and Escape closes it from any element with focus.
  function handleDocumentClick(event: MouseEvent): void {
    if (!open || !containerRef) return;
    if (!containerRef.contains(event.target as Node)) open = false;
  }

  function handleDocumentKeydown(event: KeyboardEvent): void {
    if (!open) return;
    if (event.key === 'Escape') open = false;
  }

  onMount(() => {
    document.addEventListener('click', handleDocumentClick, true);
    document.addEventListener('keydown', handleDocumentKeydown);
  });

  onDestroy(() => {
    document.removeEventListener('click', handleDocumentClick, true);
    document.removeEventListener('keydown', handleDocumentKeydown);
  });

  /* Native combobox input and compact pill styles follow the shared field
     treatment; the chat model bar keeps its compact 30px trigger face. */
  const fieldInputClass = 'min-h-[44px] w-full rounded-xl border border-line bg-surface py-3 pl-3.5 pr-[60px] text-sm text-ink outline-none transition-[border-color,box-shadow] duration-150 placeholder:text-text-dim hover:border-text-dim! focus:border-accent! focus:ring-3! focus:ring-accent/15! disabled:cursor-not-allowed disabled:opacity-55 disabled:bg-surface-sunken';
  const fieldButtonClass = 'flex min-h-[44px] w-full cursor-pointer items-center rounded-xl border border-line bg-surface px-3.5 text-left text-sm text-ink transition-[border-color,box-shadow] duration-150 hover:border-text-dim! focus:border-accent! focus:ring-3! focus:ring-accent/15! disabled:cursor-not-allowed disabled:opacity-55';
  const pillFrameClass = 'rounded-lg border border-line bg-surface text-xs font-medium text-ink outline-none transition-[border-color,box-shadow] duration-150 focus:border-accent! focus:ring-3! focus:ring-accent/15! disabled:cursor-not-allowed disabled:opacity-55 disabled:bg-surface-sunken';
  const pillPadClass = $derived(clearable ? 'pr-[52px]' : 'pr-[26px]');
  const pillInputClass = $derived(`${pillFrameClass} h-[30px] min-h-[30px] w-full py-0 pl-2.5 ${pillPadClass}`);
  const pillButtonClass = $derived(`${pillFrameClass} flex h-[30px] min-h-[30px] w-full cursor-pointer items-center px-2.5 text-left ${pillPadClass}`);
</script>

<div class={`relative grid w-full content-start gap-1.5 ${className}`} bind:this={containerRef}>
  {#if label && !pill}
    <label class="text-xs font-semibold text-ink" for={inputId}>{label}</label>
  {/if}

  <div class="relative flex w-full items-center">
    <!-- Always an input, like the old Ark Combobox trigger: when closed with a
        selection it displays the selected label; opening (click or typing)
        resets it to the live query. Tests target this placeholder/value. -->
    <input
      bind:this={inputElement}
      value={open ? filterQuery : (selectedItem ? selectedItem.label : filterQuery)}
      {placeholder}
      {disabled}
      type="text"
      id={inputId}
      role="combobox"
      aria-expanded={open}
      aria-controls={open ? listboxId : undefined}
      aria-autocomplete="list"
      aria-label={ariaLabel || label || placeholder}
      class={pill ? pillInputClass : fieldInputClass}
      onclick={openOnClick ? () => openPanel() : undefined}
      oninput={handleInput}
      onkeydown={handleInputKeydown}
    />

    <div class={`pointer-events-none absolute flex items-center gap-1 text-muted ${pill ? 'right-1.5' : 'right-2'}`}>
      {#if value && clearable}
        <button
          type="button"
          class="pointer-events-auto grid cursor-pointer place-items-center border-0 bg-transparent p-0 text-text-dim hover:text-ink disabled:cursor-not-allowed disabled:opacity-55"
          aria-label={clearLabel}
          {disabled}
          onclick={clearSelection}
        >
          <X size={13} />
        </button>
      {/if}
      <span class="grid place-items-center" aria-hidden="true">
        <ChevronDown size={pill ? 14 : 16} />
      </span>
    </div>
  </div>

  {#if open}
    <div
      role="listbox"
      id={listboxId}
      class={`absolute left-0 right-0 z-[99999] max-h-[240px] overflow-y-auto rounded-xl border border-line bg-surface-popover p-1 shadow-lg ${dropUp ? 'bottom-full mb-1' : 'top-full mt-1'}`}
    >
      {#each filteredItems as item, index (item.value)}
        <button
          type="button"
          role="option"
          id={`${listboxId}-${index}`}
          aria-selected={item.value === value}
          title={item.value === item.label ? undefined : item.value}
          class={`flex min-h-8 w-full cursor-pointer items-center justify-between gap-2 rounded-md px-3 py-1.5 text-left text-xs leading-snug text-ink transition-colors duration-100 hover:bg-surface-sunken ${index === activeIndex ? 'bg-surface-sunken' : ''} ${item.value === value ? 'bg-[color-mix(in_srgb,var(--accent)_8%,var(--surface))] font-semibold text-accent-strong' : ''}`}
          onmouseenter={() => (activeIndex = index)}
          onclick={() => selectItem(item)}
        >
          <span class="min-w-0 flex-1 truncate">{item.label}</span>
          {#if item.hint}<span class="max-w-[45%] shrink-0 truncate text-[10px] font-semibold text-muted">{item.hint}</span>{/if}
          {#if item.value === value}<Check size={14} class="shrink-0 text-accent" />{/if}
        </button>
      {:else}
        <div class="px-3 py-2 text-center text-xs text-text-dim">{noOptionsText}</div>
      {/each}
    </div>
  {/if}
</div>
