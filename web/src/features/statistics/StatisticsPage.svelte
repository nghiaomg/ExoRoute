<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Activity, Clock3, CircleCheck, CircleX, RefreshCw } from '@lucide/svelte';
  import GatewayError from '../../components/GatewayError.svelte';
  import PageHeading from '../../components/PageHeading.svelte';
  import { api } from '../../lib/api';
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { RequestStatistics } from '../../lib/types';
  import DonutChart from './DonutChart.svelte';
  import ModelBreakdownTable from './ModelBreakdownTable.svelte';

  export let tr: Translate;
  export let locale: Locale;
  export let onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;

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

  function formatTimestamp(value: string | null): string {
    if (!value) return tr('Not available');
    const parsed = new Date(`${value.replace(' ', 'T')}Z`);
    if (Number.isNaN(parsed.getTime())) return value;
    return new Intl.DateTimeFormat(getIntlLocale(locale), {
      dateStyle: 'medium', timeStyle: 'short', timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    }).format(parsed);
  }

  async function load(range: RequestStatistics['range'] = selectedRange, forceRefresh = false): Promise<void> {
    selectedRange = range;
    inlineError = '';

    const cached = cache[range];
    const isStale = !cached || (Date.now() - cached.timestamp > CACHE_TTL_MS);

    if (cached && !forceRefresh) {
      statistics = cached.data;
      isInitialLoading = false;
      if (!isStale) {
        onConnectionChange('loaded');
        return;
      }
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
    onConnectionChange('loading');

    try {
      const result = await api.statistics(range, controller.signal);
      if (requestGeneration !== generation) return;
      cache[range] = { data: result, timestamp: Date.now() };
      statistics = result;
      onConnectionChange('loaded');
    } catch (error) {
      if (requestGeneration !== generation || controller.signal.aborted) return;
      const formatted = localizedError(error, 'Something went wrong while loading this page.', tr);
      if (!statistics) {
        errorMessage = formatted;
      } else {
        inlineError = formatted;
      }
      onConnectionChange('error');
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

<PageHeading title={tr('Statistics')} subtitle={tr('Traffic performance by API key and requested model.')} {tr}>
  <div class="statistics-actions">
    <div class="range-switch" role="group" aria-label={tr('Statistics time range')}>
      <button class:active={selectedRange === '1d'} aria-pressed={selectedRange === '1d'} onclick={() => load('1d')}>{tr('24 hours')}</button>
      <button class:active={selectedRange === '7d'} aria-pressed={selectedRange === '7d'} onclick={() => load('7d')}>{tr('7 days')}</button>
      <button class:active={selectedRange === '30d'} aria-pressed={selectedRange === '30d'} onclick={() => load('30d')}>{tr('30 days')}</button>
    </div>
    <button class="secondary-button compact" aria-label={tr('Refresh statistics')} disabled={isFetching} onclick={() => load(selectedRange, true)}>
      {#if isFetching}<span class="auth-bootstrap-spinner"></span>{:else}<RefreshCw size={14} />{/if}{tr('Refresh')}
    </button>
  </div>
</PageHeading>

{#if errorMessage && !statistics}
  <GatewayError message={errorMessage} {tr} onRetry={() => load(selectedRange, true)} />
{:else if isInitialLoading || !statistics}
  <div class="statistics-skeleton" aria-label={tr('Loading statistics')}>
    <div class="statistics-kpis">
      <div class="skeleton kpi-skeleton"></div>
      <div class="skeleton kpi-skeleton"></div>
      <div class="skeleton kpi-skeleton"></div>
      <div class="skeleton kpi-skeleton"></div>
    </div>
    <div class="statistics-donuts">
      <div class="skeleton donut-skeleton"></div>
      <div class="skeleton donut-skeleton"></div>
    </div>
    <div class="skeleton details-skeleton"></div>
  </div>
{:else}
  <div class="statistics-container">
    <div class="range-progress" class:visible={isFetching} role="progressbar" aria-label={tr('Loading statistics')}></div>

    {#if inlineError}
      <div class="statistics-error-banner" role="alert">
        <CircleX size={16} />
        <span>{inlineError}</span>
        <button class="secondary-button compact" onclick={() => load(selectedRange, true)}>{tr('Retry')}</button>
      </div>
    {/if}

    <div class="statistics-content" class:fetching={isFetching}>
      {#if !statistics.complete}
        <div class="statistics-warning" role="status"><CircleX size={17} /><div><strong>{tr('Statistics are incomplete')}</strong><span>{tr('{count} telemetry events were dropped. Displayed totals and percentages use the events that were saved.', { count: formatCount(statistics.dropped_events) })}</span></div></div>
      {/if}
      {#if !statistics.aggregation_current}
        <div class="statistics-warning updating" role="status"><Clock3 size={17} /><div><strong>{tr('Rolling totals are catching up')}</strong><span>{tr('The background worker is expiring older minute buckets. Values can temporarily include older traffic.')}</span></div></div>
      {/if}

      <section class="statistics-kpis" aria-label={tr('Traffic summary')}>
        <article class="statistics-kpi"><div><span>{tr('Total requests')}</span><Activity size={17} /></div><strong>{formatCount(statistics.total_requests)}</strong><small>{tr('Across the selected period')}</small></article>
        <article class="statistics-kpi"><div><span>{tr('Success rate')}</span><CircleCheck size={17} /></div><strong>{statistics.success_rate.toFixed(1)}%</strong><small>{tr('{count} successful', { count: formatCount(statistics.successes) })}</small></article>
        <article class="statistics-kpi"><div><span>{tr('Failed requests')}</span><CircleX size={17} /></div><strong>{formatCount(statistics.failures)}</strong><small>{tr('Final client request outcome')}</small></article>
        <article class="statistics-kpi"><div><span>{tr('Average latency')}</span><Clock3 size={17} /></div><strong>{statistics.average_latency_ms.toFixed(1)} <em>ms</em></strong><small>{tr('End-to-end request duration')}</small></article>
      </section>

      <section class="statistics-donuts" aria-label={tr('Traffic breakdown')}>
        <DonutChart title={tr('Requests by API key')} items={statistics.api_keys.top} others={statistics.api_keys.others} total={statistics.total_requests} {tr} {formatCount} />
        <DonutChart title={tr('Requests by requested model')} items={statistics.models.top} others={statistics.models.others} total={statistics.total_requests} {tr} {formatCount} />
      </section>

      <section class="statistics-details">
        <div><span>{tr('Input tokens')}</span><strong>{formatCount(statistics.input_tokens)}</strong></div>
        <div><span>{tr('Output tokens')}</span><strong>{formatCount(statistics.output_tokens)}</strong></div>
        <div><span>{tr('Cached tokens')}</span><strong>{formatCount(statistics.cached_tokens)}</strong></div>
        <div><span>{tr('Cache ratio')}</span><strong>{statistics.cache_ratio.toFixed(1)}%</strong></div>
        <div><span>{tr('Data collected since')}</span><strong>{formatTimestamp(statistics.collection_started_at)}</strong></div>
        <div><span>{tr('Last aggregate update')}</span><strong>{formatTimestamp(statistics.as_of)}</strong></div>
      </section>

      <ModelBreakdownTable
        items={statistics.model_breakdown}
        truncated={statistics.model_breakdown_truncated}
        {tr}
        {locale}
      />
    </div>
  </div>
{/if}

<style>
  .statistics-actions { display: flex; align-items: center; gap: 9px; }
  .range-switch { display: inline-flex; gap: 2px; padding: 3px; border: 1px solid var(--line); border-radius: 9px; background: var(--paper); }
  .range-switch button { padding: 6px 9px; color: var(--muted); border: 0; border-radius: 6px; background: transparent; font: 600 12px var(--font-sans); cursor: pointer; transition: background 0.15s ease, color 0.15s ease; }
  .range-switch button.active { color: #fff; background: var(--violet); }
  .statistics-skeleton { display: grid; gap: 14px; }
  .kpi-skeleton { height: 106px; border-radius: 13px; }
  .donut-skeleton { height: 216px; border-radius: 14px; }
  .details-skeleton { height: 78px; border-radius: 13px; }
  .range-progress { height: 3px; width: 100%; margin-bottom: 12px; border-radius: 3px; background: transparent; overflow: hidden; opacity: 0; transition: opacity 0.15s ease; }
  .range-progress.visible { opacity: 1; background: color-mix(in srgb, var(--violet) 15%, transparent); }
  .range-progress.visible::after { content: ''; display: block; height: 100%; width: 40%; background: var(--violet); border-radius: 3px; animation: progress-indeterminate 1.1s cubic-bezier(0.4, 0, 0.2, 1) infinite; }
  @keyframes progress-indeterminate { 0% { transform: translateX(-100%); width: 20%; } 50% { transform: translateX(120%); width: 60%; } 100% { transform: translateX(350%); width: 30%; } }
  .statistics-error-banner { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-bottom: 12px; padding: 10px 14px; color: #9a2828; border: 1px solid #f3c2c2; border-radius: 10px; background: #fff5f5; font-size: 12px; }
  .statistics-error-banner :global(svg) { flex: 0 0 auto; }
  .statistics-error-banner span { flex: 1; }
  :global(:root[data-theme='dark']) .statistics-error-banner { color: #f28b82; border-color: #5c2b2b; background: #2d1818; }
  .statistics-content { transition: opacity 0.2s ease; }
  .statistics-content.fetching { opacity: 0.62; pointer-events: none; }
  .statistics-kpis { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; margin-bottom: 14px; }
  .statistics-kpi { min-width: 0; padding: 17px 18px; border: 1px solid var(--line); border-radius: 13px; background: var(--paper); }
  .statistics-kpi > div { display: flex; align-items: center; justify-content: space-between; gap: 10px; color: var(--muted); font-size: 12px; }
  .statistics-kpi > div :global(svg) { color: var(--violet); }
  .statistics-kpi > strong { display: block; margin: 13px 0 3px; overflow: hidden; color: var(--ink); font: 700 25px/1.15 var(--font-mono); text-overflow: ellipsis; white-space: nowrap; }
  .statistics-kpi > strong em { color: var(--muted); font: 500 12px var(--font-sans); }
  .statistics-kpi small { color: var(--muted); font-size: 12px; }
  .statistics-donuts { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 14px; margin-bottom: 14px; }
  .statistics-details { display: grid; grid-template-columns: repeat(6, minmax(0, 1fr)); gap: 1px; overflow: hidden; border: 1px solid var(--line); border-radius: 13px; background: var(--line); }
  .statistics-details > div { min-width: 0; display: grid; gap: 8px; padding: 15px 17px; background: var(--paper); }
  .statistics-details span { color: var(--muted); font-size: 12px; }
  .statistics-details strong { overflow: hidden; color: var(--ink); font: 600 12px var(--font-sans); text-overflow: ellipsis; white-space: nowrap; }
  .statistics-warning { display: flex; align-items: flex-start; gap: 10px; margin-bottom: 12px; padding: 12px 14px; color: #9d4827; border: 1px solid #f3d1bf; border-radius: 10px; background: #fff8f4; }
  .statistics-warning :global(svg) { flex: 0 0 auto; margin-top: 1px; }
  .statistics-warning > div { display: grid; gap: 3px; }
  .statistics-warning strong { font-size: 12px; }
  .statistics-warning span { font-size: 12px; line-height: 1.45; }
  .statistics-warning.updating { color: #755621; border-color: #ebd9a9; background: #fffdf5; }
  @media (max-width: 1000px) { .statistics-kpis { grid-template-columns: repeat(2, minmax(0, 1fr)); } .statistics-details { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
  @media (max-width: 720px) { .statistics-donuts { grid-template-columns: 1fr; } }
  @media (max-width: 620px) { .statistics-actions { align-items: flex-end; flex-direction: column; } .statistics-kpis { gap: 8px; } .statistics-kpi { padding: 13px; } .statistics-kpi > strong { font-size: 20px; } .statistics-details { grid-template-columns: 1fr 1fr; } }
</style>
