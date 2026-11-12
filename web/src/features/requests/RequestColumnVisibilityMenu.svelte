<script lang="ts">
  import Button from "../../components/Button.svelte";
  import { onDestroy, onMount } from 'svelte';
  import { Eye, EyeOff } from '@lucide/svelte';
  import Input from '../../components/Input.svelte';
  import type { Translate } from '../../lib/format';
  import {
    countHiddenColumns,
    HIDEABLE_REQUEST_COLUMNS,
    type HideableRequestColumn,
    type HiddenRequestColumns,
  } from './request.visibility';

  export let tr: Translate;
  export let hiddenColumns: HiddenRequestColumns;
  export let onToggleColumn: (column: HideableRequestColumn) => void;

  let open = false;
  let menuRef: HTMLDivElement | null = null;

  $: hiddenCount = countHiddenColumns(hiddenColumns);
  $: isAnyHidden = hiddenCount > 0;

  function toggleMenu(): void {
    open = !open;
  }

  function handleDocumentClick(event: MouseEvent): void {
    if (!open || !menuRef) return;
    if (!menuRef.contains(event.target as Node)) {
      open = false;
    }
  }

  function handleDocumentKeydown(event: KeyboardEvent): void {
    if (!open) return;
    if (event.key === 'Escape') {
      open = false;
    }
  }

  onMount(() => {
    document.addEventListener('click', handleDocumentClick, true);
    document.addEventListener('keydown', handleDocumentKeydown);
  });

  onDestroy(() => {
    document.removeEventListener('click', handleDocumentClick, true);
    document.removeEventListener('keydown', handleDocumentKeydown);
  });
</script>

<div class="req-vis-container" bind:this={menuRef}>
  <Button
    type="button"
    class="req-vis-trigger" stateClasses={{ active: isAnyHidden, open: open }}
    
    
    aria-haspopup="true"
    aria-expanded={open}
    title={tr('Hide info')}
    onclick={toggleMenu}
  >
    {#if isAnyHidden}
      <EyeOff size={14} />
    {:else}
      <Eye size={14} />
    {/if}
    <span class="req-vis-label">{tr('Hide info')}</span>
    {#if isAnyHidden}
      <span class="req-vis-badge">{hiddenCount}</span>
    {/if}
  </Button>

  {#if open}
    <div class="req-vis-dropdown" role="dialog" aria-label={tr('Hide info')}>
      <div class="req-vis-header">
        <span class="req-vis-title">{tr('Hide info')}</span>
      </div>

      <div class="req-vis-options">
        {#each HIDEABLE_REQUEST_COLUMNS as column (column.id)}
          <label class="req-vis-option" class:checked={!hiddenColumns[column.id]}>
            <Input
              type="checkbox"
              checked={!hiddenColumns[column.id]}
              onchange={() => onToggleColumn(column.id)}
              class="req-vis-checkbox"
              ariaLabel={tr(column.labelKey)}
            />
            <span class="req-vis-option-title">{tr(column.labelKey)}</span>
          </label>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .req-vis-container {
    position: relative;
    display: inline-block;
    flex: 0 0 auto;
  }

  :global(.req-vis-trigger) {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 36px;
    min-height: 36px;
    padding: 0 12px;
    border: 1px solid #e9eef0;
    border-radius: 10px;
    background: #fff;
    color: #59646e;
    font-size: 13px;
    font-weight: var(--weight-medium);
    cursor: pointer;
    transition: background-color 0.14s ease, color 0.14s ease, border-color 0.14s ease;
  }

  :global(.req-vis-trigger:hover) {
    background: #f5f8f9;
  }

  :global(.req-vis-trigger.active) {
    background: color-mix(in srgb, var(--info) 10%, var(--surface));
    border-color: color-mix(in srgb, var(--info) 30%, var(--surface));
    color: var(--info);
  }

  :global(:root[data-theme='dark']) :global(.req-vis-trigger) {
    background: #1f2133;
    border-color: #2e3148;
    color: #9499b3;
  }

  :global(:root[data-theme='dark'] .req-vis-trigger:hover) {
    background: #272a40;
    color: #e2e4ef;
  }

  :global(:root[data-theme='dark'] .req-vis-trigger.active) {
    background: color-mix(in srgb, var(--info) 20%, #1f2133);
    border-color: color-mix(in srgb, var(--info) 35%, transparent);
    color: var(--info-soft);
  }

  /* The eye icon is rendered by the lucide child component, so the rule must
     reach it through :global. */
  :global(.req-vis-trigger) :global(svg) {
    flex: 0 0 auto;
  }

  .req-vis-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 17px;
    height: 17px;
    padding: 0 4px;
    border-radius: 999px;
    background: var(--info);
    color: #fff;
    font-size: 10px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }

  .req-vis-dropdown {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 40;
    min-width: 220px;
    padding: 12px;
    border: 1px solid #e9eaf0;
    border-radius: 12px;
    background: var(--surface);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.14);
  }

  :global(:root[data-theme='dark']) .req-vis-dropdown {
    background: #181926;
    border-color: rgba(255, 255, 255, 0.15);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
  }

  .req-vis-header {
    padding-bottom: 8px;
    margin-bottom: 8px;
    border-bottom: 1px solid #e9eaf0;
  }

  :global(:root[data-theme='dark']) .req-vis-header {
    border-bottom-color: #282a3c;
  }

  .req-vis-title {
    display: block;
    font-size: 12px;
    font-weight: 700;
    color: #202330;
  }

  :global(:root[data-theme='dark']) .req-vis-title {
    color: #f1f3f9;
  }

  .req-vis-options {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .req-vis-option {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border-radius: 8px;
    cursor: pointer;
    font-size: 12px;
    font-weight: 600;
    color: #33374b;
    user-select: none;
    transition: background-color 0.1s ease;
  }

  :global(:root[data-theme='dark']) .req-vis-option {
    color: #d1d5db;
  }

  .req-vis-option:hover {
    background: #f4f5f8;
  }

  :global(:root[data-theme='dark']) .req-vis-option:hover {
    background: #202235;
  }

  :global(.req-vis-checkbox) {
    width: 15px;
    height: 15px;
    cursor: pointer;
    accent-color: #2563eb;
    margin: 0;
  }

  .req-vis-option-title {
    flex: 1;
  }

  @media (max-width: 650px) {
    .req-vis-dropdown {
      left: auto;
      right: 0;
      width: calc(100vw - 32px);
      max-width: 290px;
    }
  }
</style>
