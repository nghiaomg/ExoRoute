<script lang="ts">
  import type { ChartColumn } from '../../components/chartTypes';

  /**
   * The table twin every chart on this page renders behind its toggle. It knows
   * nothing about the data: the caller supplies the header labels and one array
   * of cells per row, already formatted, so the table can never drift from the
   * values the chart plots.
   */
  export let columns: ChartColumn[];
  export let rows: Array<Array<string | number>>;
</script>

<div class="chart-table-scroll">
  <table class="chart-table">
    <thead>
      <tr>
        {#each columns as column}
          <th scope="col" class:numeric={column.numeric}>{column.label}</th>
        {/each}
      </tr>
    </thead>
    <tbody>
      {#each rows as row}
        <tr>
          {#each row as cell, index}
            <td class:numeric={columns[index]?.numeric}>{cell}</td>
          {/each}
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .chart-table-scroll { max-height: 292px; overflow: auto; }
  .chart-table { width: 100%; border-collapse: collapse; font-size: 12px; }
  .chart-table th { position: sticky; top: 0; padding: 7px 8px; color: var(--muted); border-bottom: 1px solid var(--line); background: var(--paper); font-weight: 600; text-align: left; white-space: nowrap; }
  .chart-table td { padding: 6px 8px; color: var(--ink); border-bottom: 1px solid var(--line); }
  .chart-table tr:last-child td { border-bottom: 0; }
  .chart-table .numeric { text-align: right; font-family: var(--font-mono); font-variant-numeric: tabular-nums; }
</style>
