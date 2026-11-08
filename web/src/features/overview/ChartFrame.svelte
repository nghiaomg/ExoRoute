<script lang="ts">
  import { Activity, ListOrdered } from '@lucide/svelte';
  import type { Translate } from '../../lib/format';

  export let title: string;
  export let tr: Translate;
  /** Short line under the title. Used to state what the numbers cover. */
  export let caption: string | undefined = undefined;
  /** The bounded scan dropped rows, so the chart understates the range. */
  export let truncated = false;
  /** Some requests in the group reported no cost or usage. */
  export let partial = false;
  export let empty = false;
  export let emptyText: string;
  /** Shown under the chart when `partial` is set. */
  export let partialText = '';

  let view: 'chart' | 'table' = 'chart';
</script>

<!--
  Chart chrome shared by every panel on this page: title, coverage caption,
  completeness badges, and the chart/table toggle. The table twin is a slot
  rather than an option so a chart cannot ship without one.
-->
<section class="chart-card" aria-label={title}>
  <header class="chart-head">
    <div class="chart-titles">
      <h2>{title}</h2>
      {#if caption}<p>{caption}</p>{/if}
    </div>
    <div class="chart-tools">
      {#if truncated}<span class="chart-badge">{tr('Truncated')}</span>{/if}
      {#if partial}<span class="chart-badge partial" title={partialText}>{tr('Partial data')}</span>{/if}
      {#if $$slots.table && !empty}
        <button
          class="secondary-button compact"
          aria-pressed={view === 'table'}
          onclick={() => (view = view === 'chart' ? 'table' : 'chart')}
        >
          {#if view === 'chart'}<ListOrdered size={13} />{tr('Show table')}{:else}<Activity size={13} />{tr('Show chart')}{/if}
        </button>
      {/if}
    </div>
  </header>
  {#if empty}
    <p class="chart-empty">{emptyText}</p>
  {:else if view === 'table'}
    <slot name="table" />
  {:else}
    <slot name="chart" />
  {/if}
  {#if partial && !empty}
    <p class="chart-note">{partialText}</p>
  {/if}
</section>

<style>
  .chart-card { min-width: 0; padding: 18px 20px 20px; border: var(--nb-border); border-radius: var(--nb-radius); background: var(--paper); color: var(--ink); box-shadow: var(--nb-shadow-md); }
  .chart-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; margin-bottom: 14px; }
  .chart-titles { min-width: 0; }
  .chart-titles h2 { margin: 0; color: var(--ink); font: 650 15px/1.35 var(--font-sans); }
  .chart-titles p { margin: 3px 0 0; color: var(--muted); font-size: 12px; line-height: 1.45; }
  .chart-tools { flex: 0 0 auto; display: flex; align-items: center; gap: 7px; }
  /* Accent-token mixes: one rule covers light, dark and every palette. */
  .chart-badge { padding: 2px 7px; color: color-mix(in srgb, var(--accent-deep) 80%, var(--ink)); border: 1px solid color-mix(in srgb, var(--accent-soft) 45%, var(--line)); border-radius: var(--radius-full); background: color-mix(in srgb, var(--accent) 9%, var(--surface-raised)); font: 600 11px var(--font-sans); white-space: nowrap; }
  .chart-badge.partial { color: color-mix(in srgb, var(--accent-deep) 70%, var(--ink)); border-color: color-mix(in srgb, var(--accent-glow) 38%, var(--line)); background: color-mix(in srgb, var(--accent) 13%, var(--surface-raised)); }
  .chart-empty { margin: 0; padding: 26px 0; color: var(--muted); font-size: 12px; text-align: center; }
  .chart-note { margin: 12px 0 0; color: var(--muted); font-size: 11px; line-height: 1.5; }
</style>
