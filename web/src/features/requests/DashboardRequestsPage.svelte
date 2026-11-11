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
    <label class="switch inline-flex items-center gap-2 cursor-pointer" title={tr('Stream live requests')}>
      <Input type="checkbox" bind:checked={autoRefresh} />
      <span class="switch-track"><span class="switch-thumb"></span></span>
      <span class="text-muted text-meta font-medium">{tr('Live')}{liveActive ? ` · ${liveActive}` : ''}</span>
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

    <p class="m-0 text-muted text-meta">
      {tr('Data collected since')} <strong>{formatTimestamp(stats?.collection_started_at ?? null)}</strong>
      <span aria-hidden="true"> · </span>
      {tr('Last aggregate update')} <strong>{formatTimestamp(stats?.as_of ?? null)}</strong>
    </p>

    <section class="course-card surface flex flex-col" id="req-board" aria-labelledby="requests-heading">
      <div class="card-title-row">
        <h2 id="requests-heading">{tr('Requests')}</h2>
      </div>
      <p class="card-subtitle">{tr('Showing {count} of {total} requests', { count: visibleRows.length, total: stats ? formatCount(stats.total_requests, locale) : '—' })}{metrics.successRate !== null ? ` · ${tr('page success {percent}%', { percent: Math.round(metrics.successRate) })}` : ''}</p>
      <div class="page-toolbar mt-3 gap-2">
        <Input class="min-h-[34px]! w-[min(300px,100%)]! flex-[1_1_220px] rounded-[10px]! border-line! bg-surface! px-3! text-muted! text-dash-body! focus:outline-none! focus:border-[color-mix(in_srgb,var(--accent)_55%,var(--surface))]! focus:ring-3! focus:ring-db-accent-ring!" type="search" placeholder={tr('Search model, key, provider…')} aria-label={tr('Search requests')} bind:value={search} />
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
        <div class="inline-flex shrink-0 gap-0.5 rounded-[10px] border border-line bg-surface p-[3px]" role="group" aria-label={tr('Statistics time range')}>
          {#each RANGE_OPTIONS as range (range)}
            <button
              type="button"
              aria-pressed={rangeFilter === range}
              onclick={() => (rangeFilter = range)}
              class="min-h-7 rounded-[7px] border-0 bg-transparent px-3 text-muted text-dash-body font-medium cursor-pointer transition-colors duration-150 hover:bg-db-tint-strong focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-db-accent-ring {rangeFilter === range ? 'bg-ink text-canvas' : ''}"
            >{tr(RANGE_LABEL[range])}</button>
          {/each}
        </div>
        <RequestColumnVisibilityMenu tr={tr} hiddenColumns={hiddenColumns} onToggleColumn={toggleVisibleColumn} />
      </div>

      <div class="mt-3.5 overflow-x-auto rounded-[13px] border border-line bg-db-tint">
        <table class="w-full border-collapse min-w-[940px]">
          <thead>
            <tr>
              <th class="border-b border-line bg-db-tint px-3.5 py-[11px] text-left text-muted text-caption font-semibold tracking-[0.04em] whitespace-nowrap">{tr('Time')}</th>
              <th class="border-b border-line bg-db-tint px-3.5 py-[11px] text-left text-muted text-caption font-semibold tracking-[0.04em] whitespace-nowrap">{tr('Status')}</th>
              <th class="border-b border-line bg-db-tint px-3.5 py-[11px] text-left text-muted text-caption font-semibold tracking-[0.04em] whitespace-nowrap">{tr('Model')}</th>
              {#if !hiddenColumns.provider}<th class="border-b border-line bg-db-tint px-3.5 py-[11px] text-left text-muted text-caption font-semibold tracking-[0.04em] whitespace-nowrap">{tr('Provider')}</th>{/if}
              {#if !hiddenColumns.key}<th class="border-b border-line bg-db-tint px-3.5 py-[11px] text-left text-muted text-caption font-semibold tracking-[0.04em] whitespace-nowrap">{tr('Key')}</th>{/if}
              <th class="border-b border-line bg-db-tint px-3.5 py-[11px] text-right text-muted text-caption font-semibold tracking-[0.04em] whitespace-nowrap tabular-nums">{tr('Latency')}</th>
              <th class="border-b border-line bg-db-tint px-3.5 py-[11px] text-right text-muted text-caption font-semibold tracking-[0.04em] whitespace-nowrap tabular-nums">{tr('Tokens')}</th>
              <th class="border-b border-line bg-db-tint px-3.5 py-[11px] text-right text-muted text-caption font-semibold tracking-[0.04em] whitespace-nowrap tabular-nums">{tr('Cost')}</th>
            </tr>
          </thead>
          <tbody>
            {#each visibleRows as row (isLiveRequest(row) ? row.live_id : requestIdentity(row))}
              {@const modelLogo = modelLogoSrc(row.model)}
              {@const proxyLabel = row.proxy_name?.trim() || row.proxy_id?.trim() || ''}
              <tr class:req-live={isLiveRequest(row)} onclick={() => openDetail(row)} class="group/row cursor-pointer transition-colors duration-150 hover:bg-db-tint-strong {isLiveRequest(row) ? 'bg-db-accent-tint hover:bg-db-accent-tint' : ''}">
                <td class="border-b border-db-tint-strong px-3.5 py-2.5 text-muted text-dash-body whitespace-nowrap tabular-nums group-hover/row:text-ink [[tbody_tr:last-child_&]]:border-b-0 {isLiveRequest(row) ? 'text-db-danger! font-semibold' : ''}">{#if isLiveRequest(row)}<span class="me-1.5 inline-block size-2 rounded-full bg-accent align-[1px] animate-req-pulse"></span>{/if}{timeLabel(row)}</td>
                <td class="border-b border-db-tint-strong px-3.5 py-2.5 text-muted text-dash-body whitespace-nowrap [[tbody_tr:last-child_&]]:border-b-0"><span class="status {statusClass(row)}">{statusText(row)}</span></td>
                <td class="border-b border-db-tint-strong px-3.5 py-2.5 text-muted text-dash-body whitespace-nowrap [[tbody_tr:last-child_&]]:border-b-0">
                  <span class="inline-flex min-w-0 items-center gap-2">
                    {#if modelLogo}<img class="block w-5 h-5 shrink-0 object-contain" src={modelLogo} alt="" aria-hidden="true" />{/if}
                  <span class="text-ink font-medium transition-colors duration-150 group-hover/row:text-accent">{row.model || '—'}</span>
                  {#if row.route_alias && row.route_alias !== row.model && !hiddenColumns.combo}
                    <span class="inline-flex items-center max-w-[150px] overflow-hidden px-[7px] py-0.5 rounded-full border border-db-accent-line bg-db-tint-strong text-ink text-caption font-semibold leading-[1.25] whitespace-nowrap" title={row.route_alias}>{row.route_alias}</span>
                  {/if}
                  {#if row.reasoning_effort}
                    <span class="inline-flex items-center px-1.5 py-0.5 rounded-full border border-db-accent-line bg-db-accent-tint text-accent-strong text-caption font-semibold leading-[1.25] uppercase" title={row.reasoning_effort}>{row.reasoning_effort}</span>
                  {/if}
                  {#if proxyLabel}
                    <span class="inline-flex items-center gap-1 max-w-[150px] overflow-hidden px-[7px] py-0.5 rounded-full border border-[color-mix(in_srgb,var(--info)_30%,var(--surface))] bg-[color-mix(in_srgb,var(--info)_10%,var(--surface))] text-[var(--info)] text-caption font-semibold leading-[1.25] whitespace-nowrap [&_svg]:shrink-0 [html[data-theme=dark]_&]:border-[color-mix(in_srgb,var(--info)_35%,transparent)] [html[data-theme=dark]_&]:bg-[color-mix(in_srgb,var(--info)_20%,#1a1b2e)] [html[data-theme=dark]_&]:text-[var(--info-soft)]" title={proxyLabel}><Network size={10} aria-hidden="true" />{proxyLabel}</span>
                  {/if}
                  </span>
                </td>
                {#if !hiddenColumns.provider}<td class="border-b border-db-tint-strong px-3.5 py-2.5 text-muted text-dash-body whitespace-nowrap [[tbody_tr:last-child_&]]:border-b-0">{providerLabel(row)}</td>{/if}
                {#if !hiddenColumns.key}<td class="border-b border-db-tint-strong px-3.5 py-2.5 text-muted text-dash-body whitespace-nowrap [[tbody_tr:last-child_&]]:border-b-0">{row.api_key_name || '—'}</td>{/if}
                <td class="border-b border-db-tint-strong px-3.5 py-2.5 text-right text-muted text-dash-body whitespace-nowrap tabular-nums [[tbody_tr:last-child_&]]:border-b-0">{durationLabel(row)}</td>
                <td class="border-b border-db-tint-strong px-3.5 py-2.5 text-right text-muted text-dash-body whitespace-nowrap tabular-nums [[tbody_tr:last-child_&]]:border-b-0">
                  <span class="text-[#d97706]! font-semibold">{formatCount(row.input_tokens ?? 0, locale)}</span>
                  <span class="mx-1 text-text-dim">/</span>
                  <span class="text-[#9333ea]! font-semibold">{formatCount(row.output_tokens ?? 0, locale)}</span>
                  {#if cacheTokens(row) !== null}
                    <span class="inline-flex ms-[7px] px-[7px] py-0.5 rounded-full bg-chart-good/12! text-chart-good! text-caption font-semibold">{tr('Cache: {count}', { count: formatCount(cacheTokens(row) ?? 0, locale) })}</span>
                  {/if}
                </td>
                <td class="border-b border-db-tint-strong px-3.5 py-2.5 text-right text-muted text-dash-body whitespace-nowrap tabular-nums [[tbody_tr:last-child_&]]:border-b-0">{formatCostMicroUsd(row.cost_micro_usd) || '—'}</td>
              </tr>
            {:else}
              <tr><td colspan={tableColumnCount} class="border-b border-db-tint-strong px-3.5 py-2.5 text-muted text-dash-body [[tbody_tr:last-child_&]]:border-b-0"><div class="empty-state">{tr('No requests match the current filters.')}</div></td></tr>
            {/each}
          </tbody>
        </table>
      </div>

      <div class="mt-3.5 flex items-center justify-center gap-3.5">
        <Button class="page-action disabled:opacity-55" type="button" onclick={prevPage} disabled={!cursorStack.length || loadingMore}>← {tr('Prev')}</Button>
        <span class="text-muted text-meta">{loadingMore ? tr('Loading…') : tr('{count} rows', { count: visibleRows.length })}</span>
        <Button class="page-action disabled:opacity-55" type="button" onclick={nextPage} disabled={!nextCursor || loadingMore}>{tr('Next')} →</Button>
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
      <div class="grid gap-[18px]">
        <section class="flex min-w-0 flex-wrap items-center gap-3 rounded-xl border border-line bg-db-tint px-3.5 py-3 max-nav:flex-wrap" aria-label={tr('Request summary')}>
          <span
            class="shrink-0 rounded-lg px-[9px] py-[5px] text-meta font-bold {live || (!live && (detailRow.status ?? 0) >= 200 && (detailRow.status ?? 0) < 300) ? '' : (!live && (detailRow.status ?? 0) >= 400 ? 'bg-[color-mix(in_srgb,var(--db-danger)_10%,var(--surface))] text-db-danger' : 'bg-db-accent-tint text-accent-strong')} {!live && (detailRow.status ?? 0) >= 200 && (detailRow.status ?? 0) < 300 ? 'bg-[color-mix(in_srgb,var(--chart-good)_12%,var(--surface))] text-chart-good' : ''}"
            aria-label={tr('Status')}
          >{status}</span>
          <div class="grid min-w-0 gap-[3px]">
            <strong class="text-ink text-label [overflow-wrap:anywhere]">{detailRow.route_alias || detailRow.model || tr('request')}</strong>
            <span class="text-muted text-meta">{providerLabel(detailRow)}{detailRow.model && detailRow.route_alias ? ` · ${detailRow.model}` : ''}</span>
          </div>
          <span class="ms-auto text-ink font-mono text-label tabular-nums">{durationLabel(detailRow)}</span>
        </section>

        <section class="grid gap-2" aria-labelledby="req-detail-usage">
          <h3 id="req-detail-usage" class="m-0 text-muted text-caption font-semibold tracking-[0.04em] uppercase">{tr('Token usage')}</h3>
          <div class="grid grid-cols-[repeat(3,minmax(0,1fr))] gap-2 max-nav:grid-cols-1">
            <div class="grid gap-[7px] rounded-[11px] border border-line border-t-2 border-t-[#d97706] bg-surface p-3"><span class="text-muted text-caption">{tr('Tokens in')}</span><strong class="text-[#d97706]! font-mono! text-[17px] tabular-nums">{formatCount(detailRow.input_tokens ?? 0, locale)}</strong></div>
            <div class="grid gap-[7px] rounded-[11px] border border-line border-t-2 border-t-[#9333ea] bg-surface p-3"><span class="text-muted text-caption">{tr('Tokens out')}</span><strong class="text-[#9333ea]! font-mono! text-[17px] tabular-nums">{formatCount(detailRow.output_tokens ?? 0, locale)}</strong></div>
            <div class="grid gap-[7px] rounded-[11px] border border-line border-t-2 border-t-chart-good bg-surface p-3"><span class="text-muted text-caption">{tr('Cache read')}</span><strong class="text-chart-good! font-mono! text-[17px] tabular-nums">{formatCount(detailRow.cached_tokens ?? detailRow.cache_input_tokens ?? 0, locale)}</strong></div>
          </div>
        </section>

        <section class="grid gap-2" aria-labelledby="req-detail-info">
          <h3 id="req-detail-info" class="m-0 text-muted text-caption font-semibold tracking-[0.04em] uppercase">{tr('Request info')}</h3>
          <dl class="m-0 grid grid-cols-2 gap-x-4 gap-y-0 border-t border-line max-nav:grid-cols-1">
            <div class="min-w-0 border-b border-line py-2.5"><dt class="text-muted text-caption">{tr('Route alias')}</dt><dd class="mt-1 mb-0 text-ink text-meta font-medium [overflow-wrap:anywhere]">{detailRow.route_alias || '—'}</dd></div>
            <div class="min-w-0 border-b border-line py-2.5"><dt class="text-muted text-caption">{tr('Provider')}</dt><dd class="mt-1 mb-0 text-ink text-meta font-medium [overflow-wrap:anywhere]">{providerLabel(detailRow)}</dd></div>
            <div class="min-w-0 border-b border-line py-2.5"><dt class="text-muted text-caption">{tr('API key')}</dt><dd class="mt-1 mb-0 text-ink text-meta font-medium [overflow-wrap:anywhere]">{detailRow.api_key_name || '—'}</dd></div>
            <div class="min-w-0 border-b border-line py-2.5"><dt class="text-muted text-caption">{tr('Cost')}</dt><dd class="mt-1 mb-0 text-ink text-meta font-medium [overflow-wrap:anywhere]">{formatCostMicroUsd(detailRow.cost_micro_usd) || '—'}</dd></div>
            <div class="min-w-0 border-b border-line py-2.5"><dt class="text-muted text-caption">{tr('Time')}</dt><dd class="mt-1 mb-0 text-ink text-meta font-medium [overflow-wrap:anywhere]">{timeLabel(detailRow)}{detailRow.created_at ? ` · ${new Date(detailRow.created_at).toLocaleString(locale)}` : ''}</dd></div>
            <div class="min-w-0 border-b border-line py-2.5"><dt class="text-muted text-caption">{tr('Protocols')}</dt><dd class="mt-1 mb-0 text-ink text-meta font-medium [overflow-wrap:anywhere]">{detailRow.client_protocol || '—'} → {detailRow.upstream_protocol || '—'}</dd></div>
          </dl>
        </section>
        {#if detailRow.error}<div class="req-error-note" role="alert">{detailRow.error}</div>{/if}
        {#if detailRow.request_id}<p class="form-note m-0 [overflow-wrap:anywhere]">{tr('Request ID')}: <code class="text-ink text-caption">{detailRow.request_id}</code></p>{/if}
      </div>
    {/if}
    <div class="modal-actions">
      <Button class="page-action" type="button" data-close-modal>{tr('Close')}</Button>
    </div>
  </div>
</dialog>

