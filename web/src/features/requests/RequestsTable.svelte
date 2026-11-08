<script lang="ts">
  import type { Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import type { RequestLiveRow, RequestLog } from '../../lib/types';
  import { EyeOff } from '@lucide/svelte';
  import RequestTableRow from './RequestTableRow.svelte';
  import { DEFAULT_MASKED_COLUMNS, type MaskedColumnsState } from './request.mask';
  import { requestRowKey } from './request.metrics';

  // The table owns the row list and its framed shell, which grows with its
  // rows: the page column is the only vertical scroller, matching the overview
  // request table and the mobile card feed. The props it forwards are exactly
  // one row's inputs plus the two row actions, so the row stays dumb and the
  // page keeps the drawer/dialog ownership.
  export let tr: Translate;
  export let locale: Locale;
  export let rows: Array<RequestLog | RequestLiveRow> = [];
  export let liveClockMs: number = Date.now();
  export let slowestDurationMs: number | null = null;
  export let selectedKey: string | null = null;
  export let maskedColumns: MaskedColumnsState = DEFAULT_MASKED_COLUMNS;
  export let onSelect: (request: RequestLog | RequestLiveRow) => void = () => {};
  export let onViewError: (request: RequestLog | RequestLiveRow) => void = () => {};
</script>

<div class="table-card request-table-card">
  <table>
    <thead>
      <tr>
        <th>{tr('TIME')}</th>
        <th><span class="req-th-label">{tr('API KEY')}{#if maskedColumns.key}<EyeOff size={11} class="req-th-masked" />{/if}</span></th>
        <th><span class="req-th-label">{tr('MODEL')}{#if maskedColumns.model}<EyeOff size={11} class="req-th-masked" />{/if}</span></th>
        <th><span class="req-th-label">{tr('ALIAS / COMBO')}{#if maskedColumns.alias}<EyeOff size={11} class="req-th-masked" />{/if}</span></th>
        <th><span class="req-th-label">{tr('PROVIDER')}{#if maskedColumns.provider}<EyeOff size={11} class="req-th-masked" />{/if}</span></th>
        <th>{tr('DURATION')}</th>
        <th>{tr('TOKENS')}</th>
        <th>{tr('STATUS')}</th>
        <th>{tr('ERROR')}</th>
      </tr>
    </thead>
    <tbody>
      {#each rows as request (requestRowKey(request))}
        <RequestTableRow
          {tr}
          {locale}
          {request}
          {liveClockMs}
          {slowestDurationMs}
          selected={selectedKey === requestRowKey(request)}
          {maskedColumns}
          {onSelect}
          {onViewError}
        />
      {/each}
    </tbody>
  </table>
</div>

<style>
  /* Soft Neo-Brutalism frame, like every other card in the light theme:
     2px ink border + a hard offset ink shadow instead of a soft drop shadow.
     The card grows with its rows so the page column stays the only vertical
     scroll container. flex-shrink: 0 keeps the column's negative free space
     from squashing it: with overflow-x set the card is still a scroll
     container, so its automatic minimum size is zero. */
  .request-table-card {
    position: relative;
    flex-shrink: 0;
    overflow-x: auto;
    overscroll-behavior: contain;
    border: var(--nb-border);
    border-radius: 12px;
    background: var(--surface);
    box-shadow: var(--nb-shadow-md);
  }

  .request-table-card table {
    width: 100%;
    min-width: 0;
    border-collapse: separate;
    border-spacing: 0;
    text-align: left;
  }

  .req-th-label {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  :global(.req-th-masked) {
    color: var(--info);
  }
  .request-table-card thead th {
    height: 42px;
    padding: 0 14px;
    color: var(--muted);
    background: var(--surface-sunken);
    border-bottom: 1px solid var(--line);
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.6px;
    text-transform: uppercase;
    white-space: nowrap;
    box-shadow: none;
  }

  /* ─── Mobile App Style (<= 650px: 320px - 430px) ─── */
  @media (max-width: 650px) {
    /* The table becomes a card feed; each row component owns its own card. */
    .request-table-card {
      overflow: visible;
      padding: 0;
      border: none;
      background: transparent;
    }
    .request-table-card table {
      display: flex;
      flex-direction: column;
      width: 100%;
      min-width: 0;
      background: transparent;
    }
    .request-table-card thead {
      display: none;
    }
    .request-table-card tbody {
      display: flex;
      flex-direction: column;
      gap: 10px;
      width: 100%;
    }
  }
</style>
