<script lang="ts">
  import Button from "../../components/Button.svelte";
  import StatsCard from "../../components/StatsCard.svelte";
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import type { Translate } from '../../lib/format';
  import { formatCount, formatCostMicroUsd, formatCompactTokenTotal } from '../../lib/format';
  import { showDashboardToast, pendingRequestSearch } from '../../lib/dashboard-ui';
  import type { Provider, RequestLiveRow, RequestLog, RequestStatistics } from '../../lib/types';
  import { isLiveRequest } from '../../lib/types';
  import {
    applyRequestLiveEvent,
    createRequestLiveState,
    mergeFinishedRequests,
    requestIdentity,
  } from './request.state';
  import { requestOutcome, requestPageMetrics } from './request.metrics';
  import { requestsCsvFilename, requestsToCsv } from './request.csv';
  import { modelLogoSrc } from './model-logo';
  import { Network } from '@lucide/svelte';
  import SelectOptions from '../../components/SelectOptions.svelte';
  import Input from '../../components/Input.svelte';
  import RequestColumnVisibilityMenu from './RequestColumnVisibilityMenu.svelte';
  import {
    loadHiddenColumns,
    saveHiddenColumns,
    type HideableRequestColumn,
    type HiddenRequestColumns,
  } from './request.visibility';
  import { createStatisticsLiveRefresher } from './request.stats.refresh';
  import '../../styles/dashboard/pages/requests.css';

  interface Props {
    tr: Translate;
    locale: Locale;
    onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;
  }

  let { tr, locale, onConnectionChange }: Props = $props();

  type StatusFilter = 'all' | 'success' | 'failure' | '2xx' | '4xx' | '5xx' | 'live';
  type RangeFilter = '24h' | '7d' | '30d';

  const RANGE_MS: Record<RangeFilter, number> = {
    '24h': 86_400_000,
    '7d': 7 * 86_400_000,
    '30d': 30 * 86_400_000,
  };
  const STATS_RANGE: Record<RangeFilter, RequestStatistics['range']> = {
    '24h': '1d',
    '7d': '7d',
    '30d': '30d',
  };
  const RANGE_LABEL: Record<RangeFilter, string> = {
    '24h': '24 hours',
    '7d': '7 days',
    '30d': '30 days',
  };
  const RANGE_OPTIONS: readonly RangeFilter[] = ['24h', '7d', '30d'];

  let rows = $state<Array<RequestLog | RequestLiveRow>>([]);
  let providers = $state<Provider[]>([]);
  let stats = $state<RequestStatistics | null>(null);
  let statsGeneration = 0;
  let statsAbort: AbortController | null = null;
  let loading = $state(true);
  let loadingMore = $state(false);
  let errorMessage = $state('');
  let generation = 0;
  let nextCursor = $state<string | null>(null);
  let cursorStack = $state<string[]>([]);
  let liveClock = $state(Date.now());
  let liveActive = $state(0);

  let search = $state('');
  let statusFilter = $state<StatusFilter>('all');
  let providerFilter = $state('all');
  let rangeFilter = $state<RangeFilter>('24h');
  let autoRefresh = $state(true);
  let prevStatsRange: RangeFilter = '24h';

  // Display-only column hiding (see request.visibility.ts); never affects what
  // the page fetches, exports, or matches.
  let hiddenColumns = $state(loadHiddenColumns());

  function toggleVisibleColumn(column: HideableRequestColumn): void {
    hiddenColumns = { ...hiddenColumns, [column]: !hiddenColumns[column] };
    saveHiddenColumns(hiddenColumns);
  }

  let detailDialog = $state<HTMLDialogElement>();
  let detailRow = $state<RequestLog | RequestLiveRow | null>(null);

  const providerNameById = $derived(new Map(providers.map((p) => [p.id, p.name])));
  const rangeStart = $derived(Date.now() - RANGE_MS[rangeFilter]);
  // Time, Status, Model, Latency, Tokens and Cost are always rendered; only
  // Provider and Key are hideable columns, so the empty-state colspan follows
  // their visibility.
  const tableColumnCount = $derived(6 + (hiddenColumns.provider ? 0 : 1) + (hiddenColumns.key ? 0 : 1));

  const visibleRows = $derived.by(() => {
    const q = search.trim().toLowerCase();
    return rows.filter((row) => {
      const created = new Date(row.created_at).getTime();
      if (Number.isFinite(created) && created < rangeStart) return false;
      if (statusFilter === 'live' && !isLiveRequest(row)) return false;
      if ((statusFilter === '2xx' || statusFilter === '4xx' || statusFilter === '5xx') && isLiveRequest(row)) return false;
      if (statusFilter === '2xx' && !(row.status != null && row.status >= 200 && row.status < 300)) return false;
      if (statusFilter === '4xx' && !(row.status != null && row.status >= 400 && row.status < 500)) return false;
      if (statusFilter === '5xx' && !(row.status != null && row.status >= 500)) return false;
      if (statusFilter === 'success' && requestOutcome(row) !== 'success') return false;
      if (statusFilter === 'failure' && requestOutcome(row) !== 'failure') return false;
      if (!q) return true;
      const haystack = [row.model, row.route_alias, row.api_key_name, row.provider_id, providerNameById.get(row.provider_id ?? '')]
        .filter(Boolean).join(' ').toLowerCase();
      return haystack.includes(q);
    });
  });

  const metrics = $derived(requestPageMetrics(visibleRows, liveClock));

  function timeLabel(row: RequestLog | RequestLiveRow): string {
    if (isLiveRequest(row)) return tr('now');
    const d = new Date(row.created_at);
    return Number.isNaN(d.getTime()) ? '—' : d.toLocaleTimeString(locale, { hour12: false });
  }

  function durationLabel(row: RequestLog | RequestLiveRow): string {
    if (isLiveRequest(row)) {
      const ms = Date.now() - row.started_at_ms;
      return ms >= 1000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.round(ms)} ms`;
    }
    const ms = row.duration_ms;
    if (ms == null) return '—';
    return ms >= 1000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.round(ms)} ms`;
  }

  function statusClass(row: RequestLog | RequestLiveRow): string {
    if (isLiveRequest(row)) return 'progress';
    const s = row.status ?? 0;
    if (s >= 200 && s < 300) return 'complete';
    if (s >= 400 && s < 500) return 'progress';
    return 'error';
  }

  function statusText(row: RequestLog | RequestLiveRow): string {
    if (isLiveRequest(row)) return tr('LIVE');
    return row.status ? String(row.status) : '—';
  }

  function cacheTokens(row: RequestLog | RequestLiveRow): number | null {
    const cache = row.cached_tokens ?? row.cache_input_tokens ?? null;
    return cache != null && cache > 0 ? cache : null;
  }

  function providerLabel(row: RequestLog | RequestLiveRow): string {
    return (row.provider_id && providerNameById.get(row.provider_id)) || row.provider_id || '—';
  }

  // History timestamps arrive as the `YYYY-MM-DD HH:MM:SS` UTC shape the log
  // store is keyed by; render them in the viewer's timezone.
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

  async function loadStatistics(range: RangeFilter = rangeFilter): Promise<void> {
    const requestGeneration = ++statsGeneration;
    statsAbort?.abort();
    const controller = new AbortController();
    statsAbort = controller;
    try {
      const result = await api.statistics(STATS_RANGE[range], controller.signal);
      if (requestGeneration === statsGeneration && range === rangeFilter) stats = result;
    } catch {
      // Keep the last good snapshot when a background/live refresh fails.
    } finally {
      if (statsAbort === controller) statsAbort = null;
    }
  }

  const statisticsRefresher = createStatisticsLiveRefresher({
    minIntervalMs: 3000,
    refresh: () => { void loadStatistics(rangeFilter); },
  });

  async function loadFirstPage(): Promise<void> {
    const gen = ++generation;
    loading = true;
    errorMessage = '';
    onConnectionChange('loading');
    try {
      const [page, providerRows] = await Promise.all([
        api.requests(providerFilter !== 'all' ? { provider_id: providerFilter } : {}),
        api.providers().catch(() => [] as Provider[]),
      ]);
      if (gen !== generation) return;
      providers = providerRows;
      void loadStatistics(rangeFilter);
      const liveRows = Array.from(liveState.requests.values());
      rows = mergeFinishedRequests(page.requests, null, liveState.pendingFinished, {}).requests;
      rows = [...liveRows, ...rows].slice(0, 50);
      nextCursor = page.next_cursor ?? null;
      cursorStack = [];
      onConnectionChange('loaded');
    } catch (error) {
      if (gen !== generation) return;
      errorMessage = error instanceof Error ? error.message : tr('Could not load requests.');
      onConnectionChange('error');
    } finally {
      if (gen === generation) loading = false;
    }
  }

  async function loadPage(cursor: string | null, pushStack: boolean): Promise<void> {
    if (loadingMore) return;
    loadingMore = true;
    try {
      const page = await api.requests({
        ...(providerFilter !== 'all' ? { provider_id: providerFilter } : {}),
        ...(cursor ? { cursor } : {}),
      });
      const liveRows = Array.from(liveState.requests.values());
      rows = mergeFinishedRequests(page.requests, cursor, liveState.pendingFinished, {}).requests;
      rows = [...liveRows, ...rows].slice(0, 50);
      if (pushStack && cursor) cursorStack = [...cursorStack, cursor];
      nextCursor = page.next_cursor ?? null;
    } catch (error) {
      showDashboardToast(error instanceof Error ? error.message : tr('Could not load page'));
    } finally {
      loadingMore = false;
    }
  }

  function nextPage() {
    if (!nextCursor) return;
    void loadPage(nextCursor, true);
  }

  function prevPage() {
    if (!cursorStack.length) return;
    const stack = [...cursorStack];
    stack.pop();
    const cursor = stack.length ? stack[stack.length - 1] : null;
    cursorStack = stack.slice(0, -1);
    void loadPage(cursor, false);
  }

  async function exportCsv() {
    try {
      const collected: RequestLog[] = [];
      let cursor: string | null | undefined = null;
      for (let i = 0; i < 20 && collected.length < 1000; i++) {
        const page = await api.requests({
          ...(providerFilter !== 'all' ? { provider_id: providerFilter } : {}),
          ...(cursor ? { cursor } : {}),
        });
        collected.push(...page.requests);
        cursor = page.next_cursor ?? null;
        if (!cursor) break;
      }
      const csv = requestsToCsv(collected.slice(0, 1000));
      const blob = new Blob([csv], { type: 'text/csv' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = requestsCsvFilename();
      a.click();
      URL.revokeObjectURL(url);
      showDashboardToast(tr('Exported {count} requests', { count: Math.min(collected.length, 1000) }));
    } catch (error) {
      showDashboardToast(error instanceof Error ? error.message : tr('Export failed'));
    }
  }

  // Live SSE feed
  const liveState = createRequestLiveState();
  let liveAbort: AbortController | null = null;
  let liveTimer: ReturnType<typeof setInterval> | null = null;
  let unsubSearch: (() => void) | null = null;

  function syncRowsFromLive() {
    const liveRows = Array.from(liveState.requests.values());
    const finishedRows = Array.from(liveState.pendingFinished.values());
    const identities = new Set(rows.map(requestIdentity));
    const merged = [
      ...liveRows,
      ...finishedRows.filter((r) => !identities.has(requestIdentity(r))),
      ...rows.filter((r) => !isLiveRequest(r)),
    ].slice(0, 50);
    rows = merged;
    liveActive = liveState.activeCount;
  }

  function startLive() {
    stopLive();
    liveAbort = new AbortController();
    void api.streamRequestLive(
      (event) => {
        if (event.type === 'finished') statisticsRefresher.schedule();
        const result = applyRequestLiveEvent(liveState, event, {});
        Object.assign(liveState, result.state);
        if (result.finished) {
          liveState.pendingFinished.set(requestIdentity(result.finished), result.finished);
        }
        syncRowsFromLive();
      },
      liveAbort.signal,
    );
    liveTimer = setInterval(() => { liveClock = Date.now(); }, 1000);
  }

  function stopLive() {
    liveAbort?.abort();
    liveAbort = null;
    if (liveTimer) { clearInterval(liveTimer); liveTimer = null; }
  }

  function openDetail(row: RequestLog | RequestLiveRow) {
    detailRow = row;
    detailDialog?.showModal();
  }

  $effect(() => {
    if (autoRefresh) startLive();
    else stopLive();
    return () => stopLive();
  });

  $effect(() => {
    const unsub = pendingRequestSearch.subscribe((q) => {
      if (q) {
        search = q;
        pendingRequestSearch.set(null);
      }
    });
    unsubSearch = unsub;
    return unsub;
  });

  // Refresh the active aggregate range whenever live request completion changes
  // its server-side rollup; the refresher coalesces bursts and handles visibility.
  $effect(() => {
    if (rangeFilter === prevStatsRange) return;
    prevStatsRange = rangeFilter;
    void loadStatistics(rangeFilter);
  });

  onMount(() => { void loadFirstPage(); });
  onDestroy(() => {
    generation += 1;
    statsGeneration += 1;
    statsAbort?.abort();
    statsAbort = null;
    statisticsRefresher.dispose();
    stopLive();
    unsubSearch?.();
  });
</script>

<div class="page-heading">
  <div class="heading-copy">
    <h1>{tr('Requests')}</h1>
    <p>{tr('Inspect gateway traffic and debug failures.')}</p>
  </div>
  <div class="heading-actions">
    <label class="switch switch-inline" title={tr('Stream live requests')}>
      <Input type="checkbox" bind:checked={autoRefresh} />
      <span class="switch-track"><span class="switch-thumb"></span></span>
      <span class="switch-label">{tr('Live')}{liveActive ? ` · ${liveActive}` : ''}</span>
    </label>
    <Button class="action-button" type="button" onclick={exportCsv}>
      <span class="button-icon"><svg class="icon"><use href="#i-download" /></svg></span>
      <span class="button-label">{tr('Export CSV')}</span>
    </Button>
  </div>
</div>

{#if errorMessage}
  <section class="course-card surface" aria-label={tr('Load error')}>
    <div class="card-title-row"><h2>{tr('Could not load requests')}</h2></div>
    <p class="card-subtitle">{errorMessage}</p>
    <div class="modal-actions" style="padding-top:14px">
      <Button class="page-action primary" type="button" onclick={loadFirstPage}>{tr('Retry')}</Button>
    </div>
  </section>
{:else if loading}
  <section class="stats-grid" aria-label={tr('Loading')}>
    {#each [0, 1, 2, 3] as i (i)}<StatsCard skeleton />{/each}
  </section>
{:else}
  <div class="page-surface">
    <section class="stats-grid" aria-label={tr('Request overview')}>
      <StatsCard icon="#i-clipboard" value={stats ? formatCount(stats.total_requests, locale) : '—'} title={tr('Requests')} trend={tr('{range} window', { range: rangeFilter })} actionLabel={tr('Browse table')} onAction={ () => document.getElementById('req-board')?.scrollIntoView({ behavior: 'smooth' }) } />
      <StatsCard icon="#i-trend-up" value={stats ? `${Math.round(stats.success_rate * 10) / 10}%` : '—'} title={tr('Success Rate')} trend={tr('2xx responses')} actionLabel={tr('Filter 2xx')} onAction={ () => (statusFilter = '2xx') } />
      <StatsCard icon="#i-clock" value={stats?.average_latency_ms ? `${Math.round(stats.average_latency_ms)} ms` : '—'} title={tr('Avg Latency')} trend={tr('p50 {value}', { value: stats?.latency.p50_ms ? `${Math.round(stats.latency.p50_ms)} ms` : '—' })} actionLabel={tr('Sort by latency')} onAction={ () => showDashboardToast(tr('Sort by latency is not supported by the log API')) } />
      <StatsCard icon="#i-alert" value={stats ? formatCount(stats.failures, locale) : '—'} title={tr('Failed')} trend={tr('4xx + 5xx responses')} actionLabel={tr('Show failures')} onAction={ () => (statusFilter = 'failure') } />
      <StatsCard icon="#i-play" value={formatCount(liveActive, locale)} title={tr('Active now')} trend={tr('in-progress requests')} actionLabel={tr('View live')} onAction={ () => (statusFilter = 'live') } />
      <StatsCard icon="#i-import" value={stats ? formatCompactTokenTotal(stats.input_tokens, locale) : '—'} title={tr('Input tokens')} trend={tr('{range} window', { range: rangeFilter })} actionLabel={tr('Browse table')} onAction={ () => document.getElementById('req-board')?.scrollIntoView({ behavior: 'smooth' }) } />
      <StatsCard icon="#i-export" value={stats ? formatCompactTokenTotal(stats.output_tokens, locale) : '—'} title={tr('Output tokens')} trend={tr('{range} window', { range: rangeFilter })} actionLabel={tr('Browse table')} onAction={ () => document.getElementById('req-board')?.scrollIntoView({ behavior: 'smooth' }) } />
      <StatsCard icon="#i-database" value={stats ? formatCompactTokenTotal(stats.cached_tokens, locale) : '—'} title={tr('Cached tokens')} trend={`${tr('Cache ratio')} ${stats ? `${Math.round(stats.cache_ratio * 10) / 10}%` : '—'}`} actionLabel={tr('Browse table')} onAction={ () => document.getElementById('req-board')?.scrollIntoView({ behavior: 'smooth' }) } />
    </section>

    <p class="req-collection-note">
      {tr('Data collected since')} <strong>{formatTimestamp(stats?.collection_started_at ?? null)}</strong>
      <span aria-hidden="true"> · </span>
      {tr('Last aggregate update')} <strong>{formatTimestamp(stats?.as_of ?? null)}</strong>
    </p>

    <section class="course-card surface req-board" id="req-board" aria-labelledby="requests-heading">
      <div class="card-title-row">
        <h2 id="requests-heading">{tr('Requests')}</h2>
      </div>
      <p class="card-subtitle">{tr('Showing {count} of {total} requests', { count: visibleRows.length, total: stats ? formatCount(stats.total_requests, locale) : '—' })}{metrics.successRate !== null ? ` · ${tr('page success {percent}%', { percent: Math.round(metrics.successRate) })}` : ''}</p>
      <div class="page-toolbar req-filters">
        <Input class="req-search" type="search" placeholder={tr('Search model, key, provider…')} aria-label={tr('Search requests')} bind:value={search} />
        <SelectOptions
          class="req-select"
          ariaLabel={tr('Filter by status')}
          bind:value={statusFilter}
          items={[{ label: tr('All statuses'), value: 'all' }, { label: tr('Success'), value: 'success' }, { label: tr('Failure'), value: 'failure' }, { label: '2xx', value: '2xx' }, { label: '4xx', value: '4xx' }, { label: '5xx', value: '5xx' }, { label: tr('Live only'), value: 'live' }]}
        />
        <SelectOptions
          class="req-select"
          ariaLabel={tr('Filter by provider')}
          bind:value={providerFilter}
          items={[{ label: tr('All providers'), value: 'all' }, ...providers.map((provider) => ({ label: provider.name, value: provider.id }))]}
          onValueChange={() => { cursorStack = []; void loadPage(null, false); }}
        />
        <div class="req-range" role="group" aria-label={tr('Statistics time range')}>
          {#each RANGE_OPTIONS as range (range)}
            <Button type="button" stateClasses={{ active: rangeFilter === range }} aria-pressed={rangeFilter === range} onclick={() => (rangeFilter = range)}>{tr(RANGE_LABEL[range])}</Button>
          {/each}
        </div>
        <RequestColumnVisibilityMenu tr={tr} hiddenColumns={hiddenColumns} onToggleColumn={toggleVisibleColumn} />
      </div>

      <div class="req-table-wrap">
        <table class="req-table">
          <thead>
            <tr>
              <th>{tr('Time')}</th>
              <th>{tr('Status')}</th>
              <th>{tr('Model')}</th>
              {#if !hiddenColumns.provider}<th>{tr('Provider')}</th>{/if}
              {#if !hiddenColumns.key}<th>{tr('Key')}</th>{/if}
              <th class="num">{tr('Latency')}</th>
              <th class="num">{tr('Tokens')}</th>
              <th class="num">{tr('Cost')}</th>
            </tr>
          </thead>
          <tbody>
            {#each visibleRows as row (isLiveRequest(row) ? row.live_id : requestIdentity(row))}
              {@const modelLogo = modelLogoSrc(row.model)}
              {@const proxyLabel = row.proxy_name?.trim() || row.proxy_id?.trim() || ''}
              <tr class="req-row" class:req-live={isLiveRequest(row)} onclick={() => openDetail(row)}>
                <td class="req-time">{#if isLiveRequest(row)}<span class="live-dot"></span>{/if}{timeLabel(row)}</td>
                <td><span class="status {statusClass(row)}">{statusText(row)}</span></td>
                <td class="req-model">
                  <span class="req-model-cell">
                    {#if modelLogo}<img class="req-model-logo" src={modelLogo} alt="" aria-hidden="true" />{/if}
                  <span class="req-model-label">{row.model || '—'}</span>
                  {#if row.route_alias && row.route_alias !== row.model && !hiddenColumns.combo}
                    <span class="req-combo-chip" title={row.route_alias}>{row.route_alias}</span>
                  {/if}
                  {#if row.reasoning_effort}
                    <span class="req-effort-chip" title={row.reasoning_effort}>{row.reasoning_effort}</span>
                  {/if}
                  {#if proxyLabel}
                    <span class="req-proxy-chip" title={proxyLabel}><Network size={10} aria-hidden="true" />{proxyLabel}</span>
                  {/if}
                  </span>
                </td>
                {#if !hiddenColumns.provider}<td>{providerLabel(row)}</td>{/if}
                {#if !hiddenColumns.key}<td>{row.api_key_name || '—'}</td>{/if}
                <td class="num">{durationLabel(row)}</td>
                <td class="num tok-cell">
                  <span class="req-token-input">{formatCount(row.input_tokens ?? 0, locale)}</span>
                  <span class="req-token-separator">/</span>
                  <span class="req-token-output">{formatCount(row.output_tokens ?? 0, locale)}</span>
                  {#if cacheTokens(row) !== null}
                    <span class="req-token-cache-chip">{tr('Cache: {count}', { count: formatCount(cacheTokens(row) ?? 0, locale) })}</span>
                  {/if}
                </td>
                <td class="num">{formatCostMicroUsd(row.cost_micro_usd) || '—'}</td>
              </tr>
            {:else}
              <tr><td colspan={tableColumnCount}><div class="empty-state">{tr('No requests match the current filters.')}</div></td></tr>
            {/each}
          </tbody>
        </table>
      </div>

      <div class="req-pager">
        <Button class="page-action" type="button" onclick={prevPage} disabled={!cursorStack.length || loadingMore}>← {tr('Prev')}</Button>
        <span class="req-page-info">{loadingMore ? tr('Loading…') : tr('{count} rows', { count: visibleRows.length })}</span>
        <Button class="page-action" type="button" onclick={nextPage} disabled={!nextCursor || loadingMore}>{tr('Next')} →</Button>
      </div>
    </section>
  </div>
{/if}

<!-- Request detail dialog -->
<dialog class="modal modal-wide" bind:this={detailDialog} aria-labelledby="req-dialog-title">
  <div class="modal-card">
    <div class="modal-heading">
      <div>
        <h2 id="req-dialog-title">{tr('Request detail')}</h2>
        <p>{detailRow ? (detailRow.route_alias || detailRow.model || tr('request')) : '—'}</p>
      </div>
      <Button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</Button>
    </div>
    {#if detailRow}
      {@const live = isLiveRequest(detailRow)}
      {@const status = live ? 'LIVE' : String(detailRow.status ?? '—')}
      <div class="req-detail">
        <section class="req-detail-overview" aria-label={tr('Request summary')}>
          <span class="req-detail-status" class:live class:success={!live && (detailRow.status ?? 0) >= 200 && (detailRow.status ?? 0) < 300} class:failure={!live && (detailRow.status ?? 0) >= 400} aria-label={tr('Status')}>{status}</span>
          <div class="req-detail-title">
            <strong>{detailRow.route_alias || detailRow.model || tr('request')}</strong>
            <span>{providerLabel(detailRow)}{detailRow.model && detailRow.route_alias ? ` · ${detailRow.model}` : ''}</span>
          </div>
          <span class="req-detail-latency">{durationLabel(detailRow)}</span>
        </section>

        <section class="req-detail-section" aria-labelledby="req-detail-usage">
          <h3 id="req-detail-usage">{tr('Token usage')}</h3>
          <div class="req-detail-usage">
            <div class="req-usage-metric input"><span>{tr('Tokens in')}</span><strong>{formatCount(detailRow.input_tokens ?? 0, locale)}</strong></div>
            <div class="req-usage-metric output"><span>{tr('Tokens out')}</span><strong>{formatCount(detailRow.output_tokens ?? 0, locale)}</strong></div>
            <div class="req-usage-metric cache"><span>{tr('Cache read')}</span><strong>{formatCount(detailRow.cached_tokens ?? detailRow.cache_input_tokens ?? 0, locale)}</strong></div>
          </div>
        </section>

        <section class="req-detail-section" aria-labelledby="req-detail-info">
          <h3 id="req-detail-info">{tr('Request info')}</h3>
          <dl class="req-detail-grid">
            <div><dt>{tr('Route alias')}</dt><dd>{detailRow.route_alias || '—'}</dd></div>
            <div><dt>{tr('Provider')}</dt><dd>{providerLabel(detailRow)}</dd></div>
            <div><dt>{tr('API key')}</dt><dd>{detailRow.api_key_name || '—'}</dd></div>
            <div><dt>{tr('Cost')}</dt><dd>{formatCostMicroUsd(detailRow.cost_micro_usd) || '—'}</dd></div>
            <div><dt>{tr('Time')}</dt><dd>{timeLabel(detailRow)}{detailRow.created_at ? ` · ${new Date(detailRow.created_at).toLocaleString(locale)}` : ''}</dd></div>
            <div><dt>{tr('Protocols')}</dt><dd>{detailRow.client_protocol || '—'} → {detailRow.upstream_protocol || '—'}</dd></div>
          </dl>
        </section>
        {#if detailRow.error}<div class="req-error-note" role="alert">{detailRow.error}</div>{/if}
        {#if detailRow.request_id}<p class="form-note req-detail-id">{tr('Request ID')}: <code>{detailRow.request_id}</code></p>{/if}
      </div>
    {/if}
    <div class="modal-actions">
      <Button class="page-action" type="button" data-close-modal>{tr('Close')}</Button>
    </div>
  </div>
</dialog>

<style>
  .req-range {
    display: inline-flex;
    flex: 0 0 auto;
    gap: 2px;
    padding: 3px;
    border: 1px solid #e9eef0;
    border-radius: 10px;
    background: #fff;
  }

  .req-range :global(.shared-button) {
    min-height: 28px;
    padding: 0 12px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: #59646e;
    font-size: var(--type-body);
    font-weight: var(--weight-medium);
    cursor: pointer;
    transition: background-color 0.15s ease, color 0.15s ease;
  }

  .req-range :global(.shared-button:hover) {
    background: #f5f8f9;
  }

  .req-range :global(.shared-button.active) {
    background: #293341;
    color: #fff;
  }

  .req-collection-note {
    margin: 0;
    color: var(--muted);
    font-size: var(--type-meta);
  }

  .req-collection-note strong {
    color: var(--ink);
    font-weight: var(--weight-semibold);
    font-variant-numeric: tabular-nums;
  }

  /* Egress-proxy chip, shaped like the effort chip but tinted with the info
     colour so it never reads as the accent combo chip or the green cache chip. */
  .req-proxy-chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-width: 150px;
    overflow: hidden;
    padding: 2px 7px;
    border: 1px solid color-mix(in srgb, var(--info) 30%, var(--surface));
    border-radius: 999px;
    background: color-mix(in srgb, var(--info) 10%, var(--surface));
    color: var(--info);
    font-size: var(--type-caption);
    font-weight: var(--weight-semibold);
    line-height: 1.25;
    white-space: nowrap;
  }

  .req-proxy-chip :global(svg) {
    flex: 0 0 auto;
  }

  :global(:root[data-theme='dark']) .req-proxy-chip {
    border-color: color-mix(in srgb, var(--info) 35%, transparent);
    background: color-mix(in srgb, var(--info) 20%, #1a1b2e);
    color: var(--info-soft);
  }
</style>
