<script lang="ts">
  import { formatCostMicroUsd, type Translate } from '../../lib/format';
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import type { RequestStatistics, SeriesPoint } from '../../lib/types';
  import ChartFrame from './ChartFrame.svelte';
  import ChartTableView from './ChartTableView.svelte';

  /**
   * Traffic over the selected period, one bucket per column.
   *
   * One measure is plotted at a time. Requests and cost live on scales that do
   * not share an axis, so switching between them re-labels the axis instead of
   * drawing a second one. A single series needs no legend: the title names it,
   * and the peak is labelled directly.
   */
  export let statistics: RequestStatistics;
  export let locale: Locale;
  export let tr: Translate;
  export let formatCount: (value: number) => string;

  type Metric = 'requests' | 'cost';

  const PLOT_HEIGHT = 196;
  const PADDING = { top: 14, right: 12, bottom: 26, left: 52 };
  const INNER_HEIGHT = PLOT_HEIGHT - PADDING.top - PADDING.bottom;

  let metric: Metric = 'requests';
  let plotWidth = 720;
  let cursor: number | null = null;

  $: intlLocale = getIntlLocale(locale);
  $: points = statistics.series.points;
  $: chart = buildChart(points, metric, plotWidth);

  function value(point: SeriesPoint, target: Metric): number {
    return target === 'requests' ? point.requests : point.cost_reported_micro_usd;
  }

  function formatValue(input: number, target: Metric): string {
    return target === 'requests' ? formatCount(input) : formatCostMicroUsd(input);
  }

  function buildChart(points: SeriesPoint[], metric: Metric, width: number) {
    const innerWidth = Math.max(1, width - PADDING.left - PADDING.right);
    const max = Math.max(1, ...points.map((point) => value(point, metric)));
    const x = (index: number): number =>
      PADDING.left + (points.length > 1 ? (index / (points.length - 1)) * innerWidth : innerWidth / 2);
    const y = (input: number): number => PADDING.top + INNER_HEIGHT - (input / max) * INNER_HEIGHT;
    const values = points.map((point) => value(point, metric));
    const line = values.map((input, index) => `${index === 0 ? 'M' : 'L'}${x(index).toFixed(2)},${y(input).toFixed(2)}`).join(' ');
    const baseline = PADDING.top + INNER_HEIGHT;
    return {
      max,
      values,
      x,
      y,
      baseline,
      line,
      area: values.length ? `${line} L${x(values.length - 1).toFixed(2)},${baseline} L${x(0).toFixed(2)},${baseline} Z` : '',
      grid: [0, 0.5, 1].map((share) => ({ y: y(max * share), label: formatValue(max * share, metric) })),
      peak: values.length ? values.indexOf(Math.max(...values)) : -1,
      ticks: tickIndexes(values.length),
    };
  }

  /** At most five x labels, always including the first and the last bucket. */
  function tickIndexes(count: number): number[] {
    if (count === 0) return [];
    const ticks = new Set<number>([0, count - 1]);
    const step = Math.max(1, Math.round((count - 1) / 4));
    for (let index = step; index < count - 1; index += step) ticks.add(index);
    return [...ticks].sort((left, right) => left - right);
  }

  function bucketLabel(start: string, long: boolean): string {
    const parsed = new Date(`${start.replace(' ', 'T')}Z`);
    if (Number.isNaN(parsed.getTime())) return start;
    return new Intl.DateTimeFormat(intlLocale, long ? { dateStyle: 'short', timeStyle: 'short' } : { dateStyle: 'short' }).format(parsed);
  }

  $: hourlyBuckets = statistics.series.bucket_seconds < 86_400;

  function moveCursor(event: PointerEvent): void {
    if (points.length === 0) return;
    const rect = (event.currentTarget as Element).getBoundingClientRect();
    if (rect.width <= 0) return;
    const ratio = (event.clientX - rect.left) / rect.width;
    cursor = Math.max(0, Math.min(points.length - 1, Math.round(ratio * (points.length - 1))));
  }

  function stepCursor(delta: number): void {
    if (points.length === 0) return;
    cursor = Math.max(0, Math.min(points.length - 1, (cursor ?? points.length - 1) + delta));
  }

  function onKeydown(event: KeyboardEvent): void {
    if (event.key === 'ArrowLeft') stepCursor(-1);
    else if (event.key === 'ArrowRight') stepCursor(1);
    else if (event.key === 'Escape') cursor = null;
    else return;
    event.preventDefault();
  }

  function tooltipLines(point: SeriesPoint): string[] {
    const lines = [
      bucketLabel(point.start, true),
      `${tr('Requests')}: ${formatCount(point.requests)}`,
      `${tr('Successful')}: ${formatCount(point.successes)}`,
      `${tr('Failed')}: ${formatCount(point.failures)}`,
      `${tr('Input tokens')}: ${formatCount(point.input_tokens)}`,
      `${tr('Output tokens')}: ${formatCount(point.output_tokens)}`,
    ];
    lines.push(
      point.missing_cost_events > 0
        ? tr('{cost} or more', { cost: formatCostMicroUsd(point.cost_reported_micro_usd) })
        : formatCostMicroUsd(point.cost_micro_usd),
    );
    return lines;
  }

  function bucketCountLabel(): string {
    const seconds = statistics.series.bucket_seconds;
    if (seconds <= 3_600) return tr('One column per hour.');
    if (seconds <= 21_600) return tr('One column every 6 hours.');
    return tr('One column per day.');
  }
