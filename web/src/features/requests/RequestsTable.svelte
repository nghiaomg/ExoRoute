<script lang="ts">
  import type { Locale } from '../../lib/i18n';
  import {
    formatDate,
    formatTokenCount,
    requestCreatedAt,
    requestDuration,
    type Translate,
  } from '../../lib/format';
  import type { RequestLiveRow, RequestLog } from '../../lib/types';
  import { isLiveRequest } from './request.state';

  export let tr: Translate;
  export let locale: Locale;
  export let rows: Array<RequestLog | RequestLiveRow> = [];
  export let liveClockMs: number = Date.now();
  export let onViewError: (request: RequestLog | RequestLiveRow) => void = () => {};

  function tokenUsageLabel(request: RequestLog | RequestLiveRow): string {
    return `${tr('Input tokens')}: ${formatTokenCount(request.input_tokens, locale)} / ${tr('Output tokens')}: ${formatTokenCount(request.output_tokens, locale)}`;
  }
</script>

  <div class="table-card request-table-card"><table><thead><tr><th>{tr('TIME')}</th><th>{tr('API KEY')}</th><th>{tr('MODEL')}</th><th>{tr('ALIAS / COMBO')}</th><th>{tr('PROVIDER')}</th><th>{tr('DURATION')}</th><th>{tr('TOKENS')}</th><th>{tr('STATUS')}</th><th>{tr('ERROR')}</th></tr></thead><tbody>
    {#each rows as request (isLiveRequest(request) ? request.live_id : request.id ?? request.request_id ?? request.created_at)}
      <tr class="request-row" class:live-request={isLiveRequest(request)}>
        <td class="req-time muted-cell">{formatDate(requestCreatedAt(request), locale)}</td>
        <td class="req-key" title={request.api_key_id ?? tr('Unknown key')}><span class="strong-cell">{request.api_key_name ?? (request.api_key_id ? request.api_key_id.slice(0, 12) : tr('Unknown key'))}</span></td>
        <td class="req-model strong-cell" title={request.model}>{request.model}</td>
        <td class="req-alias" title={request.route_alias ?? ''}>{request.route_alias ?? '—'}</td>
        <td class="req-provider" title={request.provider_id ?? ''}>{request.provider_id ?? (isLiveRequest(request) ? tr('Routing…') : '—')}</td>
        <td class="req-duration">{requestDuration(request, liveClockMs)}</td>
        <td class="req-tokens" aria-label={tokenUsageLabel(request)}>
          <div class="req-token-lines">
            <strong class="req-token-value req-token-input">{formatTokenCount(request.input_tokens, locale)}</strong>
            <span class="req-token-separator" aria-hidden="true">/</span>
            <strong class="req-token-value req-token-output">{formatTokenCount(request.output_tokens, locale)}</strong>
          </div>
        </td>
        <td class="req-status"><span class="status-badge" class:live={isLiveRequest(request)} class:success={!isLiveRequest(request) && request.status != null && request.status >= 200 && request.status < 300} class:failure={!isLiveRequest(request) && request.status != null && (request.status < 200 || request.status >= 300)}>{isLiveRequest(request) ? tr('In progress') : request.status ?? '—'}</span></td>
        <td class="req-error" class:has-error={Boolean(request.error)}>
          {#if request.error && !isLiveRequest(request)}
            <button
              type="button"
              class="req-error-trigger-btn"
              title={tr('View error')}
              onclick={() => onViewError(request)}
            >
              <span class="req-error-dot"></span>
              <span>{tr('View error')}</span>
            </button>
          {:else}
            <span class="muted-cell">—</span>
          {/if}
        </td>
      </tr>
    {/each}
  </tbody></table></div>

<style>
  .request-table-card tr.live-request {
    background: #fffaf2;
  }

  .request-table-card .status-badge.live {
    color: #9a3412;
    background: #ffedd5;
  }

  :global(:root[data-theme='dark']) .request-table-card tr.live-request {
    background: #261f1d;
  }

  :global(:root[data-theme='dark']) .request-table-card .status-badge.live {
    color: #fed7aa;
    background: #431f12;
  }

  .request-table-card {
    max-height: calc(100dvh - 275px);
    min-height: 400px;
    overflow-y: auto;
    overflow-x: auto;
    overscroll-behavior: contain;
    border-radius: 14px;
    position: relative;
    background: #ffffff;
  }

  :global(:root[data-theme='dark']) .request-table-card {
    background: #181926;
  }

  .request-table-card table {
    width: 100%;
    min-width: 0;
    border-collapse: separate;
    border-spacing: 0;
    text-align: left;
  }

  .request-table-card thead th {
    position: sticky;
    top: 0;
    z-index: 10;
    height: 40px;
    padding: 0 12px;
    color: #8e92a2;
    background: #fafafd;
    border-bottom: 1px solid #ececf1;
    font-size: var(--text-2xs);
    font-weight: var(--font-bold);
    letter-spacing: 0.6px;
    white-space: nowrap;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.03);
  }

  :global(:root[data-theme='dark']) .request-table-card thead th {
    background: #181926;
    border-bottom-color: #282a3c;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.2);
  }

  .request-table-card td {
    height: 48px;
    padding: 0 12px;
    border-bottom: 1px solid #f1f1f5;
    font-size: var(--text-xs);
    vertical-align: middle;
  }

  :global(:root[data-theme='dark']) .request-table-card td {
    border-bottom-color: #222436;
  }

  .request-table-card td.req-time {
    white-space: nowrap;
    font-size: 11.5px;
  }

  .request-table-card td.req-key {
    max-width: 130px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .request-table-card td.req-model {
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .request-table-card td.req-alias {
    max-width: 130px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .request-table-card td.req-provider {
    max-width: 110px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .request-table-card td.req-duration {
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 12px;
  }

  .request-table-card td.req-tokens {
    min-width: 142px;
    white-space: nowrap;
  }

  .req-token-lines {
    display: flex;
    align-items: baseline;
    gap: 6px;
    line-height: 1.2;
  }

  .req-token-value {
    color: #36394d;
    font-family: var(--font-mono);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }

  .req-token-separator {
    color: #a3a6b5;
    font-size: 11px;
  }

  .req-token-input {
    color: #ea580c;
  }

  .req-token-output {
    color: #7c3aed;
  }

  :global(:root[data-theme='dark']) .req-token-value {
    color: #e7e4f8;
  }

  :global(:root[data-theme='dark']) .req-token-separator {
    color: #8c8fa4;
  }

  :global(:root[data-theme='dark']) .req-token-input {
    color: #fdba74;
  }

  :global(:root[data-theme='dark']) .req-token-output {
    color: #c4b5fd;
  }

  .request-table-card td.req-status {
    white-space: nowrap;
  }

  .request-table-card td.req-error {
    white-space: nowrap;
  }

  .req-error-trigger-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 8px;
    border-radius: 6px;
    background: #fef2f2;
    border: 1px solid #fecaca;
    color: #dc2626;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .req-error-trigger-btn:hover {
    background: #fee2e2;
    border-color: #fca5a5;
    color: #b91c1c;
  }

  .req-error-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #ef4444;
    flex-shrink: 0;
  }

  :global(:root[data-theme='dark']) .req-error-trigger-btn {
    background: #2a1515;
    border-color: #4c1d1d;
    color: #fca5a5;
  }

  :global(:root[data-theme='dark']) .req-error-trigger-btn:hover {
    background: #381a1a;
    border-color: #7f1d1d;
    color: #fecaca;
  }

  /* ─── Mobile App Style (<= 650px: 320px - 430px) ─── */
  @media (max-width: 650px) {
    /* ─── Convert Table into Mobile App Card Feed ─── */
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
      grid-template-rows: auto auto auto auto auto !important;
      gap: 6px 10px !important;
      padding: 14px 14px !important;
      background: #ffffff !important;
      border-radius: 14px !important;
      border: 1px solid #eeeff4 !important;
      box-shadow: none !important;
      box-sizing: border-box;
      width: 100% !important;
      transition: transform 0.1s ease, background 0.12s ease;
    }
    .request-table-card tr.request-row:active {
      transform: scale(0.99);
      background: #fafafc !important;
    }

    /* Row 1: Model & Status Badge */
    .request-table-card td.req-model {
      grid-column: 1 !important;
      grid-row: 1 !important;
      display: flex !important;
      align-items: center !important;
      font-size: 14px !important;
      font-weight: 700 !important;
      color: #2a2d40 !important;
      word-break: break-all !important;
      padding: 0 !important;
      border: none !important;
    }
    .request-table-card td.req-status {
      grid-column: 2 !important;
      grid-row: 1 !important;
      justify-self: end !important;
      display: flex !important;
      align-items: center !important;
      padding: 0 !important;
      border: none !important;
    }
    .request-table-card td.req-status .status-badge {
      font-size: 12px !important;
      padding: 2px 7px !important;
      border-radius: 6px !important;
      line-height: 1.3;
    }

    /* Row 2: Provider / Alias & Duration */
    .request-table-card td.req-provider {
      grid-column: 1 !important;
      grid-row: 2 !important;
      display: flex !important;
      align-items: center !important;
      font-size: 12px !important;
      color: #63677d !important;
      padding: 0 !important;
      border: none !important;
    }
    .request-table-card td.req-alias {
      display: none !important;
    }
    .request-table-card td.req-duration {
      grid-column: 2 !important;
      grid-row: 2 !important;
      justify-self: end !important;
      display: flex !important;
      align-items: center !important;
      font-size: 12px !important;
      color: #ea580c !important;
      font-weight: 600 !important;
      font-family: var(--font-mono) !important;
      padding: 0 !important;
      border: none !important;
    }

    /* Row 4: Input & output token usage */
    .request-table-card td.req-tokens {
      grid-column: 1 / -1 !important;
      grid-row: 4 !important;
      display: block !important;
      min-width: 0 !important;
      padding: 4px 0 0 !important;
      border: none !important;
    }
    .request-table-card td.req-tokens .req-token-lines {
      gap: 6px;
    }
    .request-table-card td.req-tokens .req-token-value {
      font-size: 11px;
    }

    /* Row 3: API Key & Time */
    .request-table-card td.req-key {
      grid-column: 1 !important;
      grid-row: 3 !important;
      display: flex !important;
      align-items: center !important;
      font-size: 12px !important;
      color: #8e92a4 !important;
      padding: 0 !important;
      border: none !important;
    }
    .request-table-card td.req-time {
      grid-column: 2 !important;
      grid-row: 3 !important;
      justify-self: end !important;
      display: flex !important;
      align-items: center !important;
      font-size: 12px !important;
      color: #9fa3b5 !important;
      padding: 0 !important;
      border: none !important;
    }
    .request-table-card td.req-error {
      grid-column: 1 / -1 !important;
      grid-row: 5 !important;
      display: block !important;
      min-width: 0 !important;
      max-width: none !important;
      padding: 4px 0 0 !important;
      border: none !important;
    }
    .request-table-card td.req-error:not(.has-error) {
      display: none !important;
    }
  }

  /* ─── Ultra-compact Displays (320px - 360px) ─── */
  @media (max-width: 360px) {
    .request-table-card tr.request-row {
      padding: 12px 10px !important;
      border-radius: 12px !important;
      gap: 5px 8px !important;
    }
    .request-table-card td.req-model {
      font-size: 13px !important;
    }
  }

  /* Dark mode overrides for mobile card items */
  :global(:root[data-theme='dark']) .request-table-card tr.request-row {
    background: #181926 !important;
    border-color: #202131 !important;
  }

  :global(:root[data-theme='dark']) .request-table-card tr.request-row:active {
    background: #1f2032 !important;
  }

  :global(:root[data-theme='dark']) .request-table-card tr.live-request {
    background: #261f1d !important;
  }

  :global(:root[data-theme='dark']) .request-table-card td.req-model {
    color: #f0edff !important;
  }

  :global(:root[data-theme='dark']) .request-table-card td.req-provider {
    color: #a4a7be !important;
  }

  :global(:root[data-theme='dark']) .request-table-card td.req-duration {
    color: #fdba74 !important;
  }

  :global(:root[data-theme='dark']) .request-table-card td.req-key {
    color: #8c8fa4 !important;
  }

  :global(:root[data-theme='dark']) .request-table-card td.req-time {
    color: #7b7f94 !important;
  }
</style>
