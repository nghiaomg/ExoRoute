<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Check, ChevronLeft, ChevronRight, Search } from '@lucide/svelte';
  import ArkField from '../../components/ArkField.svelte';
  import ArkSelect from '../../components/ArkSelect.svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import GatewayError from '../../components/GatewayError.svelte';
  import InlineLoading from '../../components/InlineLoading.svelte';
  import PageHeading from '../../components/PageHeading.svelte';
  import RequestErrorDialog from './RequestErrorDialog.svelte';
  import RequestLogDeleteDialog from './RequestLogDeleteDialog.svelte';
  import RequestsTable from './RequestsTable.svelte';
  import { api } from '../../lib/api';
  import type { Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { RequestLiveConnectionState, RequestLiveRow, RequestLog, RequestLogFilters } from '../../lib/types';
  import {
    createRequestLiveState,
    hasRequestFilters,
    isLiveRequest,
    matchesRequestFilters,
    mergeFinishedRequests,
    requestIdentity,
  } from './request.state';
  import { createRequestLiveController, type RequestLiveController } from './request.live';

  export let tr: Translate;
  export let locale: Locale;
  export let onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;

  let requests: RequestLog[] = [];
  let filters: RequestLogFilters = {
    api_key_id: '',
    model: '',
    provider_id: '',
    status: '',
  };
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
  let activeFilters: RequestLogFilters = { ...filters };
  let requestLiveState = createRequestLiveState();
  let liveConnection: RequestLiveConnectionState = 'connecting';
  let liveClockMs = Date.now();
  let liveController: RequestLiveController;
  let visibleLiveRequests: RequestLiveRow[] = [];
  let displayRequests: Array<RequestLog | RequestLiveRow> = [];
  let liveRequests: Map<string, RequestLiveRow> = requestLiveState.requests;
  let liveTruncated = false;

  $: visibleLiveRequests = currentCursor === null
    ? Array.from(liveRequests.values()).filter((request) => matchesRequestFilters(request, activeFilters)).sort((left, right) => right.started_at_ms - left.started_at_ms)
    : [];
  $: displayRequests = currentCursor === null
    ? [...visibleLiveRequests, ...requests.filter((request) => !request.id || !Array.from(liveRequests.values()).some((live) => live.id === request.id))]
    : requests;

  $: liveRequests = requestLiveState.requests;
  $: liveTruncated = requestLiveState.truncated;

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
      onConnectionChange('loaded');
    } catch (error) {
      if (requestGeneration !== generation || controller.signal.aborted) return;
      errorMessage = localizedError(error, 'Something went wrong while loading this page.', tr);
      onConnectionChange('error');
    } finally {
      if (requestGeneration === generation) loading = false;
    }
  }

  function applyFilters(event: SubmitEvent): void {
    event.preventDefault();
    activeFilters = { ...filters };
    previousCursors = [];
    currentCursor = null;
    void load(null);
  }

  function clearFilters(): void {
    filters = {
      api_key_id: '',
      model: '',
      provider_id: '',
      status: '',
    };
    activeFilters = { ...filters };
    previousCursors = [];
    currentCursor = null;
    void load(null);
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
    onState: (state) => { requestLiveState = state; },
    onFinished: (finished) => {
      if (currentCursor === null) {
        const identity = requestIdentity(finished);
        requests = [finished, ...requests.filter((request) => requestIdentity(request) !== identity)].slice(0, 50);
      }
    },
    onClockChange: (now) => { liveClockMs = now; },
    onConnectionChange: (state) => { liveConnection = state; },
  });

  let selectedErrorRequest: RequestLog | null = null;
  function viewRequestError(request: RequestLog | RequestLiveRow): void {
    if (isLiveRequest(request)) return;
    selectedErrorRequest = request;
  }

  onMount(() => {
    void load(null);
    liveController.start();
  });
  onDestroy(() => {
    generation += 1;
    activeController?.abort();
    liveController.stop();
  });
</script>

<PageHeading title={tr('Requests')} subtitle={tr('Recent gateway traffic, timings, and outcomes.')} {tr}>
  <RequestLogDeleteDialog {tr} {busy} disabled={loading} onConfirm={confirmDeleteAllRequestLogs} />
</PageHeading>