</script>

<ChartFrame
  title={metric === 'requests' ? tr('Traffic over time') : tr('Spend over time')}
  {tr}
  caption={tr('{buckets} Taken from the retained request history for this period.', { buckets: bucketCountLabel() })}
  truncated={statistics.series.truncated}
  partial={metric === 'cost' && points.some((point) => point.missing_cost_events > 0)}
  empty={points.length === 0}
  emptyText={tr('No request history is retained for the selected period.')}
  partialText={tr('Some requests in this period reported no cost, so each column is a lower bound.')}
>
  <svelte:fragment slot="chart">
    <div class="dimension-switch" role="group" aria-label={tr('Series metric')}>
      <button class:active={metric === 'requests'} aria-pressed={metric === 'requests'} onclick={() => (metric = 'requests')}>{tr('Requests')}</button>
      <button class:active={metric === 'cost'} aria-pressed={metric === 'cost'} onclick={() => (metric = 'cost')}>{tr('Cost')}</button>
    </div>
    <div class="plot" bind:clientWidth={plotWidth}>
      <!--
        The plot is focusable on purpose: the arrow keys step a cursor through the
        buckets, which is the keyboard path to the same numbers the table lists.
      -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions a11y_no_noninteractive_tabindex -->
      <svg
        class="plot-svg"
        viewBox={`0 0 ${plotWidth} ${PLOT_HEIGHT}`}
        role="img"
        tabindex="0"
        aria-label={metric === 'requests' ? tr('Requests over time') : tr('Spend over time')}
        onpointermove={moveCursor}
        onpointerleave={() => (cursor = null)}
        onkeydown={onKeydown}
      >
        {#each chart.grid as line}
          <line x1={PADDING.left} x2={plotWidth - PADDING.right} y1={line.y} y2={line.y} stroke="var(--chart-grid)" stroke-width="1" />
          <text x={PADDING.left - 8} y={line.y + 3} text-anchor="end" class="axis-label">{line.label}</text>
        {/each}
        <path d={chart.area} fill="var(--chart-series-soft)" opacity="0.14" />
        <path d={chart.line} fill="none" stroke="var(--chart-series)" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" />
        {#each chart.ticks as index}
          <text x={chart.x(index)} y={PLOT_HEIGHT - 8} text-anchor={index === 0 ? 'start' : index === points.length - 1 ? 'end' : 'middle'} class="axis-label">
            {bucketLabel(points[index].start, false)}
          </text>
        {/each}
        {#if chart.peak >= 0}
          <circle cx={chart.x(chart.peak)} cy={chart.y(chart.values[chart.peak])} r="3.5" fill="var(--chart-series)" stroke="var(--paper)" stroke-width="2" />
          <text
            x={Math.min(chart.x(chart.peak), plotWidth - PADDING.right - 4)}
            y={Math.max(PADDING.top + 9, chart.y(chart.values[chart.peak]) - 9)}
            text-anchor={chart.peak > points.length * 0.7 ? 'end' : 'start'}
            class="peak-label"
          >{formatValue(chart.values[chart.peak], metric)}</text>
        {/if}
        {#if cursor !== null && points[cursor]}
          <line x1={chart.x(cursor)} x2={chart.x(cursor)} y1={PADDING.top} y2={chart.baseline} stroke="var(--ink)" stroke-width="1" stroke-dasharray="3 3" />
          <circle cx={chart.x(cursor)} cy={chart.y(chart.values[cursor])} r="4" fill="var(--chart-series)" stroke="var(--paper)" stroke-width="2" />
        {/if}
      </svg>
      {#if cursor !== null && points[cursor]}
        <div
          class="plot-tip"
          role="tooltip"
          style={`left:${Math.min(Math.max(chart.x(cursor), 94), Math.max(94, plotWidth - 94))}px`}
        >
          {#each tooltipLines(points[cursor]) as line}<span>{line}</span>{/each}
        </div>
      {/if}
    </div>
  </svelte:fragment>
  <svelte:fragment slot="table">
    <ChartTableView
      columns={[
        { label: tr('From') },
        { label: tr('Requests'), numeric: true },
        { label: tr('Successful'), numeric: true },
        { label: tr('Failed'), numeric: true },
        { label: tr('Input tokens'), numeric: true },
        { label: tr('Output tokens'), numeric: true },
        { label: tr('Average latency'), numeric: true },
        { label: tr('Cost'), numeric: true },
      ]}
      rows={points.map((point) => [
        bucketLabel(point.start, true),
        formatCount(point.requests),
        formatCount(point.successes),
        formatCount(point.failures),
        formatCount(point.input_tokens),
        formatCount(point.output_tokens),
        `${point.average_latency_ms.toFixed(1)} ms`,
        point.missing_cost_events > 0
          ? tr('{cost} or more', { cost: formatCostMicroUsd(point.cost_reported_micro_usd) })
          : formatCostMicroUsd(point.cost_micro_usd),
      ])}
    />
  </svelte:fragment>
</ChartFrame>

<style>
  .dimension-switch { display: inline-flex; gap: 2px; margin-bottom: 12px; padding: 3px; border: 2px solid var(--ink); border-radius: 9px; background: var(--paper); }
  .dimension-switch button { padding: 5px 10px; color: var(--muted); border: 0; border-radius: 6px; background: transparent; font: 600 12px var(--font-sans); cursor: pointer; }
  .dimension-switch button.active { color: #fff; background: var(--violet); }
  .plot { position: relative; }
  .plot-svg { display: block; width: 100%; height: 196px; overflow: visible; }
  .plot-svg:focus-visible { outline: 2px solid var(--violet); outline-offset: 3px; border-radius: 6px; }
  .axis-label { fill: var(--muted); font: 500 10px var(--font-mono); font-variant-numeric: tabular-nums; }
  .peak-label { fill: var(--ink); font: 600 11px var(--font-mono); }
  .plot-tip { position: absolute; z-index: 6; top: 4px; transform: translateX(-50%); display: grid; gap: 2px; padding: 8px 10px; color: #fff; border-radius: 8px; background: var(--ink); box-shadow: 2px 2px 0 rgba(0, 0, 0, 0.2); font-size: 11px; line-height: 1.45; white-space: nowrap; pointer-events: none; }
  :global(:root[data-theme='dark']) .plot-tip { color: #0b0b0b; }
</style>
