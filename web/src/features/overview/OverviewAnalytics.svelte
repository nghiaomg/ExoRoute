<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { CircleX, Database, RefreshCw } from '@lucide/svelte';
  import { api } from '../../lib/api';
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
  import { localizedError } from '../../lib/errors';
  import type { RequestStatistics } from '../../lib/types';
  import BreakdownChart from './BreakdownChart.svelte';
  import CostPanel from './CostPanel.svelte';
  import LatencyPanel from './LatencyPanel.svelte';
  import TrafficSeries from './TrafficSeries.svelte';

  export let tr: Translate;
  export let locale: Locale;

  let selectedRange: RequestStatistics['range'] = '1d';
  let statistics: RequestStatistics | null = null;
  let isInitialLoading = true;
  let isFetching = false;
  let errorMessage = '';
  let inlineError = '';
  let generation = 0;
  let activeController: AbortController | null = null;

  const cache: Partial<Record<RequestStatistics['range'], { data: RequestStatistics; timestamp: number }>> = {};
  const CACHE_TTL_MS = 60_000;

  $: numberFormat = new Intl.NumberFormat(getIntlLocale(locale));

  function formatCount(value: number): string {
    return numberFormat.format(Number.isFinite(value) ? value : 0);
  }

  async function load(range: RequestStatistics['range'] = selectedRange, forceRefresh = false): Promise<void> {
    selectedRange = range;
    inlineError = '';

    const cached = cache[range];
    const isStale = !cached || (Date.now() - cached.timestamp > CACHE_TTL_MS);

    if (cached && !forceRefresh) {
      statistics = cached.data;
      isInitialLoading = false;
      if (!isStale) return;
    }

    const requestGeneration = ++generation;
    activeController?.abort();
    const controller = new AbortController();
    activeController = controller;

    if (!statistics) {
      isInitialLoading = true;
    }
    isFetching = true;
    errorMessage = '';

    try {
      const result = await api.statistics(range, controller.signal);
      if (requestGeneration !== generation) return;
      cache[range] = { data: result, timestamp: Date.now() };
      statistics = result;
    } catch (error) {
      if (requestGeneration !== generation || controller.signal.aborted) return;
      const formatted = localizedError(error, 'Something went wrong while loading this page.', tr);
      if (!statistics) {
        errorMessage = formatted;
      } else {
        inlineError = formatted;
      }
    } finally {
      if (requestGeneration === generation) {
        isInitialLoading = false;
        isFetching = false;
      }
    }
  }

  onMount(() => { void load(); });
  onDestroy(() => {
    generation += 1;
    activeController?.abort();
  });
</script>

