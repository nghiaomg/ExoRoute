<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import {
    Activity,
    ArrowLeft,
    Calendar,
    Check,
    CircleCheck,
    CircleX,
    Clock3,
    Copy,
    KeyRound,
    RefreshCw,
    Trash2,
    Zap,
  } from '@lucide/svelte';
  import { api } from '../../lib/api';
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import { formatDate, type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { GatewayApiKey, GatewayApiKeyStatistics } from '../../lib/types';
  import DonutChart from '../statistics/DonutChart.svelte';
  import ModelBreakdownTable from '../statistics/ModelBreakdownTable.svelte';

  export let apiKey: GatewayApiKey;
  export let tr: Translate;
  export let locale: Locale;
  export let onBack: () => void;
  export let onRevoke: (key: GatewayApiKey) => void;

  let selectedRange: GatewayApiKeyStatistics['range'] = '1d';
  let statistics: GatewayApiKeyStatistics | null = null;
  let isLoadingStats = false;
  let statsErrorMessage = '';
  let generation = 0;
  let activeStatsController: AbortController | null = null;
  let copiedKeyId = false;
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  $: numberFormat = new Intl.NumberFormat(getIntlLocale(locale));

  function formatCount(value: number): string {
    return numberFormat.format(Number.isFinite(value) ? value : 0);
  }

  function formatTimestamp(value: string | null): string {
    if (!value) return tr('Not available');
    const parsed = new Date(`${value.replace(' ', 'T')}Z`);
    if (Number.isNaN(parsed.getTime())) return value;
    return new Intl.DateTimeFormat(getIntlLocale(locale), {
      dateStyle: 'medium',
      timeStyle: 'short',
      timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    }).format(parsed);
  }

  async function copyId(): Promise<void> {
    if (!apiKey?.id) return;
    try {
      await navigator.clipboard.writeText(apiKey.id);
      copiedKeyId = true;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => {
        copiedKeyId = false;
      }, 2000);
    } catch {
      // Clipboard copy fallback
    }
  }

  async function loadStatistics(range: GatewayApiKeyStatistics['range'] = selectedRange): Promise<void> {
    selectedRange = range;
    const requestGeneration = ++generation;
    activeStatsController?.abort();
    const controller = new AbortController();
    activeStatsController = controller;
    isLoadingStats = true;
    statsErrorMessage = '';

    try {
      const result = await api.apiKeyStatistics(apiKey.id, range, controller.signal);
      if (requestGeneration !== generation || controller.signal.aborted) return;
      statistics = result;
    } catch (error) {
      if (requestGeneration !== generation || controller.signal.aborted) return;
      statsErrorMessage = localizedError(error, 'Could not load this API key’s usage.', tr);
    } finally {
      if (requestGeneration === generation) isLoadingStats = false;
    }
  }

  function handleRefresh(): void {
    void loadStatistics(selectedRange);
  }

  onMount(() => {
    if (typeof window !== 'undefined') {
      window.scrollTo({ top: 0, behavior: 'instant' });
    }
    void loadStatistics('1d');
  });

  onDestroy(() => {
    generation += 1;
    activeStatsController?.abort();
    if (copyTimer) clearTimeout(copyTimer);
  });
</script>

