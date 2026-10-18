<script lang="ts">
  import type { Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import type { RequestLiveRow, RequestLog } from '../../lib/types';
  import RequestTableRow from './RequestTableRow.svelte';
  import { requestRowKey } from './request.metrics';

  // The table owns the sticky header, the scroll shell, and the row list. The
  // props it forwards are exactly one row's inputs plus the two row actions, so
  // the row stays dumb and the page keeps the drawer/dialog ownership.
  export let tr: Translate;
  export let locale: Locale;
  export let rows: Array<RequestLog | RequestLiveRow> = [];
  export let liveClockMs: number = Date.now();
  export let slowestDurationMs: number | null = null;
  export let selectedKey: string | null = null;
  export let onSelect: (request: RequestLog | RequestLiveRow) => void = () => {};
  export let onViewError: (request: RequestLog | RequestLiveRow) => void = () => {};
</script>

<div class="table-card request-table-card">
  <table>
    <thead>
      <tr>
        <th>{tr('TIME')}</th>
        <th>{tr('API KEY')}</th>
        <th>{tr('MODEL')}</th>
        <th>{tr('ALIAS / COMBO')}</th>
        <th>{tr('PROVIDER')}</th>
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
          {onSelect}
          {onViewError}
        />
      {/each}
    </tbody>
  </table>
</div>

<style>
  /* Soft Neo-Brutalism frame, like every other card in the light theme:
     2px ink border + a hard offset ink shadow instead of a soft drop shadow. */
  .request-table-card {
    position: relative;
    max-height: calc(100dvh - 275px);
    min-height: 400px;
    overflow-x: auto;
    overflow-y: auto;
    overscroll-behavior: contain;
    border: var(--nb-border, 2px solid var(--ink));
    border-radius: 12px;
    background: #ffffff;
    box-shadow: var(--nb-shadow-md, 3px 3px 0 var(--ink));
  }

  :global(:root[data-theme='dark']) .request-table-card {
    border-color: rgba(255, 255, 255, 0.08);
    background: #181926;
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.35);
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
    height: 42px;
    padding: 0 14px;
    color: #626679;
    background: #f8f9fd;
    border-bottom: 1px solid #e9eaf0;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.6px;
    text-transform: uppercase;
    white-space: nowrap;
    box-shadow: none;
  }

  :global(:root[data-theme='dark']) .request-table-card thead th {
    background: #181926;
    color: #8e93aa;
    border-bottom-color: #282a3c;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.2);
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
