<script lang="ts">
  import Button from '../../components/ui/Button.svelte';
  import EmptyState from '../../components/ui/EmptyState.svelte';
  import StatsCard from '../../components/ui/StatsCard.svelte';
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import { localizedError } from '../../lib/errors';
  import { formatCount, formatCompactTokenTotal, formatCostMicroUsd, formatPercent, type Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import { showDashboardToast, submitTopSearch } from '../../lib/dashboard-ui';
  import type { DashboardPageRoute } from '../../lib/navigation';
  import type { GatewayApiKey, GatewayApiKeyStatistics } from '../../lib/types';
  import { formatUtcTimestamp } from './format-utc';
  import ApiKeyDonutChart from './ApiKeyDonutChart.svelte';

  interface Props {
    apiKey: GatewayApiKey | null;
    keyId: string;
    loading: boolean;
    error: string;
    tr: Translate;
    locale: Locale;
    onBack: () => void;
    onEdit: (key: GatewayApiKey) => void;
    onToggle: (key: GatewayApiKey) => void;
    onNavigate: (page: DashboardPageRoute) => void;
    onRetry: () => void;
  }

  let { apiKey, keyId, loading, error, tr, locale, onBack, onEdit, onToggle, onNavigate, onRetry }: Props = $props();

  type Range = GatewayApiKeyStatistics['range'];
  const RANGES: Range[] = ['1d', '7d', '30d'];
  const RANGE_KEYS: Record<Range, string> = { '1d': '24 hours', '7d': '7 days', '30d': '30 days' };
  const colors = ['var(--accent)', 'var(--chart-good)', 'var(--info)', 'var(--mint)', 'var(--accent-strong)', 'var(--db-danger, var(--accent-strong))'];

  let range = $state<Range>('1d');
  let statistics = $state<GatewayApiKeyStatistics | null>(null);
  let statsLoading = $state(true);
  let statsError = $state('');
  let generation = 0;
  let controller: AbortController | null = null;
  let copiedId = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  const createdLabel = $derived(apiKey ? formatUtcTimestamp(apiKey.created_at, locale) ?? apiKey.created_at : '');
  const lastUsedLabel = $derived(apiKey?.last_used_at ? formatUtcTimestamp(apiKey.last_used_at, locale) ?? apiKey.last_used_at : tr('Never used'));
  const scopeLabel = $derived(apiKey?.scope_restricted ? tr('Restricted models') : tr('Any model'));
  const failureSlice = $derived(statistics ? statistics.failures : 0);
  const outcomeSlices = $derived([
    { id: 'success', label: tr('Successful'), value: statistics?.successes ?? 0, color: 'var(--chart-good)' },
    { id: 'failure', label: tr('Failed'), value: failureSlice, color: 'var(--db-danger, var(--accent-strong))' },
  ]);
  const tokenSlices = $derived([
    { id: 'input', label: tr('Input tokens'), value: statistics?.input_tokens ?? 0, color: 'var(--accent)' },
    { id: 'output', label: tr('Output tokens'), value: statistics?.output_tokens ?? 0, color: 'var(--chart-good)' },
    { id: 'cached', label: tr('Cached tokens'), value: statistics?.cached_tokens ?? 0, color: 'var(--info)' },
  ]);
  const modelSlices = $derived.by(() => {
    if (!statistics) return [];
    const rows = statistics.models.top.map((model, index) => ({
      id: model.id,
      label: model.label,
      value: model.requests,
      color: colors[index % colors.length],
    }));
    if (statistics.models.others > 0) rows.push({ id: 'others', label: tr('Others'), value: statistics.models.others, color: 'var(--muted)' });
    return rows;
  });
  const successRate = $derived(statistics ? formatPercent(statistics.success_rate, locale) : '—');
  const totalTokens = $derived(statistics ? statistics.input_tokens + statistics.output_tokens + statistics.cached_tokens : 0);
  const activityCells = $derived.by(() => {
    if (!statistics) return [];
    const points = statistics.series.points;
    const max = Math.max(1, ...points.map((point) => point.requests));
    return points.map((point) => {
      const date = new Date(point.start.replace(' ', 'T') + 'Z');
      return {
        id: point.start,
        requests: point.requests,
        level: point.requests <= 0 ? 0 : Math.max(1, Math.ceil((point.requests / max) * 4)),
        label: Number.isNaN(date.valueOf())
          ? point.start
          : new Intl.DateTimeFormat(locale, { month: 'short', day: 'numeric', hour: '2-digit' }).format(date),
      };
    });
  });
  const modelRanking = $derived.by(() => {
    if (!statistics) return [];
    const rows = [...statistics.models.top].sort((a, b) => b.requests - a.requests);
    if (statistics.models.others > 0) rows.push({ id: 'others', label: tr('Others'), requests: statistics.models.others, percentage: 0 });
    const max = Math.max(1, ...rows.map((row) => row.requests));
    return rows.slice(0, 8).map((row) => ({ ...row, share: Math.round((row.requests / max) * 100) }));
  });
  const providerRanking = $derived.by(() => {
    if (!statistics) return [];
    const rows = [...statistics.providers.rows].sort((a, b) => b.requests - a.requests);
    const max = Math.max(1, ...rows.map((row) => row.requests));
    return rows.slice(0, 8).map((row) => ({
      id: row.provider ?? 'none',
      label: row.provider ?? tr('No provider'),
      requests: row.requests,
      share: Math.round((row.requests / max) * 100),
    }));
  });
  let rankingMode = $state<'model' | 'provider'>('model');
  const rankingRows = $derived(rankingMode === 'model' ? modelRanking : providerRanking);

  async function load(next: Range): Promise<void> {
    const requestGeneration = ++generation;
    controller?.abort();
    const activeController = new AbortController();
    controller = activeController;
    range = next;
    statsLoading = true;
    statsError = '';
    try {
      const result = await api.apiKeyStatistics(keyId, next, activeController.signal);
      if (requestGeneration !== generation || activeController.signal.aborted) return;
      statistics = result;
    } catch (cause) {
      if (requestGeneration !== generation || activeController.signal.aborted) return;
      statsError = localizedError(cause, 'Could not load this API key’s usage.', tr);
    } finally {
      if (requestGeneration === generation) statsLoading = false;
    }
  }

  onMount(() => { void load('1d'); });
  onDestroy(() => {
    generation += 1;
    controller?.abort();
    if (copyTimer) clearTimeout(copyTimer);
  });

  async function copyId(): Promise<void> {
    if (!apiKey) return;
    try {
      await navigator.clipboard.writeText(apiKey.id);
      copiedId = true;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copiedId = false), 1800);
    } catch {
      showDashboardToast(tr('Copy failed'));
    }
  }

  function viewRequests(): void {
    if (!apiKey) return;
    submitTopSearch(apiKey.name);
    onNavigate('requests');
  }
