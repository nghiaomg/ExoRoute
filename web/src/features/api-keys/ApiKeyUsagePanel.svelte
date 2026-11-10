<script lang="ts">
  import { Activity, CircleCheck, CircleX, Clock3, RefreshCw } from '@lucide/svelte';
  import { onMount } from 'svelte';
  import { api } from '../../lib/api';
  import { localizedError } from '../../lib/errors';
  import { formatCostMicroUsd, formatCount, formatPercent, type Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import type { GatewayApiKeyStatistics } from '../../lib/types';

  interface Props {
    apiKeyId: string;
    tr: Translate;
    locale: Locale;
  }
  let { apiKeyId, tr, locale }: Props = $props();

  const RANGES: GatewayApiKeyStatistics['range'][] = ['1d', '7d', '30d'];
  const RANGE_KEYS: Record<GatewayApiKeyStatistics['range'], string> = {
    '1d': '24 hours',
    '7d': '7 days',
    '30d': '30 days',
  };

  let range = $state<GatewayApiKeyStatistics['range']>('1d');
  let statistics = $state<GatewayApiKeyStatistics | null>(null);
  let loading = $state(true);
  let errorMessage = $state('');

  // A range switch fires a new request while the previous one may still be in
  // flight, so each load owns a generation number: only the newest response is
  // allowed to publish, and abandoning the old fetch also cancels its body.
  let generation = 0;
  let controller: AbortController | null = null;

  const modelRows = $derived.by(() => {
    if (!statistics) return [];
    const rows = statistics.models.top.map((model) => ({
      id: model.id,
      label: model.label,
      requests: model.requests,
      share: model.percentage,
    }));
    if (statistics.models.others > 0) {
      const total = statistics.total_requests;
      rows.push({
        id: 'others',
        label: tr('Others'),
        requests: statistics.models.others,
        share: total > 0 ? (statistics.models.others / total) * 100 : 0,
      });
    }
    return rows;
  });
  const maxModelRequests = $derived(Math.max(0, ...modelRows.map((row) => row.requests)));

  async function load(next: GatewayApiKeyStatistics['range']): Promise<void> {
    const requestGeneration = ++generation;
    controller?.abort();
    const activeController = new AbortController();
    controller = activeController;
    range = next;
    loading = true;
    errorMessage = '';
    try {
      const result = await api.apiKeyStatistics(apiKeyId, next, activeController.signal);
      if (requestGeneration !== generation || activeController.signal.aborted) return;
      statistics = result;
    } catch (cause) {
      if (requestGeneration !== generation || activeController.signal.aborted) return;
      errorMessage = localizedError(cause, 'Could not load this API key’s usage.', tr);
    } finally {
      if (requestGeneration === generation) loading = false;
    }
  }

  // The panel is mounted only while its dialog is open, so the first load can
  // start on mount and cancelling the in-flight request is the whole teardown.
  onMount(() => {
    void load('1d');
    return () => {
      generation += 1;
      controller?.abort();
      controller = null;
    };
  });
</script>

<section class="usage-panel" aria-label={tr('API key usage')}>
  <div class="usage-heading">
    <h3>{tr('API key usage')}</h3>
    <div class="usage-controls">
      <div class="range-switch" role="group" aria-label={tr('Statistics time range')}>
        {#each RANGES as candidate (candidate)}
          <button
            type="button"
            class:active={range === candidate}
            aria-pressed={range === candidate}
            disabled={loading}
            onclick={() => void load(candidate)}
          >{tr(RANGE_KEYS[candidate])}</button>
        {/each}
      </div>
      <button
        class="usage-refresh"
        type="button"
        aria-label={tr('Refresh')}
        disabled={loading}
        onclick={() => void load(range)}
      >
        {#if loading}<span class="auth-bootstrap-spinner"></span>{:else}<RefreshCw size={13} />{/if}
      </button>
    </div>
  </div>

  {#if loading && !statistics}
    <p class="usage-note" role="status">{tr('Loading statistics')}…</p>
  {:else if errorMessage && !statistics}
    <div class="usage-error" role="alert">
      <span>{errorMessage}</span>
      <button class="usage-retry" type="button" onclick={() => void load(range)}>{tr('Retry')}</button>
    </div>
  {:else if statistics}
    {#if !statistics.complete}
      <div class="usage-warning" role="status">
        <CircleX size={15} />
        <div>
          <strong>{tr('Statistics are incomplete')}</strong>
          <span>{tr('{count} telemetry events were dropped. Displayed totals and percentages use the events that were saved.', { count: formatCount(statistics.dropped_events, locale) })}</span>
        </div>
      </div>
    {/if}
    {#if !statistics.aggregation_current}
      <div class="usage-warning" role="status">
        <Clock3 size={15} />
        <div>
          <strong>{tr('Rolling totals are catching up')}</strong>
          <span>{tr('The background worker is expiring older minute buckets. Values can temporarily include older traffic.')}</span>
        </div>
      </div>
    {/if}

    <div class="usage-kpis">
      <article class="usage-kpi">
        <span>{tr('Total requests')}<Activity size={14} /></span>
        <strong>{formatCount(statistics.total_requests, locale)}</strong>
        <small>{tr('Across the selected period')}</small>
      </article>
      <article class="usage-kpi">
        <span>{tr('Success rate')}<CircleCheck size={14} /></span>
        <strong>{formatPercent(statistics.success_rate, locale)}</strong>
        <small>{tr('{count} successful', { count: formatCount(statistics.successes, locale) })}</small>
      </article>
      <article class="usage-kpi">
        <span>{tr('Failed requests')}<CircleX size={14} /></span>
        <strong>{formatCount(statistics.failures, locale)}</strong>
        <small>{tr('Final client request outcome')}</small>
      </article>
      <article class="usage-kpi">
        <span>{tr('Average latency')}<Clock3 size={14} /></span>
        <strong>{formatCount(Math.round(statistics.average_latency_ms), locale)}<em>ms</em></strong>
        <small>{tr('End-to-end request duration')}</small>
      </article>
    </div>

    <div class="usage-tokens">
      <div><span>{tr('Input tokens')}</span><strong>{formatCount(statistics.input_tokens, locale)}</strong></div>
      <div><span>{tr('Output tokens')}</span><strong>{formatCount(statistics.output_tokens, locale)}</strong></div>
      <div><span>{tr('Cached tokens')}</span><strong>{formatCount(statistics.cached_tokens, locale)}</strong></div>
      <div><span>{tr('Cache ratio')}</span><strong>{formatPercent(statistics.cache_ratio, locale)}</strong></div>
      <div><span>{tr('Spend')}</span><strong>{formatCostMicroUsd(statistics.cost.cost_reported_micro_usd)}</strong></div>
    </div>

    {#if modelRows.length}
      <div class="usage-models">
        <h4>{tr('Requests by model for this key')}</h4>
        <ul>
          {#each modelRows as row (row.id)}
            <li>
              <div class="usage-model-row">
                <span class="usage-model-name" title={row.label}>{row.label}</span>
                <span class="usage-model-count">{formatCount(row.requests, locale)}</span>
              </div>
              <div class="usage-model-track">
                <span style="width: {maxModelRequests > 0 ? Math.max(2, (row.requests / maxModelRequests) * 100) : 0}%"></span>
              </div>
            </li>
          {/each}
        </ul>
      </div>
    {:else}
      <p class="usage-note">{tr('No traffic for this API key in the selected period.')}</p>
    {/if}

    {#if errorMessage}
      <div class="usage-error" role="alert">
        <span>{errorMessage}</span>
        <button class="usage-retry" type="button" onclick={() => void load(range)}>{tr('Retry')}</button>
      </div>
    {/if}
  {/if}
</section>

<style>
  .usage-panel {
    display: grid;
    gap: 12px;
    min-width: 0;
    padding-top: 14px;
    border-top: 1px solid var(--line);
  }
  .usage-heading {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .usage-heading h3 {
    margin: 0;
    color: var(--ink);
    font: 650 14px var(--font-sans);
  }
  .usage-controls {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .range-switch {
    display: inline-flex;
    gap: 2px;
    padding: 3px;
    border: 1px solid var(--line);
    border-radius: 9px;
    background: var(--surface-sunken);
  }
  .range-switch button {
    padding: 5px 9px;
    color: var(--muted);
    border: 0;
    border-radius: 6px;
    background: transparent;
    font: 600 12px var(--font-sans);
    cursor: pointer;
    transition: background 0.15s ease, color 0.15s ease;
  }
  .range-switch button:hover:not(:disabled):not(.active) {
    color: var(--ink);
  }
  .range-switch button.active {
    color: var(--accent-contrast);
    background: var(--accent);
  }
  .range-switch button:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .usage-refresh {
    display: grid;
    width: 28px;
    height: 28px;
    place-items: center;
    color: var(--muted);
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--surface);
    cursor: pointer;
  }
  .usage-refresh:hover:not(:disabled) {
    color: var(--accent-deep);
  }
  .usage-refresh:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .usage-kpis {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 8px;
  }
  .usage-kpi {
    display: grid;
    gap: 3px;
    min-width: 0;
    padding: 11px 13px;
    border: 1px solid var(--line);
    border-radius: 11px;
    background: var(--surface);
  }
  .usage-kpi > span {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    color: var(--muted);
    font-size: 11px;
  }
  .usage-kpi > span :global(svg) {
    flex: 0 0 auto;
    color: var(--accent);
  }
  .usage-kpi > strong {
    overflow: hidden;
    color: var(--ink);
    font: 700 19px/1.2 var(--font-mono);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .usage-kpi > strong em {
    margin-left: 3px;
    color: var(--muted);
    font: 500 11px var(--font-sans);
  }
  .usage-kpi > small {
    color: var(--muted);
    font-size: 11px;
  }
  .usage-tokens {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: 1px;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: 11px;
    background: var(--line);
  }
  .usage-tokens > div {
    display: grid;
    gap: 5px;
    min-width: 0;
    padding: 10px 12px;
    background: var(--surface);
  }
  .usage-tokens span {
    color: var(--muted);
    font-size: 11px;
  }
  .usage-tokens strong {
    overflow: hidden;
    color: var(--ink);
    font: 600 12px var(--font-sans);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .usage-models {
    display: grid;
    gap: 8px;
    min-width: 0;
  }
  .usage-models h4 {
    margin: 0;
    color: var(--ink);
    font: 600 12px var(--font-sans);
  }
  .usage-models ul {
    display: grid;
    gap: 7px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .usage-models li {
    display: grid;
    gap: 4px;
    min-width: 0;
  }
  .usage-model-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
    min-width: 0;
  }
  .usage-model-name {
    overflow: hidden;
    color: var(--ink);
    font-size: 12px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .usage-model-count {
    flex: 0 0 auto;
    color: var(--muted);
    font: 600 11px var(--font-mono);
  }
  .usage-model-track {
    height: 5px;
    overflow: hidden;
    border-radius: 999px;
    background: var(--surface-sunken);
  }
  .usage-model-track > span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
  }
  .usage-note {
    margin: 0;
    color: var(--muted);
    font-size: 12px;
  }
  .usage-error {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    color: var(--db-danger, #b84935);
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--db-accent-tint, rgba(237, 118, 94, 0.08));
    font-size: 12px;
  }
  .usage-retry {
    margin-left: auto;
    padding: 4px 10px;
    color: inherit;
    border: 1px solid currentColor;
    border-radius: 7px;
    background: transparent;
    font: 600 11px var(--font-sans);
    cursor: pointer;
  }
  .usage-warning {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 9px 11px;
    color: var(--accent-deep);
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--surface-sunken);
  }
  .usage-warning :global(svg) {
    flex: 0 0 auto;
    margin-top: 1px;
  }
  .usage-warning > div {
    display: grid;
    gap: 2px;
  }
  .usage-warning strong {
    font-size: 12px;
  }
  .usage-warning span {
    font-size: 11.5px;
    line-height: 1.45;
  }

  @media (max-width: 720px) {
    .usage-kpis {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .usage-tokens {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
</style>
