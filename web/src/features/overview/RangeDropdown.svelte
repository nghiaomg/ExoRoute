<script lang="ts">
  import { onDestroy } from 'svelte';
  import { Check, ChevronDown } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';
  interface Option { value: string; label: string; disabled?: boolean; hint?: string }
  interface Props { tr: Translate; value: string; options: ReadonlyArray<Option>; label: string; onChange: (value: string) => void; class?: string }
  let { tr, value, options, label, onChange, class: className = '' }: Props = $props();
  let open = $state(false);
  let focusIndex = $state(0);
  let root: HTMLDivElement | undefined = $state();
  const selected = $derived(options.find((o) => o.value === value) ?? options[0]);
  function toggle(): void { open = !open; if (open) focusIndex = Math.max(0, options.findIndex((o) => o.value === value)); }
  function choose(option: Option): void { if (option.disabled) return; onChange(option.value); open = false; }
  function onKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') { open = false; return; }
    if (!open && (event.key === 'Enter' || event.key === ' ' || event.key === 'ArrowDown')) { event.preventDefault(); toggle(); return; }
    if (!open) return;
    if (event.key === 'ArrowDown') { event.preventDefault(); focusIndex = Math.min(options.length - 1, focusIndex + 1); }
    else if (event.key === 'ArrowUp') { event.preventDefault(); focusIndex = Math.max(0, focusIndex - 1); }
    else if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); choose(options[focusIndex]); }
  }
  function onWindowClick(event: MouseEvent): void { if (root && !root.contains(event.target as Node)) open = false; }
  $effect(() => { if (!open) return; window.addEventListener('click', onWindowClick); return () => window.removeEventListener('click', onWindowClick); });
  onDestroy(() => window.removeEventListener('click', onWindowClick));
</script>
<div class={`custom-select ${className}`} bind:this={root}>
  <span class="cs-label">{label}</span>
  <button type="button" class="cs-trigger" aria-haspopup="listbox" aria-expanded={open} aria-label={`${label}: ${selected?.label ?? ''}`} onclick={toggle} onkeydown={onKeydown}>
    <span>{selected?.label ?? '—'}</span><ChevronDown size={13} class={open ? 'rotated' : ''} aria-hidden="true" />
  </button>
  {#if open}
    <ul class="cs-menu" role="listbox" aria-label={label}>
      {#each options as option, index (option.value)}
        <li role="option" aria-selected={option.value === value} aria-disabled={option.disabled} class:disabled={option.disabled} class:highlight={index === focusIndex}>
          <button type="button" disabled={option.disabled} title={option.hint ?? option.label} onclick={() => choose(option)} onmouseenter={() => (focusIndex = index)}>
            <span>{option.label}</span>
            {#if option.disabled}<span class="cs-hint">{tr('Unavailable')}</span>{:else if option.value === value}<Check size={13} aria-hidden="true" />{/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>
<style>
  .custom-select { position:relative; display:inline-flex; align-items:center; gap:6px; min-width:0; }
  .cs-label { color:var(--muted); font-size:11px; white-space:nowrap; }
  .cs-trigger { display:inline-flex; align-items:center; gap:6px; padding:4px 9px; border:1px solid var(--line); border-radius:8px; background:var(--surface-sunken); color:var(--ink); font-size:11px; cursor:pointer; }
  .cs-trigger:hover { border-color:var(--accent); }
  .cs-trigger:focus-visible, .cs-menu button:focus-visible { outline:2px solid var(--accent); outline-offset:2px; }
  .cs-trigger :global(svg) { transition:transform .15s ease; }
  .cs-trigger :global(svg.rotated) { transform:rotate(180deg); }
  .cs-menu { position:absolute; top:calc(100% + 4px); left:0; z-index:40; min-width:150px; margin:0; padding:4px; list-style:none; border:1px solid var(--line); border-radius:10px; background:var(--surface); }
  .cs-menu button { display:flex; align-items:center; justify-content:space-between; gap:10px; width:100%; padding:6px 9px; border:0; border-radius:7px; background:transparent; color:var(--ink); font-size:11px; text-align:left; cursor:pointer; }
  .cs-menu li.highlight button { background:var(--surface-sunken); }
  .cs-menu li.disabled button { color:var(--muted); cursor:not-allowed; }
  .cs-menu button:disabled { cursor:not-allowed; }
  .cs-hint { font-size:10px; color:var(--muted); }
</style>