</script>

<section class="key-detail-page" aria-labelledby="api-key-detail-title">
  <div class="page-heading key-detail-heading">
    <div class="heading-copy">
      <h1 id="api-key-detail-title">{tr('Key details')} · <span class="detail-name">{apiKey?.name ?? keyId.slice(0, 12)}</span></h1>
      <p>{tr('API key ID')}: <code class="inline-id">{keyId.slice(0, 12)}…</code></p>
    </div>
    <div class="heading-actions">
      <Button class="action-button" type="button" onclick={onBack}><span class="button-icon"><svg class="icon"><use href="#i-arrow-left" /></svg></span><span class="button-label">{tr('Back to API keys')}</span></Button>
      {#if apiKey}
        <Button class="action-button" type="button" onclick={() => onToggle(apiKey)}><span class="button-icon"><svg class="icon"><use href={apiKey.enabled ? '#i-stop' : '#i-play'} /></svg></span><span class="button-label">{tr(apiKey.enabled ? 'Pause' : 'Resume')}</span></Button>
        <Button class="action-button" type="button" onclick={() => onEdit(apiKey)}><span class="button-icon"><svg class="icon"><use href="#i-edit" /></svg></span><span class="button-label">{tr('Edit scope')}</span></Button>
        <Button class="action-button primary" type="button" onclick={viewRequests}><span class="button-label">{tr('View requests for this key')} →</span></Button>
      {/if}
    </div>
  </div>

  <div class="page-surface key-detail">
    {#if error}
      <div role="alert"><EmptyState icon="requests" title={error} description="" action={tr('Retry')} onclick={onRetry} /></div>
    {:else if loading && !apiKey}
      <p class="usage-note" role="status">{tr('Loading…')}</p>
    {:else if !apiKey}
      <div role="alert"><EmptyState icon="api-keys" title={tr('Unknown key')} description="" /></div>
    {:else}
      {#if apiKey.scope_valid === false}<p class="key-alert" role="alert">{tr('The stored access scope is invalid, so every request with this key is rejected. Save a scope to repair it.')}</p>{/if}

      <section class="summary-card" aria-label={tr('Key details')}>
        <div class="summary-grid">
          <div class="summary-field"><span>{tr('Status')}</span><strong><span class="status-pill" class:paused={!apiKey.enabled}><i></i>{apiKey.enabled ? tr('Active') : tr('Disabled')}</span></strong></div>
          <div class="summary-field"><span>{tr('Requests')}</span><strong class="numeric">{statistics ? formatCount(statistics.total_requests, locale) : '—'}</strong></div>
          <div class="summary-field"><span>{tr('Created')}</span><strong title={apiKey.created_at}>{createdLabel}</strong></div>
          <div class="summary-field"><span>{tr('Last used')}</span><strong title={apiKey.last_used_at ?? ''}>{lastUsedLabel}</strong></div>
        </div>
        <div class="summary-strip">
          <div class="id-cluster"><span class="strip-label">{tr('API key ID')}</span><code title={apiKey.id}>{apiKey.id}</code><Button class="mini-add" type="button" onclick={copyId}>{copiedId ? tr('Copied') : tr('Copy')}</Button></div>
          <div class="scope-cluster"><span class="strip-label">{tr('Access scope')}</span><span class="scope-badge">{scopeLabel}</span><Button class="mini-add" type="button" onclick={() => onEdit(apiKey)}>{tr('Edit')}</Button></div>
        </div>
      </section>

      <section class="telemetry-head" aria-label={tr('Statistics time range')}>
        <div><h2>{tr('API key usage')}</h2><p>{tr('Traffic for {name} in the selected period.', { name: apiKey.name })}</p></div>
        <div class="range-switch" role="group" aria-label={tr('Statistics time range')}>
          {#each RANGES as candidate (candidate)}
            <button type="button" aria-pressed={range === candidate} disabled={statsLoading} onclick={() => void load(candidate)}>{tr(RANGE_KEYS[candidate])}</button>
          {/each}
        </div>
      </section>

      {#if statsLoading && !statistics}
        <p class="usage-note" role="status">{tr('Loading statistics')}…</p>
      {:else if statsError && !statistics}
        <div role="alert"><EmptyState icon="requests" title={statsError} description="" action={tr('Retry')} onclick={() => void load(range)} /></div>
      {:else if statistics}
        <section class="telemetry-strip" aria-label={tr('Traffic summary')}>
          <div><span>{tr('Total requests')}</span><strong class="numeric">{formatCount(statistics.total_requests, locale)}</strong></div>
          <div><span>{tr('Success rate')}</span><strong class="numeric">{successRate}</strong></div>
          <div><span>{tr('Failed requests')}</span><strong class="numeric">{formatCount(statistics.failures, locale)}</strong></div>
          <div><span>{tr('Average latency')}</span><strong class="numeric">{formatCount(Math.round(statistics.average_latency_ms), locale)}ms</strong></div>
          <div><span>{tr('Spend')}</span><strong class="numeric">{formatCostMicroUsd(statistics.cost.cost_reported_micro_usd)}</strong></div>
          <div><span>{tr('Cache ratio')}</span><strong class="numeric">{formatPercent(statistics.cache_ratio, locale)}</strong></div>
        </section>

        <div class="donut-grid">
          <ApiKeyDonutChart title={tr('Request outcomes')} slices={outcomeSlices} {tr} {locale} centerValue={successRate} centerLabel={tr('Success rate')} description={tr('Successful and final failed client requests in this range.')} />
          <ApiKeyDonutChart title={tr('Token mix')} slices={tokenSlices} {tr} {locale} centerValue={formatCompactTokenTotal(totalTokens, locale)} centerLabel={tr('Tokens')} description={tr('Input, output and cached token volume.')} />
          <ApiKeyDonutChart title={tr('Model distribution')} slices={modelSlices} {tr} {locale} centerValue={formatCount(statistics.total_requests, locale)} centerLabel={tr('Requests')} description={tr('Requests grouped by the model this key requested.')} />
        </div>

        <section class="stats-grid" aria-label={tr('Token usage')}>
          <StatsCard index={5} eyebrow={tr('Token usage')} value={formatCompactTokenTotal(totalTokens, locale)} title={tr('Token usage')} trend={tr('Input, output and cached tokens')} />
          <StatsCard index={6} eyebrow={tr('Token usage')} value={formatCompactTokenTotal(statistics.input_tokens, locale)} title={tr('Input tokens')} trend={tr('Prompt tokens sent to models')} />
          <StatsCard index={7} eyebrow={tr('Token usage')} value={formatCompactTokenTotal(statistics.output_tokens, locale)} title={tr('Output tokens')} trend={tr('Generated tokens returned by models')} />
          <StatsCard index={8} eyebrow={tr('Token usage')} value={formatCompactTokenTotal(statistics.cached_tokens, locale)} title={tr('Cached tokens')} trend={`${formatPercent(statistics.cache_ratio, locale)} ${tr('Cache ratio')}`} />
        </section>

        <div class="detail-panels">
          <section class="detail-panel" aria-labelledby="activity-heatmap-title">
            <div class="panel-head">
              <div><h3 id="activity-heatmap-title">{tr('Activity heatmap')}</h3><p>{tr('Request volume across the selected period')}</p></div>
              <span class="panel-total numeric tabular-nums">{formatCount(statistics.total_requests, locale)}</span>
            </div>
            {#if activityCells.length}
              <div class="heatmap" role="img" aria-label={tr('Activity heatmap')}>
                {#each activityCells as cell (cell.id)}
                  <span class="heat-cell level-{cell.level}" title={`${cell.label}: ${formatCount(cell.requests, locale)}`}></span>
                {/each}
              </div>
              <div class="heatmap-legend"><span>{tr('Low')}</span><i class="heat-cell level-0"></i><i class="heat-cell level-1"></i><i class="heat-cell level-2"></i><i class="heat-cell level-3"></i><i class="heat-cell level-4"></i><span>{tr('High')}</span></div>
            {:else}<p class="usage-note">{tr('No traffic for this API key in the selected period.')}</p>{/if}
          </section>

          <section class="detail-panel" aria-labelledby="usage-ranking-title">
            <div class="panel-head">
              <div><h3 id="usage-ranking-title">{tr('Usage ranking')}</h3><p>{tr('Top consumers for this API key')}</p></div>
              <div class="view-switch" role="group" aria-label={tr('Usage ranking')}>
                <button type="button" aria-pressed={rankingMode === 'model'} onclick={() => (rankingMode = 'model')}>{tr('By model')}</button>
                <button type="button" aria-pressed={rankingMode === 'provider'} onclick={() => (rankingMode = 'provider')}>{tr('By provider')}</button>
              </div>
            </div>
            {#if rankingRows.length}
              <ol class="ranking-list">
                {#each rankingRows as row, i (row.id)}
                  <li>
                    <span class="rank-index numeric">{String(i + 1).padStart(2, '0')}</span>
                    <span class="rank-label" title={row.label}>{row.label}</span>
                    <span class="rank-track"><i style={`width:${row.share}%`}></i></span>
                    <strong class="numeric">{formatCount(row.requests, locale)}</strong>
                  </li>
                {/each}
              </ol>
            {:else}<p class="usage-note">{tr('No ranking data for the selected period.')}</p>{/if}
          </section>
        </div>

        {#if !statistics.complete}
          <p class="key-alert" role="status">{tr('{count} telemetry events were dropped. Displayed totals and percentages use the events that were saved.', { count: formatCount(statistics.dropped_events, locale) })}</p>
        {/if}
        {#if statsError}
          <p class="key-alert" role="alert">{statsError}</p>
        {/if}
      {/if}
    {/if}
  </div>
</section>

<style>
  .key-detail-heading h1 { display:flex; flex-wrap:wrap; align-items:center; gap:8px; }
  .detail-name { color:var(--accent); }
  .inline-id { font-family:var(--font-mono); font-size:12px; color:var(--muted); }
  .status-pill { display:inline-flex; align-items:center; gap:5px; padding:3px 9px; border:1px solid var(--chart-good); border-radius:999px; background:color-mix(in srgb, var(--chart-good) 10%, var(--surface)); color:var(--chart-good); font-size:11px; font-weight:600; }
  .status-pill i { width:6px; height:6px; border-radius:50%; background:currentColor; }
  .status-pill.paused { border-color:var(--line); background:var(--surface-sunken); color:var(--muted); }
  .key-detail { display:grid; gap:13px; }
  .key-alert { margin:0; padding:9px 11px; color:var(--accent-strong); border:1px solid var(--line); border-radius:9px; background:var(--surface-sunken); font-size:12px; line-height:1.45; }
  .summary-card { min-width:0; padding:20px; border:1px solid var(--line); border-radius:16px; background:var(--surface); }
  .summary-grid { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:16px; }
  @media (min-width:640px){ .summary-grid { grid-template-columns:repeat(4,minmax(0,1fr)); } }
  .summary-field { display:grid; gap:5px; min-width:0; }
  .summary-field > span { color:var(--muted); font-size:11px; }
  .summary-field strong { overflow:hidden; color:var(--ink); font-size:13px; font-weight:600; text-overflow:ellipsis; white-space:nowrap; }
  .numeric { font-variant-numeric:tabular-nums; }
  .summary-strip { display:flex; flex-direction:column; gap:12px; margin-top:16px; padding-top:16px; border-top:1px solid var(--line); }
  @media (min-width:640px){ .summary-strip { flex-direction:row; align-items:center; justify-content:space-between; } }
  .id-cluster, .scope-cluster { display:flex; flex-wrap:wrap; align-items:center; gap:8px; min-width:0; }
  .strip-label { color:var(--muted); font-size:11px; }
  .id-cluster code { padding:4px 10px; border:1px solid var(--line); border-radius:8px; background:var(--surface-sunken); color:var(--ink); font-family:var(--font-mono); font-size:11px; overflow-wrap:anywhere; }
  .scope-badge { padding:3px 9px; border:1px solid var(--line); border-radius:999px; background:var(--surface-sunken); color:var(--ink); font-size:11px; }
  .telemetry-head { display:flex; flex-wrap:wrap; align-items:center; justify-content:space-between; gap:10px; }
  .telemetry-head h2 { margin:0; color:var(--ink); font:650 14px var(--font-sans); }
  .telemetry-head p { margin:3px 0 0; color:var(--muted); font-size:11px; }
  .range-switch { display:inline-flex; gap:2px; padding:3px; border:1px solid var(--line); border-radius:9px; background:var(--surface-sunken); }
  .range-switch button { border:0; border-radius:6px; padding:5px 9px; background:transparent; color:var(--muted); font:600 12px var(--font-sans); cursor:pointer; }
  .range-switch button[aria-pressed='true'] { color:var(--accent-contrast); background:var(--accent); }
  .range-switch button:disabled { cursor:default; opacity:.6; }
  .usage-note { margin:0; color:var(--muted); font-size:12px; }
  .telemetry-strip { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:8px; }
  @media (min-width:900px){ .telemetry-strip { grid-template-columns:repeat(6,minmax(0,1fr)); } }
  .telemetry-strip > div { display:grid; gap:3px; min-width:0; padding:10px 12px; border:1px solid var(--line); border-radius:11px; background:var(--surface); }
  .telemetry-strip span { color:var(--muted); font-size:10.5px; }
  .telemetry-strip strong { overflow:hidden; color:var(--ink); font-size:13px; text-overflow:ellipsis; white-space:nowrap; }
  .donut-grid { display:grid; grid-template-columns:1fr; gap:12px; }
  @media (min-width:1100px){ .donut-grid { grid-template-columns:repeat(3,minmax(0,1fr)); } }
  .stats-grid { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:12px; }
  @media (min-width:1050px){ .stats-grid { grid-template-columns:repeat(4,minmax(0,1fr)); } }
  .detail-panels { display:grid; grid-template-columns:1fr; gap:12px; }
  @media (min-width:1050px){ .detail-panels { grid-template-columns:minmax(0,1.1fr) minmax(0,1fr); } }
  .detail-panel { display:grid; gap:12px; min-width:0; padding:16px; border:1px solid var(--line); border-radius:14px; background:var(--surface); }
  .panel-head { display:flex; align-items:flex-start; justify-content:space-between; gap:10px; }
  .panel-head h3 { margin:0; color:var(--ink); font:650 13px var(--font-sans); }
  .panel-head p { margin:4px 0 0; color:var(--muted); font-size:11px; }
  .panel-total { color:var(--ink); font-size:18px; font-weight:700; }
  .view-switch { display:flex; gap:4px; padding:3px; border:1px solid var(--line); border-radius:9px; background:var(--surface-sunken); }
  .view-switch button { border:0; border-radius:6px; padding:4px 9px; background:transparent; color:var(--muted); font-size:11px; cursor:pointer; }
  .view-switch button[aria-pressed='true'] { color:var(--accent-contrast); background:var(--accent); }
  .heatmap { display:flex; flex-wrap:wrap; gap:5px; }
  .heat-cell { display:block; width:17px; height:17px; border-radius:4px; background:var(--surface-sunken); border:1px solid var(--line); }
  .heat-cell.level-1 { background:color-mix(in srgb, var(--accent) 22%, var(--surface)); border-color:color-mix(in srgb, var(--accent) 35%, var(--line)); }
  .heat-cell.level-2 { background:color-mix(in srgb, var(--accent) 45%, var(--surface)); border-color:color-mix(in srgb, var(--accent) 55%, var(--line)); }
  .heat-cell.level-3 { background:color-mix(in srgb, var(--accent) 70%, var(--surface)); border-color:var(--accent); }
  .heat-cell.level-4 { background:var(--accent-strong); border-color:var(--accent-strong); }
  .heatmap-legend { display:flex; align-items:center; gap:4px; color:var(--muted); font-size:10px; }
  .heatmap-legend i { width:10px; height:10px; border-radius:2px; }
  .ranking-list { display:grid; gap:8px; margin:0; padding:0; list-style:none; }
  .ranking-list li { display:grid; grid-template-columns:22px minmax(70px,1fr) minmax(60px,1.2fr) auto; align-items:center; gap:8px; color:var(--ink); font-size:11px; }
  .rank-index { color:var(--muted); font-size:10px; }
  .rank-label { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .rank-track { height:6px; border-radius:999px; background:var(--surface-sunken); overflow:hidden; }
  .rank-track i { display:block; height:100%; border-radius:999px; background:var(--accent); }
  .ranking-list strong { font-weight:650; }
</style>