<form class="request-filter-grid" onsubmit={applyFilters}>
  <ArkField
    label={tr('API key ID')}
    bind:value={filters.api_key_id}
    maxlength={256}
    placeholder={tr('Filter by API key ID')}
    disabled={loading}
  />
  <ArkField
    label={tr('Requested model')}
    bind:value={filters.model}
    maxlength={256}
    placeholder={tr('Exact model name')}
    disabled={loading}
  />
  <ArkField
    label={tr('Provider ID')}
    bind:value={filters.provider_id}
    maxlength={256}
    placeholder={tr('Exact provider ID')}
    disabled={loading}
  />
  <ArkSelect
    label={tr('Outcome')}
    bind:value={filters.status}
    disabled={loading}
    items={[
      { label: tr('All outcomes'), value: '' },
      { label: tr('Successful'), value: 'success' },
      { label: tr('Failed'), value: 'failure' },
    ]}
  />
  <div class="request-filter-actions">
    <button class="primary-button compact" type="submit" disabled={loading}><Search size={14} />{tr('Apply filters')}</button>
    <button class="secondary-button compact" type="button" disabled={loading} onclick={clearFilters}>{tr('Clear')}</button>
  </div>
</form>

{#if deleteError}<div class="request-log-feedback database-error" role="alert">{deleteError}</div>{:else if deleteSuccess}<div class="request-log-feedback database-success" role="status"><Check size={13} />{deleteSuccess}</div>{/if}
{#if liveConnection === 'reconnecting'}<div class="request-live-feedback" role="status">{tr('Reconnecting live updates…')}</div>{:else if liveConnection === 'unavailable'}<div class="request-live-feedback" role="status">{tr('Live updates unavailable.')}</div>{/if}
{#if liveTruncated}<div class="request-live-feedback" role="status">{tr('Some active requests are not shown because the live view is bounded.')}</div>{/if}
{#if errorMessage}<GatewayError message={errorMessage} {tr} onRetry={() => load()} />
{:else if loading}<InlineLoading label={'Loading {page}…'} {tr} vars={{ page: tr('Requests').toLowerCase() }} />
{:else if displayRequests.length}
  <RequestsTable {tr} {locale} rows={displayRequests} {liveClockMs} onViewError={viewRequestError} />
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

<style>
  .request-live-feedback {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 0 0 12px;
    padding: 9px 12px;
    color: #9a3412;
    border: 1px solid #fed7aa;
    border-radius: 10px;
    background: #fff7ed;
    font-size: 12px;
  }

  :global(:root[data-theme='dark']) .request-live-feedback {
    color: #fed7aa;
    border-color: #7c2d12;
    background: #2b1d18;
  }

  .request-filter-grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(140px, 1fr)) auto;
    align-items: end;
    gap: 12px;
    margin: 0 0 18px;
    padding: 16px;
    border: none;
    border-radius: 14px;
    background: #ffffff;
    box-shadow: none;
  }

  :global(:root[data-theme='dark']) .request-filter-grid {
    background: #181926 !important;
    border: none !important;
  }

  .request-filter-actions, .request-pager-controls {
    display: flex;
    gap: 8px;
  }

  .request-filter-actions button {
    height: 38px;
  }

  .request-pager {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 14px 4px;
    color: var(--text-secondary);
    font-size: 12px;
  }

  /* Tablet (768px – 1050px) */
  @media (max-width: 1050px) and (min-width: 651px) {
    .request-filter-grid {
      grid-template-columns: repeat(2, minmax(140px, 1fr));
      gap: 12px;
    }
    .request-filter-actions {
      grid-column: 1 / -1;
      display: flex;
      justify-content: flex-start;
      margin-top: 4px;
    }
  }

  /* ─── Mobile App Style (<= 650px: 320px - 430px) ─── */
  @media (max-width: 650px) {
    /* Mobile Filter Controls */
    .request-filter-grid {
      grid-template-columns: 1fr;
      gap: 10px;
      padding: 14px;
      border-radius: 16px;
      margin-bottom: 14px;
    }
    .request-filter-actions {
      display: flex;
      gap: 8px;
    }
    .request-filter-actions button {
      flex: 1;
      height: 42px;
      justify-content: center;
    }

    /* Mobile Pager Controls */
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
    .request-filter-grid {
      padding: 12px 10px;
      gap: 8px;
    }
    .request-pager-controls button {
      padding: 0 10px;
      font-size: 12px;
    }
    .request-pager-info {
      font-size: 12px;
    }
  }
</style>
