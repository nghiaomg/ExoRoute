<script lang="ts">
  import { Activity, CircleCheck, CircleX, Clock3 } from '@lucide/svelte';
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
  import type { RequestStatistics } from '../../lib/types';
  import Sparkline from '../../components/Sparkline.svelte';

  export let tr: Translate;
  export let locale: Locale;
  export let statistics: RequestStatistics | null = null;
  export let selectedRange: RequestStatistics['range'] = '1d';
  export let isInitialLoading = false;
  export let isFetching = false;
  export let errorMessage = '';
  export let inlineError = '';
  export let onRangeChange: (range: RequestStatistics['range']) => void = () => {};
  export let onRefresh: () => void = () => {};

  $: numberFormat = new Intl.NumberFormat(getIntlLocale(locale));
  $: points = statistics?.series.points ?? [];
  // Buckets with no request have no rate to average, so they are left out of the
  // rate and latency trends rather than plotted as a zero.
  $: servedBuckets = points.filter((point) => point.requests > 0);
  $: requestTrend = points.map((point) => point.requests);
  $: failureTrend = points.map((point) => point.failures);
  $: successRateTrend = servedBuckets.map((point) => (point.successes / point.requests) * 100);
  $: latencyTrend = servedBuckets.map((point) => point.average_latency_ms);

  function formatCount(value: number): string {
    return numberFormat.format(Number.isFinite(value) ? value : 0);
  }
</script>

