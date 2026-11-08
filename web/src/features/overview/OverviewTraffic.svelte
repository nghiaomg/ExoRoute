<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Activity, ArrowRight } from '@lucide/svelte';
  import { api } from '../../lib/api';
  import type { DashboardPage } from '../../lib/navigation';
  import type { Locale } from '../../lib/i18n';
  import { formatDate, requestCreatedAt, requestDuration, type Translate } from '../../lib/format';
  import type { RequestLiveRow, RequestLog, RequestLogFilters } from '../../lib/types';
  import { createRequestLiveController, type RequestLiveController } from '../requests/request.live';
  import {
    createRequestLiveState,
    isLiveRequest,
    requestIdentity,
    type RequestLiveState,
  } from '../requests/request.state';

  export let requests: RequestLog[] = [];
  export let tr: Translate;
  export let locale: Locale;
  export let onNavigate: (page: DashboardPage) => void;
  export let onRequestFinished: (() => void) | undefined = undefined;
  export let onLiveCountChange: ((count: number) => void) | undefined = undefined;

  const activeFilters: RequestLogFilters = {
    api_key_id: '',
    model: '',
    provider_id: '',
    status: '',
  };

  let localRequests: RequestLog[] = [];
  $: if (requests) {
    localRequests = requests;
  }

  let requestLiveState: RequestLiveState = createRequestLiveState();
  let liveClockMs = Date.now();
  let liveController: RequestLiveController;
  let liveRequests: Map<string, RequestLiveRow> = requestLiveState.requests;
  let visibleLiveRequests: RequestLiveRow[] = [];
  let displayRequests: Array<RequestLog | RequestLiveRow> = [];

  $: liveRequests = requestLiveState.requests;
  $: visibleLiveRequests = Array.from(liveRequests.values()).sort((left, right) => right.started_at_ms - left.started_at_ms);
  $: displayRequests = [
    ...visibleLiveRequests,
    ...localRequests.filter((request) => !request.id || !Array.from(liveRequests.values()).some((live) => live.id === request.id)),
  ];

  liveController = createRequestLiveController({
    getState: () => requestLiveState,
    getFilters: () => activeFilters,
    onState: (state) => {
      requestLiveState = state;
      onLiveCountChange?.(state.activeCount);
    },
    onFinished: (finished) => {
      const identity = requestIdentity(finished);
      localRequests = [finished, ...localRequests.filter((req) => requestIdentity(req) !== identity)].slice(0, 50);
      onRequestFinished?.();
    },
    onClockChange: (now) => { liveClockMs = now; },
  });

  onMount(() => {
    liveController.start();
  });

  onDestroy(() => {
    liveController.stop();
    onLiveCountChange?.(0);
  });
</script>

