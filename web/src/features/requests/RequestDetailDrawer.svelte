<script lang="ts">
  import ArkClipboard from '../../components/ArkClipboard.svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import {
    formatCount,
    formatCostMicroUsd,
    formatDate,
    formatTokenCount,
    requestCreatedAt,
    requestDuration,
    type Translate,
  } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import { isLiveRequest, type RequestLiveRow, type RequestLog } from '../../lib/types';
  import { requestOutcome } from './request.metrics';

  export let tr: Translate;
  export let locale: Locale;
  export let request: RequestLog | RequestLiveRow | null = null;
  export let liveClockMs: number = Date.now();
  export let onClose: () => void;

  $: live = request != null && isLiveRequest(request);
  $: status = request?.status ?? null;
  $: statusClass = live
    ? 'live'
    : status == null
      ? ''
      : status >= 200 && status < 300
        ? 'success'
        : status >= 500
          ? 'critical'
          : 'warning';
  $: requestId = request ? (request.request_id ?? request.id ?? '') : '';
  $: cachedTokens = request?.cached_tokens ?? 0;
</script>

<ArkDialog
  open={request != null}
  title={tr('Request details')}
  kicker=""
  closeLabel={tr('Close')}
  class="request-drawer-card"
  {onClose}
>
  {#if request}
    <div class="request-drawer-body">
      <header class="request-drawer-summary">
        <div class="request-drawer-summary-main">
          <span class="request-drawer-model" title={request.model}>{request.model}</span>
          <span class="request-drawer-subtitle">
            {request.route_alias ? request.route_alias : tr('No alias')}
          </span>
        </div>
        <span class="status-badge {statusClass}">
          {live ? tr('In progress') : status ?? '—'}
        </span>
      </header>

      <dl class="request-drawer-fields">
        <div>
          <dt>{tr('Status')}</dt>
          <dd>{live ? tr('In progress') : requestOutcome(request) === 'success' ? tr('Successful') : tr('Failed')}</dd>
        </div>
        <div>
          <dt>{tr('Time')}</dt>
          <dd>{formatDate(requestCreatedAt(request), locale)}</dd>
        </div>
        <div>
          <dt>{tr('Duration')}</dt>
          <dd class="numeric">{requestDuration(request, liveClockMs)}</dd>
        </div>
        <div>
          <dt>{tr('Model')}</dt>
          <dd title={request.model}>{request.model}</dd>
        </div>
        <div>
          <dt>{tr('Provider')}</dt>
          <dd title={request.provider_id ?? ''}>{request.provider_id ?? '—'}</dd>
        </div>
        <div>
          <dt>{tr('Provider credential')}</dt>
          <dd title={request.provider_credential_id ?? ''}>{request.provider_credential_id ?? '—'}</dd>
        </div>
        <div>
          <dt>{tr('API key')}</dt>
          <dd title={request.api_key_id ?? ''}>{request.api_key_name ?? request.api_key_id ?? tr('Unknown key')}</dd>
        </div>
        <div>
          <dt>{tr('Client protocol')}</dt>
          <dd>{request.client_protocol ?? '—'}</dd>
        </div>
        <div>
          <dt>{tr('Upstream protocol')}</dt>
          <dd>{request.upstream_protocol ?? '—'}</dd>
        </div>
        <div>
          <dt>{tr('Input tokens')}</dt>
          <dd class="numeric">{formatTokenCount(request.input_tokens, locale)}</dd>
        </div>
        <div>
          <dt>{tr('Output tokens')}</dt>
          <dd class="numeric">{formatTokenCount(request.output_tokens, locale)}</dd>
        </div>
        <div>
          <dt>{tr('Cached tokens')}</dt>
          <dd class="numeric">{formatCount(cachedTokens, locale)}</dd>
        </div>
        <div>
          <dt>{tr('Cost')}</dt>
          <dd class="numeric">{formatCostMicroUsd(request.cost_micro_usd)}</dd>
        </div>
      </dl>

      {#if request.error && !live}
        <section class="request-drawer-error" aria-label={tr('Error')}>
          <h3>{tr('Error')}</h3>
          <p>{request.error}</p>
        </section>
      {/if}

      <footer class="request-drawer-footer">
        {#if requestId}
          <div class="request-drawer-id">
            <span class="request-drawer-id-label">{tr('Request ID')}</span>
            <code title={requestId}>{requestId}</code>
          </div>
          <ArkClipboard
            value={requestId}
            copyLabel={tr('Copy to clipboard')}
            copiedLabel={tr('Copied')}
            class="request-drawer-clipboard"
          />
        {/if}
        <button type="button" class="secondary-button compact" onclick={onClose}>{tr('Close')}</button>
      </footer>
    </div>
  {/if}
</ArkDialog>

<style>
  /* The drawer reuses the shared dialog (focus trap, escape, portal) and only
     moves it to the right edge through the shared positioner. */
  :global(.modal-positioner:has(> .request-drawer-card)) {
    align-items: stretch !important;
    justify-items: end !important;
    padding: 0 !important;
  }

  :global(.request-drawer-card) {
    display: flex !important;
    flex-direction: column;
    width: min(460px, 100%) !important;
    max-height: 100dvh !important;
    height: 100dvh;
    padding: 18px 20px !important;
    border-radius: 16px 0 0 16px !important;
    animation: request-drawer-in 0.2s cubic-bezier(0.16, 1, 0.3, 1) !important;
  }

  @keyframes request-drawer-in {
    from {
      transform: translateX(24px);
      opacity: 0;
    }
    to {
      transform: translateX(0);
      opacity: 1;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    :global(.request-drawer-card) {
      animation: none !important;
    }
  }

  .request-drawer-body {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    gap: 16px;
    min-height: 0;
    overflow-y: auto;
    margin-top: 4px;
  }

  /* An inset box inside the drawer's own card, framed the way the theme frames
     nested boxes (see `.key-reveal-field code`). */
  .request-drawer-summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 14px 16px;
    border: 2px solid var(--ink);
    border-radius: 10px;
    background: #f8f9fd;
    box-shadow: 2px 2px 0 var(--ink);
  }

  .request-drawer-summary-main {
    display: grid;
    gap: 3px;
    min-width: 0;
  }

  .request-drawer-model {
    overflow: hidden;
    color: #33374b;
    font-family: var(--font-heading);
    font-size: 16px;
    font-weight: 700;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .request-drawer-subtitle {
    overflow: hidden;
    color: #85899b;
    font-size: 12px;
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .request-drawer-fields {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px 12px;
    margin: 0;
  }

  /* Flat inset tiles: they sit inside the framed summary/box, so they must not
     read as cards of their own. */
  .request-drawer-fields > div {
    display: grid;
    gap: 2px;
    min-width: 0;
    padding: 8px 10px;
    border: none;
    border-radius: 8px;
    background: #f8f9fd;
  }

  .request-drawer-fields dt {
    color: #85899b;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.5px;
    text-transform: uppercase;
  }

  .request-drawer-fields dd {
    margin: 0;
    overflow: hidden;
    color: #33374b;
    font-size: 12.5px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .request-drawer-fields dd.numeric {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }

  .request-drawer-error {
    padding: 12px 14px;
    border: 2px solid #b91c1c;
    border-radius: 10px;
    background: #fef2f2;
    box-shadow: 2px 2px 0 #b91c1c;
  }

  .request-drawer-error h3 {
    margin: 0 0 6px;
    color: #dc2626;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.5px;
    text-transform: uppercase;
  }

  .request-drawer-error p {
    margin: 0;
    color: #991b1b;
    font-family: var(--font-mono);
    font-size: 11.5px;
    line-height: var(--leading-normal);
    word-break: break-word;
  }

  .request-drawer-footer {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: auto;
    padding-top: 14px;
    border-top: 1px solid var(--line);
  }

  .request-drawer-id {
    display: grid;
    gap: 3px;
    min-width: 0;
    flex: 1 1 auto;
  }

  .request-drawer-id-label {
    color: #85899b;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.5px;
    text-transform: uppercase;
  }

  /* Flat orange-tint code chip, matching `.api-key-card-heading code`. */
  .request-drawer-id code {
    display: inline-block;
    overflow: hidden;
    color: #ea580c;
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
    background: #fff7ed;
    border: none;
    border-radius: 6px;
    padding: 2px 6px;
  }

  :global(:root[data-theme='dark']) .request-drawer-summary {
    border-color: rgba(255, 255, 255, 0.08);
    background: #2b1d18;
  }

  :global(:root[data-theme='dark']) .request-drawer-model {
    color: #f0edff;
  }

  :global(:root[data-theme='dark']) .request-drawer-subtitle {
    color: #fed7aa;
  }

  :global(:root[data-theme='dark']) .request-drawer-fields > div {
    border-color: rgba(255, 255, 255, 0.05);
    background: rgba(255, 255, 255, 0.03);
  }

  :global(:root[data-theme='dark']) .request-drawer-fields dt {
    color: #8e93aa;
  }

  :global(:root[data-theme='dark']) .request-drawer-fields dd {
    color: #d9dbe9;
  }

  :global(:root[data-theme='dark']) .request-drawer-id code {
    background: #222436;
    border-color: #282a3c;
    color: #d9dbe9;
  }

  :global(:root[data-theme='dark']) .request-drawer-error {
    border-color: #4c1d1d;
    background: #2a1515;
  }

  :global(:root[data-theme='dark']) .request-drawer-error h3,
  :global(:root[data-theme='dark']) .request-drawer-error p {
    color: #fca5a5;
  }

  :global(:root[data-theme='dark']) .request-drawer-footer {
    border-top-color: #282a3c;
  }

  @media (max-width: 650px) {
    /* On phones the shared dialog already turns into a bottom sheet. */
    :global(.request-drawer-card) {
      width: 100% !important;
      height: auto;
      max-height: 88vh !important;
      border-radius: 20px 20px 0 0 !important;
    }
    .request-drawer-fields {
      grid-template-columns: 1fr;
    }
  }
</style>
