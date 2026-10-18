<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Check, ChevronLeft, ChevronRight } from '@lucide/svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import GatewayError from '../../components/GatewayError.svelte';
  import InlineLoading from '../../components/InlineLoading.svelte';
  import PageHeading from '../../components/PageHeading.svelte';
  import RequestDetailDrawer from './RequestDetailDrawer.svelte';
  import RequestErrorDialog from './RequestErrorDialog.svelte';
  import RequestFilterPanel from './RequestFilterPanel.svelte';
  import RequestLiveToggle from './RequestLiveToggle.svelte';
  import RequestLogDeleteDialog from './RequestLogDeleteDialog.svelte';
  import RequestMetricsStrip from './RequestMetricsStrip.svelte';
  import RequestOutcomeBar from './RequestOutcomeBar.svelte';
  import RequestSkeleton from './RequestSkeleton.svelte';
  import RequestsTable from './RequestsTable.svelte';
  import { api } from '../../lib/api';
  import type { Locale } from '../../lib/i18n';
  import { localizedError } from '../../lib/errors';
  import { type Translate } from '../../lib/format';
  import type { RequestLiveConnectionState, RequestLiveRow, RequestLog, RequestLogFilters } from '../../lib/types';
  import {
    clearedRequestFilter,
    requestPageMetrics,
    requestRowKey,
    type RequestFilterKey,
  } from './request.metrics';
  import { createRequestLiveController, type RequestLiveController } from './request.live';
  import {
    createRequestLiveState,
    hasRequestFilters,
    isLiveRequest,
    matchesRequestFilters,
    mergeFinishedRequests,
    requestIdentity,
  } from './request.state';

  export let tr: Translate;
  export let locale: Locale;
  export let onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;

  const EMPTY_FILTERS: RequestLogFilters = { api_key_id: '', model: '', provider_id: '', status: '' };

  let requests: RequestLog[] = [];
  let loading = true;
  let busy = false;
  let errorMessage = '';
  let deleteError = '';
  let deleteSuccess = '';
  let generation = 0;
  let nextCursor: string | null = null;
  let currentCursor: string | null = null;
  let previousCursors: Array<string | null> = [];
  let activeController: AbortController | null = null;
  let hasFilters = false;
  let activeFilters: RequestLogFilters = { ...EMPTY_FILTERS };
  let requestLiveState = createRequestLiveState();
  let liveConnection: RequestLiveConnectionState = 'connecting';
  let liveController: RequestLiveController;
  let visibleLiveRequests: RequestLiveRow[] = [];
  let liveDisplayRequests: Array<RequestLog | RequestLiveRow> = [];
  let displayRequests: Array<RequestLog | RequestLiveRow> = [];
  let liveRequests: Map<string, RequestLiveRow> = requestLiveState.requests;
  let liveTruncated = false;
  let livePaused = false;
  // Pausing freezes the rendered rows without dropping the live feed: the table
  // shows this snapshot until live resumes, so a row cannot move under the
  // reader's cursor.
  let pausedRequests: Array<RequestLog | RequestLiveRow> | null = null;
  let pausedActiveCount: number | null = null;
  let selectedRequest: RequestLog | RequestLiveRow | null = null;
  let selectedErrorRequest: RequestLog | null = null;
  let lastUpdatedMs = 0;
  let nowMs = Date.now();
  let ticker: number | null = null;

  $: liveRequests = requestLiveState.requests;
  $: liveTruncated = requestLiveState.truncated;
  $: activeCount = livePaused && pausedActiveCount != null ? pausedActiveCount : requestLiveState.activeCount;
  $: visibleLiveRequests = currentCursor === null
    ? Array.from(liveRequests.values())
        .filter((request) => matchesRequestFilters(request, activeFilters))
        .sort((left, right) => right.started_at_ms - left.started_at_ms)
    : [];
  $: liveDisplayRequests = currentCursor === null
    ? [
        ...visibleLiveRequests,
        ...requests.filter(
          (request) => !request.id || !Array.from(liveRequests.values()).some((live) => live.id === request.id),
        ),
      ]
    : requests;
  $: displayRequests = livePaused && pausedRequests ? pausedRequests : liveDisplayRequests;
  $: metrics = requestPageMetrics(displayRequests, nowMs);
  $: selectedKey = selectedRequest ? requestRowKey(selectedRequest) : null;

  async function load(cursor: string | null = currentCursor): Promise<void> {
    const requestGeneration = ++generation;
    activeController?.abort();
    const controller = new AbortController();
    activeController = controller;
    loading = true;
    errorMessage = '';
    onConnectionChange('loading');
    try {
      const result = await api.requests({ ...activeFilters, cursor }, controller.signal);
      if (requestGeneration !== generation) return;
      const merged = mergeFinishedRequests(result.requests, cursor, requestLiveState.pendingFinished, activeFilters);
      requests = merged.requests;
      requestLiveState = { ...requestLiveState, pendingFinished: merged.pendingFinished };
      nextCursor = result.next_cursor ?? null;
      currentCursor = cursor;
      hasFilters = hasRequestFilters(activeFilters);
      lastUpdatedMs = Date.now();
      onConnectionChange('loaded');
    } catch (error) {
      if (requestGeneration !== generation || controller.signal.aborted) return;
      errorMessage = localizedError(error, 'Something went wrong while loading this page.', tr);
      onConnectionChange('error');
    } finally {
      if (requestGeneration === generation) loading = false;
    }
  }

  // Applying a filter set always returns to the first page and to live rows:
  // a frozen snapshot would describe a query that no longer matches.
  function applyFilters(next: RequestLogFilters): void {
    activeFilters = { ...next };
    livePaused = false;
    pausedRequests = null;
    pausedActiveCount = null;
    previousCursors = [];
    currentCursor = null;
    void load(null);
  }

  function clearFilters(): void {
    applyFilters({ ...EMPTY_FILTERS });
  }

  function removeFilter(key: RequestFilterKey): void {
    applyFilters(clearedRequestFilter(activeFilters, key));
  }

  function toggleLive(): void {
    if (livePaused) {
      livePaused = false;
      pausedRequests = null;
      pausedActiveCount = null;
      lastUpdatedMs = Date.now();
      return;
    }
    pausedRequests = liveDisplayRequests;
    pausedActiveCount = requestLiveState.activeCount;
    livePaused = true;
  }

  function selectRequest(request: RequestLog | RequestLiveRow): void {
    selectedRequest = request;
  }

  function viewRequestError(request: RequestLog | RequestLiveRow): void {
    if (isLiveRequest(request)) return;
    selectedErrorRequest = request;
  }

  function nextPage(): void {
    if (!nextCursor || loading) return;
    previousCursors = [...previousCursors, currentCursor];
    void load(nextCursor);
  }

  function previousPage(): void {
    if (!previousCursors.length || loading) return;
    const previous = previousCursors[previousCursors.length - 1] ?? null;
    previousCursors = previousCursors.slice(0, -1);
    void load(previous);
  }

  async function confirmDeleteAllRequestLogs(): Promise<boolean> {
    if (busy) return false;
    deleteError = '';
    deleteSuccess = '';
    busy = true;
    try {
      const result = await api.deleteRequestLogs();
      requests = [];
      requestLiveState = { ...requestLiveState, pendingFinished: new Map() };
      nextCursor = null;
      previousCursors = [];
      currentCursor = null;
      livePaused = false;
      pausedRequests = null;
      pausedActiveCount = null;
      deleteSuccess = tr('Deleted {count} request logs and reset traffic history.', { count: result.deleted_count });
      return true;
    } catch (error) {
      deleteError = localizedError(error, 'Could not delete request logs.', tr);
      return false;
    } finally {
      busy = false;
    }
  }

  liveController = createRequestLiveController({
    getState: () => requestLiveState,
    getFilters: () => activeFilters,
    onState: (state) => {
      requestLiveState = state;
      if (!livePaused) lastUpdatedMs = Date.now();
    },
    onFinished: (finished) => {
      if (currentCursor === null) {
        const identity = requestIdentity(finished);
        requests = [finished, ...requests.filter((request) => requestIdentity(request) !== identity)].slice(0, 50);
      }
    },
    onConnectionChange: (state) => { liveConnection = state; },
  });

  onMount(() => {
    void load(null);
    liveController.start();
    ticker = window.setInterval(() => { nowMs = Date.now(); }, 1000);
  });
  onDestroy(() => {
    generation += 1;
    activeController?.abort();
    liveController.stop();
    if (ticker !== null) window.clearInterval(ticker);
    ticker = null;
  });