<section class="overview-main-col">
  <div class="section-card">
    <div class="section-card-header">
      <div class="header-with-icon">
        <span class="panel-icon mint-panel"><Activity size={18} /></span>
        <div><h2 class="section-title">{tr('Recent requests')}</h2><p class="section-caption">{tr('The latest traffic passing through your gateway')}</p></div>
      </div>
      <button class="text-button" onclick={() => onNavigate('requests')}>{tr('View all')} <ArrowRight size={14} /></button>
    </div>

    {#if displayRequests.length}
      <div class="table-wrap request-table-card">
        <table>
          <thead>
            <tr>
              <th class="col-model">{tr('MODEL')}</th>
              <th class="col-key">{tr('API KEY')}</th>
              <th class="col-duration">{tr('DURATION')}</th>
              <th class="col-status">{tr('STATUS')}</th>
              <th class="col-time">{tr('TIME')}</th>
            </tr>
          </thead>
          <tbody>
            {#each displayRequests.slice(0, 8) as request (isLiveRequest(request) ? request.live_id : request.id ?? request.request_id ?? request.created_at)}
              <tr class="request-row" class:live-request={isLiveRequest(request)}>
                <td class="req-model">
                  <span class="req-model-title" title={request.model}>{request.model}</span>
                  <div class="req-model-meta">
                    <span class="req-provider-chip">{request.provider_id ?? (isLiveRequest(request) ? tr('Routing…') : '—')}</span>
                    {#if request.route_alias && request.route_alias !== request.model}
                      <span class="req-alias-chip" title="{tr('Alias / Combo')}: {request.route_alias}">· {request.route_alias}</span>
                    {/if}
                  </div>
                </td>
                <td class="req-key" title={request.api_key_name ?? request.api_key_id ?? tr('Unknown key')}>
                  <span class="strong-cell">{request.api_key_name ?? tr('Unknown key')}</span>
                </td>
                <td class="req-duration">{requestDuration(request, liveClockMs)}</td>
                <td class="req-status">
                  <span
                    class="status-badge"
                    class:live={isLiveRequest(request)}
                    class:success={!isLiveRequest(request) && request.status != null && request.status >= 200 && request.status < 300}
                    class:failure={!isLiveRequest(request) && request.status != null && (request.status < 200 || request.status >= 300)}
                  >
                    {isLiveRequest(request) ? tr('In progress') : request.status ?? '—'}
                  </span>
                  {#if request.error && !isLiveRequest(request)}
                    <details class="req-error-expand">
                      <summary>{tr('View error')}</summary>
                      {#if request.provider_credential_id}
                        <div class="request-error-credential">{tr('Provider credential')}: <code>{request.provider_credential_id}</code></div>
                      {/if}
                      <pre>{request.error}</pre>
                    </details>
                  {/if}
                </td>
                <td class="req-time muted-cell">{formatDate(requestCreatedAt(request), locale)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}
      <div class="empty-inline">
        <span class="empty-symbol"><Activity size={24} /></span>
        <div class="empty-inline-content">
          <strong>{tr('No requests yet')}</strong>
          <span>{tr('Once an AI client sends traffic through ExoRoute, it will show up here.')}</span>
        </div>
      </div>
    {/if}
  </div>
</section>

<style>
  .request-table-card {
    overflow-x: hidden;
    border-radius: 14px;
  }
  .request-table-card table {
    width: 100%;
    table-layout: fixed;
    border-collapse: collapse;
  }
  .request-table-card th,
  .request-table-card td {
    padding: 10px 12px;
    vertical-align: middle;
    overflow: hidden;
  }
  .col-model { width: 37%; }
  .col-key { width: 17%; }
  .col-duration { width: 13%; }
  .col-status { width: 15%; }
  .col-time { width: 18%; text-align: right; }
  .request-table-card td.req-time { text-align: right; }

  .request-table-card tr.live-request {
    background: color-mix(in srgb, var(--accent) 7%, var(--surface));
  }
  .request-table-card .status-badge.live {
    color: var(--accent-deep);
    background: color-mix(in srgb, var(--accent) 14%, var(--surface));
  }
  .req-model-title {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    font-weight: 600;
    color: var(--ink);
  }
  .req-model-meta {
    display: flex;
    align-items: center;
    gap: 5px;
    margin-top: 2px;
    font-size: 11.5px;
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .req-provider-chip {
    color: var(--muted);
    font-weight: 500;
  }
  .req-alias-chip {
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .req-key span {
    display: inline-block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
    font-size: 12px;
  }
  .request-table-card td.req-duration {
    color: var(--accent-strong);
    font-weight: 600;
    font-family: var(--font-mono);
    font-size: 12px;
    white-space: nowrap;
  }
  :global(:root[data-theme='dark']) .request-table-card td.req-duration {
    color: var(--accent-glow);
  }
  .request-table-card td.req-status {
    vertical-align: middle;
  }
  .req-error-expand {
    margin-top: 4px;
  }
  .req-error-expand summary {
    color: var(--chart-critical);
    cursor: pointer;
    font-size: 11px;
    font-weight: 600;
    user-select: none;
  }
  .req-error-expand summary:hover {
    text-decoration: underline;
  }
  .req-error-expand pre {
    max-height: 160px;
    margin: 6px 0 0;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--chart-critical);
    background: color-mix(in srgb, var(--chart-critical) 8%, var(--surface));
    border: 1px solid color-mix(in srgb, var(--chart-critical) 32%, var(--line));
    border-radius: 6px;
    padding: 6px 8px;
    font: 500 11px/1.4 var(--font-mono);
  }
  .request-error-credential {
    margin-top: 4px;
    color: var(--accent-deep);
    font-size: 11px;
  }
  .request-table-card td.req-time {
    font-size: 12px;
    white-space: nowrap;
  }

  /* ─── Mobile App Style (<= 650px) ─── */
  @media (max-width: 650px) {
    .request-table-card {
      background: transparent !important;
      border: none !important;
      box-shadow: none !important;
      overflow: visible !important;
      padding: 0 !important;
    }
    .request-table-card table {
      min-width: 0 !important;
      width: 100% !important;
      display: flex !important;
      flex-direction: column !important;
      background: transparent !important;
      table-layout: auto !important;
    }
    .request-table-card thead {
      display: none !important;
    }
    .request-table-card tbody {
      display: flex !important;
      flex-direction: column !important;
      gap: 10px !important;
      width: 100% !important;
    }
    .request-table-card tr.request-row {
      display: grid !important;
      grid-template-columns: 1fr auto !important;
      grid-template-rows: auto auto auto !important;
      gap: 6px 10px !important;
      padding: 14px !important;
      background: var(--surface) !important;
      border-radius: 14px !important;
      border: 1px solid var(--line) !important;
      box-shadow: none !important;
      box-sizing: border-box;
      width: 100% !important;
      transition: transform 0.1s ease, background 0.12s ease;
    }
    .request-table-card tr.request-row:active {
      transform: scale(0.99);
      background: var(--surface-sunken) !important;
    }
    .request-table-card td.req-model {
      grid-column: 1 !important;
      grid-row: 1 !important;
      display: block !important;
      padding: 0 !important;
      border: none !important;
      min-width: 0 !important;
    }
    .request-table-card td.req-status {
      grid-column: 2 !important;
      grid-row: 1 !important;
      justify-self: end !important;
      display: flex !important;
      flex-direction: column !important;
      align-items: flex-end !important;
      padding: 0 !important;
      border: none !important;
    }
    .request-table-card td.req-status .status-badge {
      font-size: 12px !important;
      padding: 2px 7px !important;
      border-radius: 6px !important;
      line-height: 1.3;
    }
    .request-table-card td.req-key {
      grid-column: 1 !important;
      grid-row: 2 !important;
      display: flex !important;
      align-items: center !important;
      font-size: 12px !important;
      color: var(--text-dim) !important;
      padding: 0 !important;
      border: none !important;
      min-width: 0 !important;
    }
    .request-table-card td.req-duration {
      grid-column: 2 !important;
      grid-row: 2 !important;
      justify-self: end !important;
      display: flex !important;
      align-items: center !important;
      font-size: 12px !important;
      padding: 0 !important;
      border: none !important;
    }
    .request-table-card td.req-time {
      grid-column: 1 / -1 !important;
      grid-row: 3 !important;
      display: flex !important;
      align-items: center !important;
      justify-content: flex-end !important;
      font-size: 11.5px !important;
      color: var(--text-dim) !important;
      padding: 2px 0 0 !important;
      border: none !important;
      text-align: right !important;
    }
  }

</style>