{#if isInitialLoading || (!statistics && !errorMessage)}
  <div class="statistics-skeleton" aria-label={tr('Loading statistics')}>
    <div class="statistics-kpis">
      <div class="skeleton kpi-skeleton"></div>
      <div class="skeleton kpi-skeleton"></div>
      <div class="skeleton kpi-skeleton"></div>
      <div class="skeleton kpi-skeleton"></div>
    </div>
  </div>
{:else if !statistics}
  <div class="statistics-error-banner" role="alert">
    <CircleX size={16} />
    <span>{errorMessage}</span>
    <button class="secondary-button compact" onclick={onRefresh}>{tr('Retry')}</button>
  </div>
{:else}
  <div class="statistics-summary">
    <div class="range-progress" class:visible={isFetching} role="progressbar" aria-label={tr('Loading statistics')}></div>

    <div class="statistics-actions">
      <div class="range-switch" role="group" aria-label={tr('Statistics time range')}>
        <button class:active={selectedRange === '1d'} aria-pressed={selectedRange === '1d'} onclick={() => onRangeChange('1d')}>{tr('24 hours')}</button>
        <button class:active={selectedRange === '7d'} aria-pressed={selectedRange === '7d'} onclick={() => onRangeChange('7d')}>{tr('7 days')}</button>
        <button class:active={selectedRange === '30d'} aria-pressed={selectedRange === '30d'} onclick={() => onRangeChange('30d')}>{tr('30 days')}</button>
      </div>
      <button class="secondary-button compact" aria-label={tr('Refresh statistics')} disabled={isFetching} onclick={onRefresh}>
        {#if isFetching}<span class="auth-bootstrap-spinner"></span>{:else}<Activity size={14} />{/if}{tr('Refresh')}
      </button>
    </div>

    {#if inlineError}
      <div class="statistics-error-banner" role="alert">
        <CircleX size={16} />
        <span>{inlineError}</span>
        <button class="secondary-button compact" onclick={onRefresh}>{tr('Retry')}</button>
      </div>
    {/if}

    {#if !statistics.complete}
      <div class="statistics-warning" role="status"><CircleX size={17} /><div><strong>{tr('Statistics are incomplete')}</strong><span>{tr('{count} telemetry events were dropped. Displayed totals and percentages use the events that were saved.', { count: formatCount(statistics.dropped_events) })}</span></div></div>
    {/if}
    {#if !statistics.aggregation_current}
      <div class="statistics-warning updating" role="status"><Clock3 size={17} /><div><strong>{tr('Rolling totals are catching up')}</strong><span>{tr('The background worker is expiring older minute buckets. Values can temporarily include older traffic.')}</span></div></div>
    {/if}

    <section class="statistics-kpis" aria-label={tr('Traffic summary')}>
      <article class="statistics-kpi">
        <div><span>{tr('Total requests')}</span><Activity size={17} /></div>
        <strong>{formatCount(statistics.total_requests)}</strong>
        <small>{tr('Across the selected period')}</small>
        <Sparkline values={requestTrend} label={tr('Requests in each column of the selected period.')} />
      </article>
      <article class="statistics-kpi">
        <div><span>{tr('Success rate')}</span><CircleCheck size={17} /></div>
        <strong>{statistics.success_rate.toFixed(1)}%</strong>
        <small>{tr('{count} successful', { count: formatCount(statistics.successes) })}</small>
        <Sparkline values={successRateTrend} label={tr('Success rate in each column that served a request.')} />
      </article>
      <article class="statistics-kpi">
        <div><span>{tr('Failed requests')}</span><CircleX size={17} /></div>
        <strong>{formatCount(statistics.failures)}</strong>
        <small>{tr('Final client request outcome')}</small>
        <Sparkline values={failureTrend} label={tr('Failed requests in each column of the selected period.')} />
      </article>
      <article class="statistics-kpi">
        <div><span>{tr('Average latency')}</span><Clock3 size={17} /></div>
        <strong>{statistics.average_latency_ms.toFixed(1)} <em>ms</em></strong>
        <small>{tr('End-to-end request duration')}</small>
        <Sparkline values={latencyTrend} label={tr('Average latency in each column that served a request.')} />
      </article>
    </section>
  </div>
{/if}

<style>
  .statistics-actions { display: flex; align-items: center; gap: 9px; margin-bottom: 14px; }
  .range-switch { display: inline-flex; gap: 2px; padding: 3px; border: 2px solid var(--ink); border-radius: 9px; background: var(--paper); box-shadow: 2px 2px 0 var(--ink); }
  .range-switch button { padding: 6px 9px; color: var(--muted); border: 0; border-radius: 6px; background: transparent; font: 600 12px var(--font-sans); cursor: pointer; transition: background 0.15s ease, color 0.15s ease; }
  .range-switch button.active { color: #fff; background: var(--violet); }
  .statistics-skeleton { display: grid; gap: 14px; }
  .kpi-skeleton { height: 132px; border-radius: 13px; }
  .range-progress { height: 3px; width: 100%; margin-bottom: 12px; border-radius: 3px; background: transparent; overflow: hidden; opacity: 0; transition: opacity 0.15s ease; }
  .range-progress.visible { opacity: 1; background: color-mix(in srgb, var(--violet) 15%, transparent); }
  .range-progress.visible::after { content: ''; display: block; height: 100%; width: 40%; background: var(--violet); border-radius: 3px; animation: progress-indeterminate 1.1s cubic-bezier(0.4, 0, 0.2, 1) infinite; }
  @keyframes progress-indeterminate { 0% { transform: translateX(-100%); width: 20%; } 50% { transform: translateX(120%); width: 60%; } 100% { transform: translateX(350%); width: 30%; } }
  .statistics-error-banner { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-bottom: 12px; padding: 10px 14px; color: #b91c1c; border: 2px solid #b91c1c; border-radius: 10px; box-shadow: 2px 2px 0 #b91c1c; background: #fef2f2; font-size: 12px; }
  .statistics-error-banner :global(svg) { flex: 0 0 auto; }
  .statistics-error-banner span { flex: 1; }
  :global(:root[data-theme='dark']) .statistics-error-banner { color: #fecaca; border-color: #f87171; box-shadow: 2px 2px 0 #f87171; background: #2b1818; }
  .statistics-kpis { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; margin-bottom: 14px; }
  .statistics-kpi { min-width: 0; padding: 17px 18px; border: 2px solid var(--ink); border-radius: 13px; background: var(--paper); box-shadow: 3px 3px 0 var(--ink); }
  .statistics-kpi > div { display: flex; align-items: center; justify-content: space-between; gap: 10px; color: var(--muted); font-size: 12px; }
  .statistics-kpi > div :global(svg) { color: var(--violet); }
  .statistics-kpi > strong { display: block; margin: 13px 0 3px; overflow: hidden; color: var(--ink); font: 700 25px/1.15 var(--font-mono); text-overflow: ellipsis; white-space: nowrap; }
  .statistics-kpi > strong em { color: var(--muted); font: 500 12px var(--font-sans); }
  .statistics-kpi small { color: var(--muted); font-size: 12px; }
  .statistics-warning { display: flex; align-items: flex-start; gap: 10px; margin-bottom: 12px; padding: 12px 14px; color: #9a3412; border: 2px solid var(--ink); border-radius: 10px; box-shadow: 2px 2px 0 var(--ink); background: color-mix(in srgb, var(--accent) 7%, #fff); }
  .statistics-warning :global(svg) { flex: 0 0 auto; margin-top: 1px; }
  .statistics-warning > div { display: grid; gap: 3px; }
  .statistics-warning strong { font-size: 12px; }
  .statistics-warning span { font-size: 12px; line-height: 1.45; }
  .statistics-warning.updating { color: #755621; border-color: #ebd9a9; background: #fffdf5; }
  @media (max-width: 1000px) { .statistics-kpis { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  @media (max-width: 620px) { .statistics-actions { align-items: flex-end; flex-direction: column; } .statistics-kpis { gap: 8px; } .statistics-kpi { padding: 13px; } .statistics-kpi > strong { font-size: 20px; } }
</style>
