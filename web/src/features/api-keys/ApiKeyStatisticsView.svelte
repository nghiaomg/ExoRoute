<script lang="ts">
  import { onDestroy } from 'svelte';
  import { Activity, CircleCheck, CircleX, Clock3 } from '@lucide/svelte';
  import DonutChart from '../statistics/DonutChart.svelte';
  import { api } from '../../lib/api';
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
  import { localizedError } from '../../lib/errors';
  import type { GatewayApiKeyStatistics } from '../../lib/types';

  export let apiKeyId: string;
  export let tr: Translate;
  export let locale: Locale;

  let selectedRange: GatewayApiKeyStatistics['range'] = '1d';
  let statistics: GatewayApiKeyStatistics | null = null;
  let isLoading = false;
  let errorMessage = '';
  let generation = 0;
  let loadedKeyId = '';
  let activeController: AbortController | null = null;

  $: numberFormat = new Intl.NumberFormat(getIntlLocale(locale));
  $: if (apiKeyId !== loadedKeyId) {
    loadedKeyId = apiKeyId;
    selectedRange = '1d';
    statistics = null;
    errorMessage = '';
    void load(apiKeyId, '1d');
  }

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

  async function load(id: string, range: GatewayApiKeyStatistics['range']): Promise<void> {
    selectedRange = range;
    const requestGeneration = ++generation;
    activeController?.abort();
    const controller = new AbortController();
    activeController = controller;
    isLoading = true;
    errorMessage = '';
    try {
      const result = await api.apiKeyStatistics(id, range, controller.signal);
      if (requestGeneration !== generation || controller.signal.aborted) return;
      statistics = result;
    } catch (error) {
      if (requestGeneration !== generation || controller.signal.aborted) return;
      errorMessage = localizedError(error, 'Could not load this API key’s usage.', tr);
    } finally {
      if (requestGeneration === generation) isLoading = false;
    }
  }

  onDestroy(() => {
    generation += 1;
    activeController?.abort();
  });
</script>