<section class="overview-analytics" aria-label={tr('Statistics time range')}>
  <div class="analytics-actions">
    <div class="range-switch" role="group" aria-label={tr('Statistics time range')}>
      <button class:active={selectedRange === '1d'} aria-pressed={selectedRange === '1d'} onclick={() => load('1d')}>{tr('24 hours')}</button>
      <button class:active={selectedRange === '7d'} aria-pressed={selectedRange === '7d'} onclick={() => load('7d')}>{tr('7 days')}</button>
      <button class:active={selectedRange === '30d'} aria-pressed={selectedRange === '30d'} onclick={() => load('30d')}>{tr('30 days')}</button>
    </div>
    <button class="secondary-button compact" aria-label={tr('Refresh statistics')} disabled={isFetching} onclick={() => load(selectedRange, true)}>
      {#if isFetching}<span class="auth-bootstrap-spinner"></span>{:else}<RefreshCw size={14} />{/if}{tr('Refresh')}
    </button>
  </div>

  <div class="range-progress" class:visible={isFetching} role="progressbar" aria-label={tr('Loading statistics')}></div>

  {#if isInitialLoading || !statistics}
    <div class="analytics-skeleton" aria-label={tr('Loading statistics')}>
      <div class="skeleton chart-skeleton"></div>
      <div class="skeleton chart-skeleton"></div>
      <div class="chart-row">
        <div class="skeleton chart-skeleton"></div>
        <div class="skeleton chart-skeleton"></div>
      </div>
    </div>
  {:else}
    {#if inlineError}
      <div class="analytics-error-banner" role="alert">
        <CircleX size={16} />
        <span>{inlineError}</span>
        <button class="secondary-button compact" onclick={() => load(selectedRange, true)}>{tr('Retry')}</button>
      </div>
    {/if}
    {#if statistics.model_breakdown_incomplete}
      <div class="analytics-warning coverage" role="status">
        <Database size={17} />
        <div>
          <strong>{tr('Charts cover part of the period')}</strong>
          <span>{tr('The retained request history holds {covered} of the {total} requests in this period. The traffic, breakdown, spend and latency charts use the requests that are still retained; the totals above come from the aggregate counters.', { covered: formatCount(statistics.logged_requests), total: formatCount(statistics.total_requests) })}</span>
        </div>
      </div>
    {/if}

    <div class="analytics-content" class:fetching={isFetching}>
      <TrafficSeries {statistics} {locale} {tr} {formatCount} />
      <BreakdownChart {statistics} {locale} {tr} {formatCount} />
      <div class="chart-row">
        <CostPanel {statistics} {locale} {tr} {formatCount} />
        <LatencyPanel {statistics} {locale} {tr} {formatCount} />
      </div>
    </div>
  {/if}
</section>

<style>
  .analytics-actions { display: flex; align-items: center; gap: 9px; margin-bottom: 12px; }
  .range-switch { display: inline-flex; gap: 2px; padding: 3px; border: 2px solid var(--ink); border-radius: 9px; background: var(--paper); box-shadow: 2px 2px 0 var(--ink); }
  .range-switch button { padding: 6px 9px; color: var(--muted); border: 0; border-radius: 6px; background: transparent; font: 600 12px var(--font-sans); cursor: pointer; transition: background 0.15s ease, color 0.15s ease; }
  .range-switch button.active { color: #fff; background: var(--violet); }
  .analytics-skeleton { display: grid; gap: 14px; }
  .chart-skeleton { height: 316px; border-radius: 14px; }
  .range-progress { height: 3px; width: 100%; margin-bottom: 12px; border-radius: 3px; background: transparent; overflow: hidden; opacity: 0; transition: opacity 0.15s ease; }
  .range-progress.visible { opacity: 1; background: color-mix(in srgb, var(--violet) 15%, transparent); }
  .range-progress.visible::after { content: ''; display: block; height: 100%; width: 40%; background: var(--violet); border-radius: 3px; animation: progress-indeterminate 1.1s cubic-bezier(0.4, 0, 0.2, 1) infinite; }
  @keyframes progress-indeterminate { 0% { transform: translateX(-100%); width: 20%; } 50% { transform: translateX(120%); width: 60%; } 100% { transform: translateX(350%); width: 30%; } }
  .analytics-error-banner { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-bottom: 12px; padding: 10px 14px; color: #b91c1c; border: 2px solid #b91c1c; border-radius: 10px; box-shadow: 2px 2px 0 #b91c1c; background: #fef2f2; font-size: 12px; }
  .analytics-error-banner :global(svg) { flex: 0 0 auto; }
  .analytics-error-banner span { flex: 1; }
  :global(:root[data-theme='dark']) .analytics-error-banner { color: #fecaca; border-color: #f87171; box-shadow: 2px 2px 0 #f87171; background: #2b1818; }
  /* The charts are one column wide, so the section reads top to bottom; the two
     secondary panels share a row because each is short and they are read together. */
  .chart-row { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); align-items: start; gap: 14px; margin-bottom: 14px; }
  .analytics-content :global(.chart-card) { margin-bottom: 14px; }
  .analytics-content { transition: opacity 0.2s ease; }
  .analytics-content.fetching { opacity: 0.62; pointer-events: none; }
  .analytics-warning { display: flex; align-items: flex-start; gap: 10px; margin-bottom: 12px; padding: 12px 14px; color: #37475f; border: 2px solid var(--ink); border-radius: 10px; box-shadow: 2px 2px 0 var(--ink); background: #f4f6fa; }
  .analytics-warning :global(svg) { flex: 0 0 auto; margin-top: 1px; }
  .analytics-warning > div { display: grid; gap: 3px; }
  .analytics-warning strong { font-size: 12px; }
  .analytics-warning span { font-size: 12px; line-height: 1.45; }
  :global(:root[data-theme='dark']) .analytics-warning.coverage { color: #c8d2e4; background: #14171f; }
  @media (max-width: 1000px) { .chart-row { grid-template-columns: 1fr; } }
  @media (max-width: 620px) { .analytics-actions { align-items: flex-end; flex-direction: column; } }
</style>