<div class="api-key-detail-page">
  <!-- Top Navigation & Back Button -->
  <div class="detail-page-nav">
    <button type="button" class="back-link-btn" onclick={onBack}>
      <ArrowLeft size={16} />
      <span>{tr('Back to API keys')}</span>
    </button>
  </div>

  <!-- Hero Banner -->
  <header class="detail-hero-banner api-key-hero">
    <div class="detail-hero-main">
      <div class="detail-avatar-wrap">
        <div class="api-key-avatar">
          <KeyRound size={26} />
        </div>
      </div>

      <div class="detail-hero-info">
        <div class="detail-title-row">
          <h1 class="detail-title">{apiKey.name}</h1>
          <code class="detail-id-chip">{apiKey.id}</code>
          <button
            type="button"
            class="detail-copy-btn"
            title={copiedKeyId ? tr('Key ID copied') : tr('Copy Key ID')}
            onclick={copyId}
          >
            {#if copiedKeyId}
              <Check size={14} class="copy-success-icon" />
              <span>{tr('Key ID copied')}</span>
            {:else}
              <Copy size={14} />
              <span>{tr('Copy Key ID')}</span>
            {/if}
          </button>
          <span class="state-label" class:enabled={apiKey.enabled}>
            <i></i>{tr(apiKey.enabled ? 'Enabled' : 'Disabled')}
          </span>
        </div>

        <div class="detail-tags-row">
          <span class="detail-meta-pill">
            <Calendar size={13} />
            {tr('Created')}: {formatDate(apiKey.created_at, locale)}
          </span>
          <span class="detail-meta-pill">
            <Clock3 size={13} />
            {tr('Last used')}: {apiKey.last_used_at ? formatDate(apiKey.last_used_at, locale) : tr('Never used')}
          </span>
          <span class="detail-meta-pill">
            <Zap size={13} />
            {tr('API requests')}: {(apiKey.request_count ?? 0).toLocaleString(getIntlLocale(locale))}
          </span>
        </div>
      </div>
    </div>

    <div class="detail-hero-actions">
      <button
        type="button"
        class="detail-delete-btn"
        title={tr('Revoke {name}', { name: apiKey.name })}
        onclick={() => onRevoke(apiKey)}
      >
        <Trash2 size={15} />
        {tr('Revoke key')}
      </button>
    </div>
  </header>

  <!-- Statistics Controls & Section Header -->
  <div class="api-key-detail-controls">
    <div class="api-key-details-range" role="group" aria-label={tr('Statistics time range')}>
      <button
        type="button"
        class:active={selectedRange === '1d'}
        aria-pressed={selectedRange === '1d'}
        disabled={isLoadingStats}
        onclick={() => loadStatistics('1d')}
      >
        {tr('24 hours')}
      </button>
      <button
        type="button"
        class:active={selectedRange === '7d'}
        aria-pressed={selectedRange === '7d'}
        disabled={isLoadingStats}
        onclick={() => loadStatistics('7d')}
      >
        {tr('7 days')}
      </button>
      <button
        type="button"
        class:active={selectedRange === '30d'}
        aria-pressed={selectedRange === '30d'}
        disabled={isLoadingStats}
        onclick={() => loadStatistics('30d')}
      >
        {tr('30 days')}
      </button>
    </div>

    <button
      type="button"
      class="secondary-button compact"
      disabled={isLoadingStats}
      onclick={handleRefresh}
    >
      {#if isLoadingStats}
        <span class="auth-bootstrap-spinner"></span>
      {:else}
        <RefreshCw size={14} />
      {/if}
      {tr('Refresh')}
    </button>
  </div>

  <!-- Warnings if telemetry dropped or rolling totals catch up -->
  {#if statistics}
    {#if !statistics.complete}
      <div class="api-key-details-warning" role="status">
        <CircleX size={17} />
        <div>
          <strong>{tr('Statistics are incomplete')}</strong>
          <span>
            {tr('{count} telemetry events were dropped. Displayed totals and percentages use the events that were saved.', {
              count: formatCount(statistics.dropped_events),
            })}
          </span>
        </div>
      </div>
    {/if}
    {#if !statistics.aggregation_current}
      <div class="api-key-details-warning updating" role="status">
        <Clock3 size={17} />
        <div>
          <strong>{tr('Rolling totals are catching up')}</strong>
          <span>{tr('The background worker is expiring older minute buckets. Values can temporarily include older traffic.')}</span>
        </div>
      </div>
    {/if}
  {/if}

  {#if statsErrorMessage && !statistics}
    <div class="form-error" role="alert">{statsErrorMessage}</div>
  {:else if isLoadingStats && !statistics}
    <div class="api-key-details-loading" role="status">
      <span class="auth-bootstrap-spinner"></span>
      <span>{tr('Loading statistics')}</span>
    </div>
  {:else if statistics}
    <!-- KPIs Summary -->
    <section class="api-key-details-kpis" aria-label={tr('Traffic summary')}>
      <article class="api-key-details-kpi">
        <div>
          <span>{tr('Total requests')}</span>
          <Activity size={17} />
        </div>
        <strong>{formatCount(statistics.total_requests)}</strong>
        <small>{tr('Across the selected period')}</small>
      </article>

      <article class="api-key-details-kpi">
        <div>
          <span>{tr('Success rate')}</span>
          <CircleCheck size={17} />
        </div>
        <strong>{statistics.success_rate.toFixed(1)}%</strong>
        <small>{tr('{count} successful', { count: formatCount(statistics.successes) })}</small>
      </article>

      <article class="api-key-details-kpi">
        <div>
          <span>{tr('Failed requests')}</span>
          <CircleX size={17} />
        </div>
        <strong>{formatCount(statistics.failures)}</strong>
        <small>{tr('Final client request outcome')}</small>
      </article>

      <article class="api-key-details-kpi">
        <div>
          <span>{tr('Average latency')}</span>
          <Clock3 size={17} />
        </div>
        <strong>{statistics.average_latency_ms.toFixed(1)} <em>ms</em></strong>
        <small>{tr('End-to-end request duration')}</small>
      </article>
    </section>

    <!-- Token Breakdown -->
    <section class="api-key-details-tokens" aria-label={tr('Token usage')}>
      <div>
        <span>{tr('Input tokens')}</span>
        <strong>{formatCount(statistics.input_tokens)}</strong>
      </div>
      <div>
        <span>{tr('Output tokens')}</span>
        <strong>{formatCount(statistics.output_tokens)}</strong>
      </div>
      <div>
        <span>{tr('Cached tokens')}</span>
        <strong>{formatCount(statistics.cached_tokens)}</strong>
      </div>
      <div>
        <span>{tr('Cache ratio')}</span>
        <strong>{statistics.cache_ratio.toFixed(1)}%</strong>
      </div>
      <div>
        <span>{tr('Data collected since')}</span>
        <strong>{formatTimestamp(statistics.collection_started_at)}</strong>
      </div>
      <div>
        <span>{tr('Last aggregate update')}</span>
        <strong>{formatTimestamp(statistics.as_of)}</strong>
      </div>
    </section>

    <!-- Donut Chart of Models -->
    {#if statistics.models.top.length || statistics.models.others > 0}
      <div class="api-key-donut-wrap">
        <DonutChart
          title={tr('Requests by model for this key')}
          items={statistics.models.top}
          others={statistics.models.others}
          total={statistics.total_requests}
          {tr}
          {formatCount}
        />
      </div>
    {:else}
      <div class="api-key-empty-notice">
        <p>{tr('No traffic for this API key in the selected period.')}</p>
      </div>
    {/if}
    <ModelBreakdownTable
      items={statistics.model_breakdown}
      truncated={statistics.model_breakdown_truncated}
      {tr}
      {locale}
    />
  {/if}

</div>

<style>
  .api-key-detail-page {
    display: flex;
    flex-direction: column;
    gap: 16px;
    width: 100%;
    margin-bottom: 32px;
  }
  .api-key-hero {
    margin-bottom: 4px;
  }
  .api-key-avatar {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 60px;
    height: 60px;
    border-radius: 14px;
    background: linear-gradient(135deg, rgba(124, 58, 237, 0.12) 0%, rgba(109, 40, 217, 0.22) 100%);
    color: var(--violet, #7c3aed);
    border: 1px solid rgba(124, 58, 237, 0.18);
  }
  .detail-tags-row :global(svg) {
    color: var(--muted);
  }
  .api-key-detail-controls {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 12px;
    margin-top: 4px;
  }
  .api-key-details-range {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 3px;
    border: 1px solid var(--line);
    border-radius: 9px;
    background: var(--paper);
  }
  .api-key-details-range button {
    padding: 6px 12px;
    color: var(--muted);
    border: 0;
    border-radius: 6px;
    background: transparent;
    font: 600 12px var(--font-sans);
    cursor: pointer;
    transition: background 0.15s ease, color 0.15s ease;
  }
  .api-key-details-range button.active {
    color: #ffffff;
    background: var(--violet, #7c3aed);
  }
  .api-key-details-loading {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 18px 2px;
    color: var(--muted);
    font-size: 12px;
  }
  .api-key-details-kpis {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 12px;
  }
  .api-key-details-kpi {
    min-width: 0;
    padding: 18px 20px;
    border: 1px solid var(--line);
    border-radius: 13px;
    background: var(--paper);
  }
  .api-key-details-kpi > div {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    color: var(--muted);
    font-size: 12px;
  }
  .api-key-details-kpi > div :global(svg) {
    color: var(--violet, #7c3aed);
  }
  .api-key-details-kpi > strong {
    display: block;
    margin: 12px 0 3px;
    overflow: hidden;
    color: var(--ink);
    font: 700 24px/1.15 var(--font-mono);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .api-key-details-kpi > strong em {
    color: var(--muted);
    font: 500 12px var(--font-sans);
  }
  .api-key-details-kpi small {
    color: var(--muted);
    font-size: 12px;
  }
  .api-key-details-tokens {
    display: grid;
    grid-template-columns: repeat(6, minmax(0, 1fr));
    gap: 1px;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: 13px;
    background: var(--line);
  }
  .api-key-details-tokens > div {
    min-width: 0;
    display: grid;
    gap: 6px;
    padding: 14px 16px;
    background: var(--paper);
  }
  .api-key-details-tokens span {
    color: var(--muted);
    font-size: 12px;
  }
  .api-key-details-tokens strong {
    overflow: hidden;
    color: var(--ink);
    font: 600 13px var(--font-sans);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .api-key-details-warning {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 14px;
    color: #9d4827;
    border: 1px solid #f3d1bf;
    border-radius: 10px;
    background: #fff8f4;
  }
  .api-key-details-warning :global(svg) {
    flex: 0 0 auto;
    margin-top: 1px;
  }
  .api-key-details-warning > div {
    display: grid;
    gap: 3px;
  }
  .api-key-details-warning strong {
    font-size: 12px;
  }
  .api-key-details-warning span {
    font-size: 12px;
    line-height: 1.45;
  }
  .api-key-details-warning.updating {
    color: #755621;
    border-color: #ebd9a9;
    background: #fffdf5;
  }
  .api-key-donut-wrap {
    padding: 20px;
    border: 1px solid var(--line);
    border-radius: 14px;
    background: var(--paper);
  }
  .api-key-empty-notice {
    padding: 24px;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--paper);
    text-align: center;
    color: var(--muted);
    font-size: 13px;
  }
  @media (max-width: 960px) {
    .api-key-details-kpis {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .api-key-details-tokens {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
  }
  @media (max-width: 620px) {
    .api-key-details-kpis {
      grid-template-columns: 1fr;
    }
    .api-key-details-tokens {
      grid-template-columns: 1fr 1fr;
    }
  }
</style>