<section class="api-key-statistics" aria-label={tr('Traffic summary')}>
  <div class="api-key-details-range" role="group" aria-label={tr('Statistics time range')}>
    <button type="button" class:active={selectedRange === '1d'} aria-pressed={selectedRange === '1d'} disabled={isLoading} onclick={() => load(apiKeyId, '1d')}>{tr('24 hours')}</button>
    <button type="button" class:active={selectedRange === '7d'} aria-pressed={selectedRange === '7d'} disabled={isLoading} onclick={() => load(apiKeyId, '7d')}>{tr('7 days')}</button>
    <button type="button" class:active={selectedRange === '30d'} aria-pressed={selectedRange === '30d'} disabled={isLoading} onclick={() => load(apiKeyId, '30d')}>{tr('30 days')}</button>
  </div>

  {#if isLoading && !statistics}
    <div class="api-key-details-loading" role="status"><span class="auth-bootstrap-spinner"></span><span>{tr('Loading statistics')}</span></div>
  {:else if errorMessage && !statistics}
    <div class="form-error" role="alert">{errorMessage}</div>
    <div class="api-key-statistics-retry"><button type="button" class="secondary-button compact" onclick={() => load(apiKeyId, selectedRange)}>{tr('Retry')}</button></div>
  {:else if statistics}
    {#if !statistics.complete}
      <div class="api-key-details-warning" role="status"><CircleX size={17} /><div><strong>{tr('Statistics are incomplete')}</strong><span>{tr('{count} telemetry events were dropped. Displayed totals and percentages use the events that were saved.', { count: formatCount(statistics.dropped_events) })}</span></div></div>
    {/if}
    {#if !statistics.aggregation_current}
      <div class="api-key-details-warning updating" role="status"><Clock3 size={17} /><div><strong>{tr('Rolling totals are catching up')}</strong><span>{tr('The background worker is expiring older minute buckets. Values can temporarily include older traffic.')}</span></div></div>
    {/if}
    <section class="api-key-details-kpis" aria-label={tr('Traffic summary')}>
      <article class="api-key-details-kpi"><div><span>{tr('Total requests')}</span><Activity size={17} /></div><strong>{formatCount(statistics.total_requests)}</strong><small>{tr('Across the selected period')}</small></article>
      <article class="api-key-details-kpi"><div><span>{tr('Success rate')}</span><CircleCheck size={17} /></div><strong>{statistics.success_rate.toFixed(1)}%</strong><small>{tr('{count} successful', { count: formatCount(statistics.successes) })}</small></article>
      <article class="api-key-details-kpi"><div><span>{tr('Failed requests')}</span><CircleX size={17} /></div><strong>{formatCount(statistics.failures)}</strong><small>{tr('Final client request outcome')}</small></article>
      <article class="api-key-details-kpi"><div><span>{tr('Average latency')}</span><Clock3 size={17} /></div><strong>{statistics.average_latency_ms.toFixed(1)} <em>ms</em></strong><small>{tr('End-to-end request duration')}</small></article>
    </section>
    <section class="api-key-details-tokens">
      <div><span>{tr('Input tokens')}</span><strong>{formatCount(statistics.input_tokens)}</strong></div>
      <div><span>{tr('Output tokens')}</span><strong>{formatCount(statistics.output_tokens)}</strong></div>
      <div><span>{tr('Cached tokens')}</span><strong>{formatCount(statistics.cached_tokens)}</strong></div>
      <div><span>{tr('Cache ratio')}</span><strong>{statistics.cache_ratio.toFixed(1)}%</strong></div>
      <div><span>{tr('Data collected since')}</span><strong>{formatTimestamp(statistics.collection_started_at)}</strong></div>
      <div><span>{tr('Last aggregate update')}</span><strong>{formatTimestamp(statistics.as_of)}</strong></div>
    </section>
    {#if statistics.models.top.length || statistics.models.others > 0}
      <DonutChart title={tr('Requests by model for this key')} items={statistics.models.top} others={statistics.models.others} total={statistics.total_requests} {tr} {formatCount} />
    {:else}
      <p class="api-key-statistics-empty">{tr('No traffic for this API key in the selected period.')}</p>
    {/if}
  {/if}
</section>

<style>
  .api-key-statistics { display: grid; gap: 14px; }
  .api-key-details-range { display: inline-flex; align-self: flex-start; gap: 2px; padding: 3px; border: 1px solid var(--line); border-radius: 9px; background: var(--paper); }
  .api-key-details-range button { padding: 6px 9px; color: var(--muted); border: 0; border-radius: 6px; background: transparent; font: 600 12px var(--font-sans); cursor: pointer; transition: background 0.15s ease, color 0.15s ease; }
  .api-key-details-range button.active { color: #fff; background: var(--violet); }
  .api-key-details-loading { display: flex; align-items: center; gap: 10px; padding: 18px 2px; color: var(--muted); font-size: 12px; }
  .api-key-statistics-retry { display: flex; }
  .api-key-details-kpis { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; }
  .api-key-details-kpi { min-width: 0; padding: 17px 18px; border: 1px solid var(--line); border-radius: 13px; background: var(--paper); }
  .api-key-details-kpi > div { display: flex; align-items: center; justify-content: space-between; gap: 10px; color: var(--muted); font-size: 12px; }
  .api-key-details-kpi > div :global(svg) { color: var(--violet); }
  .api-key-details-kpi > strong { display: block; margin: 13px 0 3px; overflow: hidden; color: var(--ink); font: 700 25px/1.15 var(--font-mono); text-overflow: ellipsis; white-space: nowrap; }
  .api-key-details-kpi > strong em { color: var(--muted); font: 500 12px var(--font-sans); }
  .api-key-details-kpi small { color: var(--muted); font-size: 12px; }
  .api-key-details-tokens { display: grid; grid-template-columns: repeat(6, minmax(0, 1fr)); gap: 1px; overflow: hidden; border: 1px solid var(--line); border-radius: 13px; background: var(--line); }
  .api-key-details-tokens > div { min-width: 0; display: grid; gap: 8px; padding: 15px 17px; background: var(--paper); }
  .api-key-details-tokens span { color: var(--muted); font-size: 12px; }
  .api-key-details-tokens strong { overflow: hidden; color: var(--ink); font: 600 12px var(--font-sans); text-overflow: ellipsis; white-space: nowrap; }
  .api-key-details-warning { display: flex; align-items: flex-start; gap: 10px; padding: 12px 14px; color: #9d4827; border: 1px solid #f3d1bf; border-radius: 10px; background: #fff8f4; }
  .api-key-details-warning :global(svg) { flex: 0 0 auto; margin-top: 1px; }
  .api-key-details-warning > div { display: grid; gap: 3px; }
  .api-key-details-warning strong { font-size: 12px; }
  .api-key-details-warning span { font-size: 12px; line-height: 1.45; }
  .api-key-details-warning.updating { color: #755621; border-color: #ebd9a9; background: #fffdf5; }
  .api-key-statistics-empty { margin: 0; color: var(--muted); font-size: 12px; }
  @media (max-width: 900px) { .api-key-details-kpis { grid-template-columns: repeat(2, minmax(0, 1fr)); } .api-key-details-tokens { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
  @media (max-width: 620px) { .api-key-details-kpis { gap: 8px; } .api-key-details-kpi { padding: 13px; } .api-key-details-kpi > strong { font-size: 20px; } .api-key-details-tokens { grid-template-columns: 1fr 1fr; } }
</style>