</script>

<PageHeading title={tr('Requests')} subtitle={tr('Recent gateway traffic, timings, and outcomes.')} {tr}>
  <div class="request-heading-actions">
    <RequestLiveToggle {tr} live={!livePaused} {lastUpdatedMs} {nowMs} onToggle={toggleLive} />
    <RequestLogDeleteDialog {tr} {busy} disabled={loading} onConfirm={confirmDeleteAllRequestLogs} />
  </div>
</PageHeading>

<RequestMetricsStrip {tr} {locale} {metrics} {activeCount} />
<RequestOutcomeBar {tr} {locale} {metrics} />

<RequestFilterPanel
  {tr}
  {activeFilters}
  {loading}
  onApply={applyFilters}
  onClearFilters={clearFilters}
  onRemoveFilter={removeFilter}
/>

{#if !livePaused && liveConnection === 'reconnecting'}
  <div class="request-live-feedback" role="status">{tr('Reconnecting live updates…')}</div>
{:else if !livePaused && liveConnection === 'unavailable'}
  <div class="request-live-feedback" role="status">{tr('Live updates unavailable.')}</div>
{/if}
{#if !livePaused && liveTruncated}
  <div class="request-live-feedback" role="status">{tr('Some active requests are not shown because the live view is bounded.')}</div>
{/if}

{#if deleteError}
  <div class="request-log-feedback database-error" role="alert">{deleteError}</div>
{:else if deleteSuccess}
  <div class="request-log-feedback database-success" role="status"><Check size={13} />{deleteSuccess}</div>
{/if}

{#if errorMessage}
  <GatewayError message={errorMessage} {tr} onRetry={() => load()} />
{:else if loading && !displayRequests.length}
  <RequestSkeleton {tr} />
{:else if displayRequests.length}
  {#if loading}<InlineLoading label={'Loading {page}…'} {tr} vars={{ page: tr('Requests').toLowerCase() }} />{/if}
  <RequestsTable
    {tr}
    {locale}
    rows={displayRequests}
    liveClockMs={nowMs}
    slowestDurationMs={metrics.slowestDurationMs}
    {selectedKey}
    onSelect={selectRequest}
    onViewError={viewRequestError}
  />
{:else if hasFilters}
  <EmptyState icon="search" title={tr('No matching requests')} description={tr('Try a different search, or clear the filter.')} />
{:else}
  <EmptyState icon="requests" title={tr('No requests recorded')} description={tr('Requests are logged here after a client makes its first call to the gateway.')} />
{/if}

<div class="request-pager">
  <span class="request-pager-info">{tr('Page {current}', { current: previousCursors.length + 1 })}</span>
  <div class="request-pager-controls">
    <button class="secondary-button compact" disabled={!previousCursors.length || loading} onclick={previousPage}><ChevronLeft size={14} />{tr('Previous')}</button>
    <button class="secondary-button compact" disabled={!nextCursor || loading} onclick={nextPage}>{tr('Next')}<ChevronRight size={14} /></button>
  </div>
</div>

<RequestErrorDialog {tr} {locale} request={selectedErrorRequest} onClose={() => selectedErrorRequest = null} />
<RequestDetailDrawer {tr} {locale} request={selectedRequest} liveClockMs={nowMs} onClose={() => selectedRequest = null} />

<style>
  .request-heading-actions {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  /* Ink-framed notice, matching the theme's other framed inline feedback. */
  .request-live-feedback {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 16px;
    padding: 10px 14px;
    color: #c2410c;
    border: 2px solid var(--ink);
    border-radius: 12px;
    background: #fffaf5;
    font-size: 12.5px;
    font-weight: 500;
    box-shadow: 2px 2px 0 var(--ink);
  }

  :global(:root[data-theme='dark']) .request-live-feedback {
    color: #fed7aa;
    border-color: #7c2d12;
    background: #2b1d18;
  }

  .request-pager {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 16px 4px;
    color: #85899b;
    font-size: 12.5px;
    font-weight: 600;
  }

  .request-pager-controls {
    display: flex;
    gap: 8px;
  }

  /* ─── Mobile App Style (<= 650px: 320px - 430px) ─── */
  @media (max-width: 650px) {
    .request-heading-actions {
      flex-wrap: wrap;
    }

    .request-pager {
      padding: 14px 2px;
      font-size: 12px;
    }
    .request-pager-controls button {
      height: 38px;
      min-height: 38px;
      padding: 0 14px;
      border-radius: 9px;
      font-size: 12px;
      font-weight: 600;
    }
  }

  /* ─── Ultra-compact Displays (320px - 360px) ─── */
  @media (max-width: 360px) {
    .request-pager-controls button {
      padding: 0 10px;
      font-size: 12px;
    }
    .request-pager-info {
      font-size: 12px;
    }
  }
</style>
