<script lang="ts">
  import type { Translate } from '../../lib/format';
  import type { BarRow } from './chartTypes';

  /**
   * Horizontal bars for a ranked or ordered list.
   *
   * Every bar uses the one validated data hue: the row's length carries the
   * magnitude and its label carries the identity, so nothing is encoded by
   * colour and a change in row order can never repaint a row. Each row writes
   * its own value beside the bar, so the hover card adds context instead of
   * being the only way to read a number; the panel's table view is the
   * keyboard-reachable path, which is why rows are not focusable themselves and
   * a long list does not become a long tab sequence.
   *
   * Rows render in the order given. Ordering is the caller's decision so the
   * same component serves a ranking (by value) and an ordered set (buckets).
   */
  export let rows: BarRow[];
  export let maxValue: number;
  export let tr: Translate;

  /** Bars past this count are unreadable; the table view still lists them all. */
  const MAX_BARS = 12;

  let activeId: string | null = null;

  $: visible = rows.slice(0, MAX_BARS);
  $: hidden = rows.length - visible.length;

  function share(value: number): number {
    return maxValue > 0 ? Math.max(0, Math.min(100, (value / maxValue) * 100)) : 0;
  }
</script>

<ul class="bars">
  {#each visible as row (row.id)}
    <li
      class="bar-row"
      onpointerenter={() => (activeId = row.id)}
      onpointerleave={() => (activeId = null)}
    >
      <span class="bar-label" title={row.label}>{row.label}</span>
      <span class="bar-track">
        <span class="bar-fill" style={`width:${share(row.value)}%`}></span>
      </span>
      <span class="bar-value">{row.display}</span>
      {#if activeId === row.id && row.detail?.length}
        <span class="bar-tip" role="tooltip">
          {#each row.detail as line}<span>{line}</span>{/each}
        </span>
      {/if}
    </li>
  {/each}
</ul>
{#if hidden > 0}
  <p class="bars-note">{tr('Showing the top {count}. Open the table for every row.', { count: visible.length })}</p>
{/if}

<style>
  .bars { display: grid; gap: 7px; margin: 0; padding: 0; list-style: none; }
  .bar-row { position: relative; display: flex; align-items: center; gap: 10px; min-width: 0; }
  .bar-label { flex: 0 1 auto; max-width: 34%; overflow: hidden; color: var(--ink); font-size: 12px; text-overflow: ellipsis; white-space: nowrap; }
  .bar-track { flex: 1 1 auto; min-width: 40px; height: 9px; border-radius: 2px; background: var(--line); }
  /* 4px rounded data-end anchored at the baseline on the left. */
  .bar-fill { display: block; height: 100%; border-radius: 0 4px 4px 0; background: var(--chart-series); }
  .bar-value { flex: 0 0 auto; min-width: 3.4rem; color: var(--ink); font: 500 12px var(--font-mono); font-variant-numeric: tabular-nums; text-align: right; }
  .bar-tip { position: absolute; z-index: 6; left: 0; bottom: calc(100% + 6px); display: grid; gap: 2px; padding: 7px 9px; color: #fff; border-radius: 7px; background: var(--ink); box-shadow: 2px 2px 0 rgba(0, 0, 0, 0.18); font-size: 11px; line-height: 1.4; max-width: 260px; }
  :global(:root[data-theme='dark']) .bar-tip { color: #0b0b0b; }
  .bars-note { margin: 10px 0 0; color: var(--muted); font-size: 11px; }
</style>
