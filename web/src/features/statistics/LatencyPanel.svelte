<script lang="ts">
  import { formatPercent, type Translate } from '../../lib/format';
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import type { RequestStatistics } from '../../lib/types';
  import ChartFrame from './ChartFrame.svelte';
  import ChartTableView from './ChartTableView.svelte';
  import RankedBars from './RankedBars.svelte';
  import type { BarRow } from './chartTypes';

  /**
   * How long requests took, where the time went before the first token, and how
   * the durations spread across a fixed histogram.
   *
   * The percentiles are estimates read from the same histogram the bars show, so
   * a value is the top of its bucket and can only overstate. When a sample lands
   * in the last, open bucket the figure becomes a floor instead, and every
   * affected value is written with "or more" rather than being silently rounded.
   */
  export let statistics: RequestStatistics;
  export let locale: Locale;
  export let tr: Translate;
  export let formatCount: (value: number) => string;

  $: intlLocale = getIntlLocale(locale);
  $: msFormat = new Intl.NumberFormat(intlLocale, { maximumFractionDigits: 0 });
  $: latency = statistics.latency;
  $: buckets = buildBuckets();
  $: ttft = statistics.average_time_to_first_token_ms;

  /**
   * Milliseconds, or a floor when that percentile's own sample ran past the
   * last bucket. The floor marker follows the value, not the range: one slow
   * request must not turn a measured median into "or more".
   */
  function duration(value: number | null, isFloor: boolean): string {
    if (value === null) return tr('Not available');
    if (isFloor) return tr('{value} ms or more', { value: msFormat.format(value) });
    return `${msFormat.format(value)} ms`;
  }

  function bucketLabel(index: number, bounds: number[]): string {
    if (index === 0) return tr('Under {value} ms', { value: msFormat.format(bounds[0]) });
    if (index >= bounds.length) return tr('{value} ms or more', { value: msFormat.format(bounds[bounds.length - 1]) });
    return `${msFormat.format(bounds[index - 1])}–${msFormat.format(bounds[index])} ms`;
  }

  function buildBuckets(): BarRow[] {
    const bounds = latency.bucket_bounds_ms;
    return latency.buckets.map((count, index) => ({
      id: `bucket\u0000${index}`,
      label: bucketLabel(index, bounds),
      value: count,
      display: formatCount(count),
      detail: [
        `${tr('Requests')}: ${formatCount(count)}`,
        `${tr('Share')}: ${formatPercent(latency.samples > 0 ? (count / latency.samples) * 100 : 0, locale)}`,
      ],
    }));
  }
</script>

<ChartFrame
  title={tr('Latency')}
  {tr}
  caption={tr('Percentiles come from a fixed latency histogram, so each value is the top of its bucket.')}
  truncated={statistics.history_truncated}
  empty={latency.samples === 0}
  emptyText={tr('No completed request in the selected period reported a duration.')}
>
  <svelte:fragment slot="chart">
    <div class="latency-tiles">
      <div><span>{tr('Average latency')}</span><strong>{msFormat.format(latency.average_ms)} ms</strong></div>
      <div><span>{tr('Median (p50)')}</span><strong>{duration(latency.p50_ms, latency.p50_is_floor)}</strong></div>
      <div><span>{tr('95th percentile')}</span><strong>{duration(latency.p95_ms, latency.p95_is_floor)}</strong></div>
      <div><span>{tr('99th percentile')}</span><strong>{duration(latency.p99_ms, latency.p99_is_floor)}</strong></div>
    </div>
    <h3 class="latency-subtitle">{tr('Requests by latency bucket')}</h3>
    <RankedBars rows={buckets} maxValue={Math.max(0, ...buckets.map((bucket) => bucket.value))} {tr} />
    <div class="ttft">
      <div>
        <span>{tr('Average time to first token')}</span>
        <strong>{ttft === null ? tr('Not available') : `${msFormat.format(ttft)} ms`}</strong>
      </div>
      <small>{statistics.time_to_first_token_samples > 0
        ? tr('Based on {count} streamed responses.', { count: formatCount(statistics.time_to_first_token_samples) })
        : tr('No streamed response in the selected period.')}</small>
    </div>
  </svelte:fragment>
  <svelte:fragment slot="table">
    <ChartTableView
      columns={[
        { label: tr('Bucket') },
        { label: tr('Requests'), numeric: true },
        { label: tr('Share'), numeric: true },
      ]}
      rows={buckets.map((bucket) => [
        bucket.label,
        bucket.display,
        formatPercent(latency.samples > 0 ? (bucket.value / latency.samples) * 100 : 0, locale),
      ])}
    />
  </svelte:fragment>
</ChartFrame>

<style>
  .latency-tiles { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 10px; margin-bottom: 16px; }
  .latency-tiles > div { min-width: 0; display: grid; gap: 5px; }
  .latency-tiles span { color: var(--muted); font-size: 11px; }
  .latency-tiles strong { overflow: hidden; color: var(--ink); font: 600 15px/1.2 var(--font-mono); font-variant-numeric: tabular-nums; text-overflow: ellipsis; white-space: nowrap; }
  .latency-subtitle { margin: 0 0 10px; color: var(--muted); font: 600 12px var(--font-sans); }
  .ttft { display: grid; gap: 3px; margin-top: 16px; padding-top: 14px; border-top: 1px solid var(--line); }
  .ttft > div { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; }
  .ttft span { color: var(--muted); font-size: 12px; }
  .ttft strong { color: var(--ink); font: 600 15px var(--font-mono); font-variant-numeric: tabular-nums; }
  .ttft small { color: var(--muted); font-size: 11px; }
  @media (max-width: 620px) { .latency-tiles { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
