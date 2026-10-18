<script lang="ts">
  import { formatCostMicroUsd, formatPercent, type Translate } from '../../lib/format';
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import type { RequestStatistics } from '../../lib/types';
  import ChartFrame from './ChartFrame.svelte';
  import ChartTableView from './ChartTableView.svelte';
  import RankedBars from './RankedBars.svelte';
  import { dimensionBars, type BarRow, type ChartColumn } from './chartTypes';

  /**
   * One ranked panel over the request history, switchable between the dimensions
   * the bounded log scan actually records: model (with the provider that served
   * it), provider, API key, and response status. The bars, the hover detail and
   * the table all come from one grouped set of rows, so the views of a dimension
   * cannot disagree.
   */
  export let statistics: RequestStatistics;
  export let locale: Locale;
  export let tr: Translate;
  export let formatCount: (value: number) => string;

  type Dimension = 'model' | 'provider' | 'key' | 'status';
  const DIMENSIONS: Dimension[] = ['model', 'provider', 'key', 'status'];

  let dimension: Dimension = 'model';

  $: intlLocale = getIntlLocale(locale);
  $: latencyFormat = new Intl.NumberFormat(intlLocale, { maximumFractionDigits: 1 });
  $: bars = buildBars(dimension);
  $: table = buildTable(dimension);
  $: providerCostIsPartial =
    dimension === 'provider' && statistics.providers.rows.some((row) => row.missing_cost_events > 0);

  function latency(value: number): string {
    return `${latencyFormat.format(value)} ms`;
  }

  function buildBars(target: Dimension): BarRow[] {
    if (target === 'model') {
      return statistics.model_breakdown.map((row) => ({
        id: `model\u0000${row.model}\u0000${row.provider ?? ''}`,
        label: row.provider ? `${row.model} · ${row.provider}` : row.model,
        value: row.requests,
        display: formatCount(row.requests),
        detail: [
          `${tr('Requests')}: ${formatCount(row.requests)}`,
          `${tr('Success rate')}: ${formatPercent(row.success_rate, locale)}`,
          `${tr('Input tokens')}: ${formatCount(row.input_tokens)}`,
          `${tr('Output tokens')}: ${formatCount(row.output_tokens)}`,
          `${tr('Cache hit rate')}: ${formatPercent(row.cache_hit_rate, locale)}`,
        ],
      }));
    }
    if (target === 'provider') {
      return statistics.providers.rows.map((row) => ({
        id: `provider\u0000${row.provider ?? ''}`,
        label: row.provider ?? tr('No provider selected'),
        value: row.requests,
        display: formatCount(row.requests),
        detail: [
          `${tr('Requests')}: ${formatCount(row.requests)}`,
          `${tr('Success rate')}: ${formatPercent(row.success_rate, locale)}`,
          `${tr('Average latency')}: ${latency(row.average_latency_ms)}`,
          `${tr('Input tokens')}: ${formatCount(row.input_tokens)}`,
          `${tr('Output tokens')}: ${formatCount(row.output_tokens)}`,
          row.missing_cost_events > 0
            ? tr('{cost} or more', { cost: formatCostMicroUsd(row.cost_reported_micro_usd) })
            : formatCostMicroUsd(row.cost_micro_usd),
        ],
      }));
    }
    if (target === 'key') {
      return dimensionBars(statistics.api_keys.top, statistics.api_keys.others, statistics.total_requests, {
        tr,
        formatCount,
        locale,
        othersLabel: tr('Others'),
      });
    }
    // A status distribution reads best in code order rather than by count: the
    // reader is looking for a specific class of failure, not the largest one.
    return orderedStatuses().map((row) => ({
      id: `status\u0000${row.status}`,
      label: `${row.status} · ${tr(statusClass(row.status))}`,
      value: row.requests,
      display: formatCount(row.requests),
      detail: [
        `${tr('Successful')}: ${formatCount(row.successes)}`,
        `${tr('Failed')}: ${formatCount(row.requests - row.successes)}`,
        `${tr('Average latency')}: ${latency(row.average_latency_ms)}`,
      ],
    }));
  }

  /** Response class, read from the returned code so chart and table agree. */
  function statusClass(status: number): string {
    return status >= 200 && status < 300 ? 'Successful' : 'Failed';
  }

  function orderedStatuses(): RequestStatistics['statuses']['rows'] {
    return [...statistics.statuses.rows].sort((left, right) => left.status - right.status);
  }

  function buildTable(target: Dimension): { columns: ChartColumn[]; rows: string[][] } {
    const number = new Intl.NumberFormat(intlLocale);
    if (target === 'model') {
      return {
        columns: [
          { label: tr('Models') },
          { label: tr('Provider') },
          { label: tr('Requests'), numeric: true },
          { label: tr('Success rate'), numeric: true },
          { label: tr('Input tokens'), numeric: true },
          { label: tr('Output tokens'), numeric: true },
          { label: tr('Cache hit rate'), numeric: true },
        ],
        rows: statistics.model_breakdown.map((row) => [
          row.model,
          row.provider ?? '—',
          number.format(row.requests),
          formatPercent(row.success_rate, locale),
          number.format(row.input_tokens),
          number.format(row.output_tokens),
          formatPercent(row.cache_hit_rate, locale),
        ]),
      };
    }
    if (target === 'provider') {
      return {
        columns: [
          { label: tr('Provider') },
          { label: tr('Requests'), numeric: true },
          { label: tr('Success rate'), numeric: true },
          { label: tr('Average latency'), numeric: true },
          { label: tr('Input tokens'), numeric: true },
          { label: tr('Output tokens'), numeric: true },
          { label: tr('Cost'), numeric: true },
        ],
        rows: statistics.providers.rows.map((row) => [
          row.provider ?? tr('No provider selected'),
          number.format(row.requests),
          formatPercent(row.success_rate, locale),
          latency(row.average_latency_ms),
          number.format(row.input_tokens),
          number.format(row.output_tokens),
          row.missing_cost_events > 0
            ? tr('{cost} or more', { cost: formatCostMicroUsd(row.cost_reported_micro_usd) })
            : formatCostMicroUsd(row.cost_micro_usd),
        ]),
      };
    }
    if (target === 'key') {
      return {
        columns: [
          { label: tr('API key') },
          { label: tr('Requests'), numeric: true },
          { label: tr('Share'), numeric: true },
        ],
        // The share is recomputed from each row's own count, so the table and the
        // bar list can never show two different percentages for one row.
        rows: bars.map((bar) => [
          bar.label,
          bar.display,
          formatPercent(statistics.total_requests > 0 ? (bar.value / statistics.total_requests) * 100 : 0, locale),
        ]),
      };
    }
    return {
      columns: [
        { label: tr('Status'), numeric: true },
        { label: tr('Class') },
        { label: tr('Requests'), numeric: true },
        { label: tr('Success rate'), numeric: true },
        { label: tr('Average latency'), numeric: true },
      ],
      rows: orderedStatuses().map((row) => [
        String(row.status),
        tr(statusClass(row.status)),
        number.format(row.requests),
        formatPercent(row.requests > 0 ? (row.successes / row.requests) * 100 : 0, locale),
        latency(row.average_latency_ms),
      ]),
    };
  }

  /** The API-key list folds its remainder into its own row, so it is never short. */
  function truncatedFor(target: Dimension): boolean {
    if (target === 'model') return statistics.model_breakdown_truncated;
    if (target === 'provider') return statistics.providers.truncated;
    if (target === 'key') return false;
    return statistics.statuses.truncated;
  }

  function captionFor(target: Dimension, count: number): string {
    if (target === 'model') return tr('Usage by model in the selected period.');
    if (target === 'provider') return tr('Requests grouped by the provider that served them.');
    if (target === 'key') return tr('Requests grouped by the API key that sent them.');
    return tr('{count} status codes were returned in the selected period.', { count: formatCount(count) });
  }

  function emptyTextFor(target: Dimension): string {
    if (target === 'model') return tr('No models called in the selected period.');
    if (target === 'provider') return tr('No provider data in the selected period.');
    if (target === 'key') return tr('No API key sent a request in the selected period.');
    return tr('No status data in the selected period.');
  }
