<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Eye, EyeOff } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';
  import {
    countMaskedColumns,
    MASKABLE_COLUMNS,
    type MaskableRequestColumn,
    type MaskedColumnsState,
  } from './request.mask';

  export let tr: Translate;
  export let maskedColumns: MaskedColumnsState;
  export let onToggleColumn: (column: MaskableRequestColumn) => void;
  export let onMaskAll: () => void;
  export let onShowAll: () => void;

  let open = false;
  let menuRef: HTMLDivElement | null = null;

  $: maskedCount = countMaskedColumns(maskedColumns);
  $: isAnyMasked = maskedCount > 0;
  $: isAllMasked = maskedCount === MASKABLE_COLUMNS.length;

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

<div class="request-mask-container" bind:this={menuRef}>
  <button
    type="button"
    class="request-mask-trigger"
    class:active={isAnyMasked}
    class:open
    aria-haspopup="true"
    aria-expanded={open}
    title={tr('Mask columns')}
    onclick={toggleMenu}
  >
    {#if isAnyMasked}
      <EyeOff size={13} class="request-mask-icon" />
    {:else}
      <Eye size={13} class="request-mask-icon" />
    {/if}
    <span class="request-mask-label">{tr('Mask columns')}</span>
    {#if isAnyMasked}
      <span class="request-mask-badge">{maskedCount}</span>
    {/if}
  </button>

  {#if open}
    <div class="request-mask-dropdown" role="dialog" aria-label={tr('Mask columns')}>
      <div class="request-mask-header">
        <span class="request-mask-title">{tr('Mask column values')}</span>
        <p class="request-mask-subtitle">{tr('Hide sensitive names with asterisks (max 3 *)')}</p>
      </div>

      <div class="request-mask-options">
        {#each MASKABLE_COLUMNS as column (column.id)}
          <label class="request-mask-option" class:checked={maskedColumns[column.id]}>
            <input
              type="checkbox"
              checked={maskedColumns[column.id]}
              onchange={() => onToggleColumn(column.id)}
              class="request-mask-checkbox"
            />
            <span class="request-mask-option-title">{tr(column.labelKey)}</span>
            {#if maskedColumns[column.id]}
              <span class="request-mask-status-tag" aria-hidden="true">***</span>
            {/if}
          </label>
        {/each}
      </div>

      <div class="request-mask-actions">
        <button
          type="button"
          class="request-mask-action-btn"
          disabled={isAllMasked}
          onclick={onMaskAll}
        >
          {tr('Mask all')}
        </button>
        <button
          type="button"
          class="request-mask-action-btn"
          disabled={!isAnyMasked}
          onclick={onShowAll}
        >
          {tr('Show all')}
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .request-mask-container {
    position: relative;
    display: inline-block;
  }

  .request-mask-trigger {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 6px 13px;
    border: 2px solid var(--ink);
    border-radius: 999px;
    background: #ffffff;
    box-shadow: 2px 2px 0 var(--ink);
    color: #484c60;
    font-size: var(--text-2xs);
    font-weight: 700;
    cursor: pointer;
    transition: transform 0.14s ease, box-shadow 0.14s ease, background-color 0.14s ease, color 0.14s ease;
  }

  @media (hover: hover) {
    .request-mask-trigger:hover {
      transform: translate(-1px, -1px);
      box-shadow: 3px 3px 0 var(--ink);
    }
  }

  .request-mask-trigger:active,
  .request-mask-trigger.open {
    transform: translate(1px, 1px);
    box-shadow: 1px 1px 0 var(--ink);
  }

  .request-mask-trigger.active {
    background: #eff6ff;
    color: #1d4ed8;
  }

  :global(:root[data-theme='dark']) .request-mask-trigger {
    background: #181926;
    color: #cbd5e1;
    border-color: rgba(255, 255, 255, 0.15);
    box-shadow: 2px 2px 0 rgba(0, 0, 0, 0.6);
  }

  :global(:root[data-theme='dark']) .request-mask-trigger.active {
    background: #1e293b;
    color: #93c5fd;
    border-color: #3b82f6;
  }

  .request-mask-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 17px;
    height: 17px;
    padding: 0 4px;
    border-radius: 999px;
    background: #2563eb;
    color: #ffffff;
    font-size: 10px;
    font-weight: 800;
    line-height: 1;
  }

  :global(:root[data-theme='dark']) .request-mask-badge {
    background: #3b82f6;
  }

  .request-mask-dropdown {
    position: absolute;
    top: calc(100% + 8px);
    right: 0;
    z-index: 50;
    width: 270px;
    padding: 12px;
    border: 2px solid var(--ink);
    border-radius: 12px;
    background: #ffffff;
    box-shadow: 3px 3px 0 var(--ink);
    box-sizing: border-box;
    animation: request-mask-dropdown-in 0.12s cubic-bezier(0.16, 1, 0.3, 1);
  }

  @keyframes request-mask-dropdown-in {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  :global(:root[data-theme='dark']) .request-mask-dropdown {
    background: #181926;
    border-color: rgba(255, 255, 255, 0.15);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
  }

  .request-mask-header {
    padding-bottom: 8px;
    margin-bottom: 8px;
    border-bottom: 1px solid #e9eaf0;
  }

  :global(:root[data-theme='dark']) .request-mask-header {
    border-bottom-color: #282a3c;
  }

  .request-mask-title {
    display: block;
    font-size: 12px;
    font-weight: 700;
    color: #202330;
  }

  :global(:root[data-theme='dark']) .request-mask-title {
    color: #f1f3f9;
  }

  .request-mask-subtitle {
    margin: 3px 0 0;
    font-size: 11px;
    color: #85899b;
    line-height: 1.35;
  }

  :global(:root[data-theme='dark']) .request-mask-subtitle {
    color: #8e93aa;
  }

  .request-mask-options {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .request-mask-option {
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

  :global(:root[data-theme='dark']) .request-mask-option {
    color: #d1d5db;
  }

  .request-mask-option:hover {
    background: #f4f5f8;
  }

  :global(:root[data-theme='dark']) .request-mask-option:hover {
    background: #202235;
  }

  .request-mask-checkbox {
    width: 15px;
    height: 15px;
    cursor: pointer;
    accent-color: #2563eb;
    margin: 0;
  }

  .request-mask-option-title {
    flex: 1;
  }

  .request-mask-status-tag {
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 1px;
    color: #2563eb;
    background: #eff6ff;
    padding: 1px 5px;
    border-radius: 4px;
  }

  :global(:root[data-theme='dark']) .request-mask-status-tag {
    background: #1e293b;
    color: #93c5fd;
  }

  .request-mask-actions {
    display: flex;
    gap: 6px;
    margin-top: 10px;
    padding-top: 8px;
    border-top: 1px solid #e9eaf0;
  }

  :global(:root[data-theme='dark']) .request-mask-actions {
    border-top-color: #282a3c;
  }

  .request-mask-action-btn {
    flex: 1;
    padding: 5px 8px;
    border: 1px solid #d4d6e0;
    border-radius: 6px;
    background: #fafafa;
    color: #484c60;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    transition: background-color 0.1s ease, border-color 0.1s ease;
  }

  .request-mask-action-btn:hover:not(:disabled) {
    background: #f0f1f5;
    border-color: #b8bac7;
  }

  .request-mask-action-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  :global(:root[data-theme='dark']) .request-mask-action-btn {
    background: #1f2133;
    border-color: #2e3148;
    color: #9499b3;
  }

  :global(:root[data-theme='dark']) .request-mask-action-btn:hover:not(:disabled) {
    background: #272a40;
    border-color: #3f4362;
    color: #e2e4ef;
  }

  @media (max-width: 650px) {
    .request-mask-dropdown {
      right: 0;
      left: auto;
      width: calc(100vw - 32px);
      max-width: 290px;
    }
  }
</style>