</script>

<ChartFrame
  title={tr('Traffic breakdown')}
  {tr}
  caption={captionFor(dimension, table.rows.length)}
  truncated={truncatedFor(dimension)}
  partial={providerCostIsPartial}
  empty={bars.length === 0}
  emptyText={emptyTextFor(dimension)}
  partialText={tr('Some requests in this period reported no cost, so the spend shown for a provider is a lower bound.')}
>
  <svelte:fragment slot="chart">
    <div class="dimension-switch" role="group" aria-label={tr('Breakdown dimension')}>
      {#each DIMENSIONS as value}
        <button class:active={dimension === value} aria-pressed={dimension === value} onclick={() => (dimension = value)}>
          {tr(value === 'model' ? 'By model' : value === 'provider' ? 'By provider' : value === 'key' ? 'By API key' : 'By status')}
        </button>
      {/each}
    </div>
    <RankedBars rows={bars} maxValue={Math.max(0, ...bars.map((bar) => bar.value))} {tr} />
  </svelte:fragment>
  <svelte:fragment slot="table">
    <ChartTableView columns={table.columns} rows={table.rows} />
  </svelte:fragment>
</ChartFrame>

<style>
  .dimension-switch { display: inline-flex; gap: 2px; margin-bottom: 14px; padding: 3px; border: 2px solid var(--ink); border-radius: 9px; background: var(--paper); }
  .dimension-switch button { padding: 5px 10px; color: var(--muted); border: 0; border-radius: 6px; background: transparent; font: 600 12px var(--font-sans); cursor: pointer; }
  .dimension-switch button.active { color: #fff; background: var(--violet); }
</style>
