<script lang="ts">
  import { AlertTriangle } from '@lucide/svelte';
  import {
    formatCount,
    formatDate,
    formatTokenCount,
    requestCreatedAt,
    requestDurationMs,
    type Translate,
  } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import { isLiveRequest, type RequestLiveRow, type RequestLog } from '../../lib/types';
  import { modelLogoSrc } from './model-logo';
  import {
    durationBarPercent,
    durationTone,
    modelAccent,
    modelMonogram,
    requestOutcome,
    tokenRatio,
  } from './request.metrics';

  export let tr: Translate;
  export let locale: Locale;
  export let request: RequestLog | RequestLiveRow;
  export let liveClockMs: number;
  export let slowestDurationMs: number | null = null;
  export let selected = false;
  export let onSelect: (request: RequestLog | RequestLiveRow) => void = () => {};
  export let onViewError: (request: RequestLog | RequestLiveRow) => void = () => {};

  // Provider colours come from a fixed palette; the monogram and dot pick the
  // same bucket for one provider so rows stay recognisable.
  const accents = ['#f97316', '#36c59b', '#56a5dc', '#db6683', '#a5a9bb', '#f0a347'];

  $: live = isLiveRequest(request);
  $: status = request.status ?? null;
  $: statusClass = live
    ? 'live'
    : status == null
      ? ''
      : status >= 200 && status < 300
        ? 'success'
        : status >= 500
          ? 'critical'
          : 'warning';
  $: durationMs = requestDurationMs(request, liveClockMs);
  $: tone = durationTone(durationMs);
  $: barPercent = durationBarPercent(durationMs, slowestDurationMs);
  $: durationTitle = tone === 'verySlow' ? tr('Very slow') : tone === 'slow' ? tr('Slow') : tr('Duration');
  $: accent = accents[modelAccent(request.provider_id ?? request.model)] ?? accents[0];
  $: monogram = modelMonogram(request.model);
  $: modelLogo = modelLogoSrc(request.model);
  $: ratio = tokenRatio(request.input_tokens, request.output_tokens);
  $: cachedTokens = request.cached_tokens ?? 0;
  $: tokenUsageLabel = `${tr('Input tokens')}: ${formatTokenCount(request.input_tokens, locale)} / ${tr('Output tokens')}: ${formatTokenCount(request.output_tokens, locale)}`;

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== 'Enter' && event.key !== ' ') return;
    event.preventDefault();
    onSelect(request);
  }

  function viewError(event: MouseEvent): void {
    // The row itself opens the details drawer; the error button must not.
    event.stopPropagation();
    onViewError(request);
  }
</script>

<tr
  class="request-row outcome-{requestOutcome(request)}"
  class:live-request={live}
  class:selected
  tabindex="0"
  onclick={() => onSelect(request)}
  onkeydown={handleKeydown}
>
  <td class="req-time muted-cell">{formatDate(requestCreatedAt(request), locale)}</td>
  <td class="req-key" title={request.api_key_name ?? request.api_key_id ?? tr('Unknown key')}>
    <span class="strong-cell">
      {request.api_key_name ?? tr('Unknown key')}
    </span>
  </td>
  <td class="req-model" title={request.model}>
    {#if modelLogo}
      <span class="req-model-logo" aria-hidden="true">
        <img src={modelLogo} alt="" loading="lazy" referrerpolicy="no-referrer" />
      </span>
    {:else}
      <span class="req-model-monogram" style="--request-accent: {accent}" aria-hidden="true">{monogram}</span>
    {/if}
    <span class="req-model-name strong-cell">{request.model}</span>
  </td>
  <td class="req-alias" title={request.route_alias ?? ''}>
    {#if request.route_alias}
      <span class="req-alias-pill">{request.route_alias}</span>
    {:else}
      <span class="muted-cell">—</span>
    {/if}
  </td>
  <td class="req-provider" title={request.provider_id ?? ''}>
    {#if request.provider_id}
      <span class="req-provider-dot" style="--request-accent: {accent}" aria-hidden="true"></span>
      <span class="req-provider-name">{request.provider_id}</span>
    {:else}
      <span class="muted-cell">{live ? tr('Routing…') : '—'}</span>
    {/if}
  </td>
  <td class="req-duration">
    {#if durationMs == null}
      <span class="muted-cell">—</span>
    {:else}
      <span class="req-duration-value {tone}" title={durationTitle}>{durationMs} ms</span>
      <span class="req-duration-track" aria-hidden="true">
        <span class="req-duration-fill {tone}" style="width: {barPercent}%"></span>
      </span>
    {/if}
  </td>
  <td class="req-tokens" aria-label={tokenUsageLabel}>
    <div class="req-token-lines">
      <strong class="req-token-value req-token-input">{formatTokenCount(request.input_tokens, locale)}</strong>
      <span class="req-token-separator" aria-hidden="true">/</span>
      <strong class="req-token-value req-token-output">{formatTokenCount(request.output_tokens, locale)}</strong>
      {#if cachedTokens > 0}
        <span class="req-token-cached">{tr('Cached')} {formatCount(cachedTokens, locale)}</span>
      {/if}
    </div>
    {#if ratio}
      <span class="req-token-ratio" aria-hidden="true">
        <span class="req-token-ratio-input" style="width: {ratio.input}%"></span>
        <span class="req-token-ratio-output" style="width: {ratio.output}%"></span>
      </span>
    {/if}
  </td>
  <td class="req-status">
    <span class="status-badge {statusClass}">
      {#if live}<span class="req-status-live-dot" aria-hidden="true"></span>{/if}
      {live ? tr('In progress') : status ?? '—'}
    </span>
  </td>
  <td class="req-error">
    {#if request.error && !live}
      <button type="button" class="req-error-trigger-btn" title={tr('View error')} onclick={viewError}>
        <AlertTriangle size={12} />
        <span>{tr('View error')}</span>
      </button>
    {:else}
      <span class="muted-cell">—</span>
    {/if}
  </td>
</tr>

<style>
  /* This component owns one request row end to end: the desktop cells and the
     responsive card feed derived from the same markup. Splitting them would
     duplicate every cell, so the responsive block stays here. */
  .request-row {
    --request-row-accent: #a5a9bb;
    cursor: pointer;
  }

  .request-row.outcome-success {
    --request-row-accent: #36c59b;
  }

  .request-row.outcome-failure {
    --request-row-accent: #db6683;
  }

  .request-row.outcome-active {
    --request-row-accent: #f97316;
  }

  .request-row td {
    position: relative;
    height: 50px;
    padding: 0 14px;
    border-bottom: 1px solid #f1f2f6;
    font-size: var(--text-xs);
    vertical-align: middle;
    color: #484c60;
  }

  .request-row td:first-child::before {
    content: '';
    position: absolute;
    inset: 0 auto 0 0;
    width: 3.5px;
    border-radius: 0 2px 2px 0;
    background: var(--request-row-accent);
    transition: width 0.15s ease, background 0.15s ease;
  }

  @media (hover: hover) {
    .request-row:hover td {
      background: #f8f9fd;
    }
    .request-row:hover td:first-child::before {
      width: 5px;
    }
  }

  .request-row:focus-visible {
    outline: none;
  }

  .request-row:focus-visible td {
    box-shadow: inset 0 2px 0 var(--ink), inset 0 -2px 0 var(--ink);
  }

  .request-row.selected td {
    background: #eff6ff;
  }

  .request-row.selected td:first-child::before {
    width: 5px;
    background: #3b82f6;
  }

  /* An in-flight row uses the same orange as the table shell's live row and as
     the `active` outcome accent, so the two never disagree. */
  .request-row.live-request td {
    background: #fffaf2;
  }

  .request-row.live-request td:first-child::before {
    background: #f97316;
  }

  .request-row td.req-time {
    white-space: nowrap;
    font-size: 11.5px;
    color: #85899b;
    font-variant-numeric: tabular-nums;
  }

  .request-row td.req-key {
    max-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #626679;
    font-weight: 600;
  }

  .request-row td.req-model {
    display: flex;
    align-items: center;
    gap: 9px;
    max-width: 230px;
    overflow: hidden;
  }

  /* Both tiles take the same ink frame as `.provider-avatar` so the row's
     identity mark reads as part of the light theme's Neo-Brutalism, not as a
     soft banner-tile from another design system. */
  .req-model-monogram {
    display: grid;
    place-items: center;
    flex: 0 0 auto;
    width: 24px;
    height: 24px;
    border: 2px solid var(--ink);
    border-radius: 7px;
    background: var(--request-accent);
    color: #ffffff;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.3px;
    box-shadow: 2px 2px 0 var(--ink);
  }

  /* Brand marks are drawn in the vendor's own colours, so they sit on a light
     tile of the monogram's size instead of on the accent colour. */
  .req-model-logo {
    display: grid;
    place-items: center;
    flex: 0 0 auto;
    width: 24px;
    height: 24px;
    border: 2px solid var(--ink);
    border-radius: 7px;
    background: #ffffff;
    box-shadow: 2px 2px 0 var(--ink);
    overflow: hidden;
  }

  .req-model-logo img {
    display: block;
    width: 15px;
    height: 15px;
    object-fit: contain;
  }

  .req-model-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #33374b;
    font-weight: 600;
    font-size: 13px;
  }

  .request-row td.req-alias,
  .request-row td.req-provider {
    max-width: 130px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .request-row td.req-provider {
    display: flex;
    align-items: center;
    gap: 7px;
  }

  .req-provider-dot {
    flex: 0 0 auto;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--request-accent);
    box-shadow: 0 0 0 2px rgba(0, 0, 0, 0.04);
  }

  .req-provider-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #626679;
    font-weight: 500;
  }

  /* Same badge language as `.protocol-pill`: ink stroke on an orange tint. */
  .req-alias-pill {
    display: inline-block;
    max-width: 100%;
    overflow: hidden;
    padding: 2px 8px;
    border: 2px solid var(--ink);
    border-radius: 7px;
    background: #fff7ed;
    color: #92600f;
    font-size: 11px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
    vertical-align: middle;
  }

  .request-row td.req-duration {
    min-width: 96px;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 12px;
  }

  .req-duration-value {
    font-variant-numeric: tabular-nums;
    color: #484c60;
  }

  .req-duration-value.slow {
    color: #d97706;
    font-weight: 700;
  }

  .req-duration-value.verySlow {
    color: #dc2626;
    font-weight: 700;
  }

  .req-duration-track {
    display: block;
    width: 100%;
    max-width: 84px;
    height: 4px;
    margin-top: 4px;
    overflow: hidden;
    border-radius: 999px;
    background: #eceef4;
  }

  /* The fill's healthy colour is the row's own success accent, so the bar and
     the outcome stripe always agree. */
  .req-duration-fill {
    display: block;
    height: 100%;
    border-radius: 999px;
    background: #36c59b;
  }

  .req-duration-fill.slow {
    background: #d97706;
  }

  .req-duration-fill.verySlow {
    background: #dc2626;
  }

  .request-row td.req-tokens {
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
    color: #484c60;
    font-family: var(--font-mono);
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    font-weight: 700;
  }

  .req-token-separator {
    color: #a7a9b6;
    font-size: 11px;
  }

  .req-token-input {
    color: #ea580c;
  }

  .req-token-output {
    color: #7c3aed;
  }

  /* Flat chip, like the other in-row status chips; the ink frame is reserved
     for the row's own card surface. */
  .req-token-cached {
    padding: 1px 6px;
    border: none;
    border-radius: 5px;
    background: #eff9f4;
    color: #32876d;
    font-size: 10px;
    font-weight: 600;
  }

  .req-token-ratio {
    display: flex;
    width: 100%;
    max-width: 132px;
    height: 4px;
    margin-top: 5px;
    overflow: hidden;
    border-radius: 999px;
    background: #eceef4;
  }

  .req-token-ratio-input {
    background: #ea580c;
  }

  .req-token-ratio-output {
    background: #7c3aed;
  }

  .request-row td.req-status,
  .request-row td.req-error {
    white-space: nowrap;
  }

  .req-status-live-dot {
    display: inline-block;
    width: 6px;
    height: 6px;
    margin-right: 6px;
    border-radius: 50%;
    background: currentColor;
    animation: req-status-pulse 1.6s ease-in-out infinite;
  }

  @keyframes req-status-pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.25;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .req-status-live-dot {
      animation: none;
    }
  }

  .req-error-trigger-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 8px;
    border: 2px solid var(--ink);
    border-radius: 7px;
    background: #fef2f2;
    color: #b91c1c;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    box-shadow: 2px 2px 0 var(--ink);
    transition: background 0.15s ease, box-shadow 0.15s ease, transform 0.15s ease;
  }

  .req-error-trigger-btn:hover {
    background: #fee2e2;
    color: #b91c1c;
    transform: translate(-1px, -1px);
    box-shadow: 3px 3px 0 var(--ink);
  }

  :global(:root[data-theme='dark']) .request-row td {
    border-bottom-color: #222436;
  }

  @media (hover: hover) {
    :global(:root[data-theme='dark']) .request-row:hover td {
      background: #1f2032;
    }
  }

  :global(:root[data-theme='dark']) .request-row:focus-visible td {
    box-shadow: inset 0 2px 0 #f0f2f8, inset 0 -2px 0 #f0f2f8;
  }

  :global(:root[data-theme='dark']) .request-row.selected td {
    background: #2b1d18;
  }

  :global(:root[data-theme='dark']) .request-row.live-request td {
    background: #261f1d;
  }

  :global(:root[data-theme='dark']) .req-model-monogram,
  :global(:root[data-theme='dark']) .req-model-logo,
  :global(:root[data-theme='dark']) .req-provider-dot {
    border-color: #f0f2f8;
  }

  :global(:root[data-theme='dark']) .req-alias-pill {
    border-color: #7c2d12;
    background: #2b1d18;
    color: #fed7aa;
  }

  :global(:root[data-theme='dark']) .req-duration-value.slow {
    color: #fdba74;
  }

  :global(:root[data-theme='dark']) .req-duration-value.verySlow {
    color: #fca5a5;
  }

  :global(:root[data-theme='dark']) .req-duration-track,
  :global(:root[data-theme='dark']) .req-token-ratio {
    background: #222436;
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

  :global(:root[data-theme='dark']) .req-token-cached {
    background: #1b2f29;
    color: #34d399;
  }

  :global(:root[data-theme='dark']) .req-error-trigger-btn {
    border-color: #4c1d1d;
    background: #2a1515;
    color: #fca5a5;
  }

  :global(:root[data-theme='dark']) .req-error-trigger-btn:hover {
    border-color: #7f1d1d;
    background: #381a1a;
    color: #fecaca;
  }

  /* ─── Mobile App Style (<= 650px: 320px - 430px) ─── */
  @media (max-width: 650px) {
    .request-row {
      display: grid;
      grid-template-columns: 1fr auto;
      grid-template-areas:
        'model status'
        'provider duration'
        'key time'
        'alias alias'
        'tokens tokens'
        'error error';
      gap: 6px 10px;
      width: 100%;
      padding: 14px;
      border: 2px solid var(--ink);
      border-radius: 14px;
      background: #ffffff;
      box-shadow: 3px 3px 0 var(--ink);
      box-sizing: border-box;
      transition: transform 0.1s ease, background 0.12s ease, box-shadow 0.1s ease;
    }

    .request-row.selected {
      background: #eff6ff;
    }

    .request-row.live-request {
      background: #fffaf2;
    }

    /* The pressed card sinks into its own shadow, like every other NB surface. */
    .request-row:active {
      transform: translate(1px, 1px);
      box-shadow: 1px 1px 0 var(--ink);
      background: #f8f9fd;
    }

    .request-row td {
      height: auto;
      padding: 0;
      border: none;
      font-size: 12px;
    }

    .request-row td:first-child::before {
      display: none;
    }

    /* A left stripe marks the outcome on the card itself. */
    .request-row {
      border-left: 4px solid var(--request-row-accent);
    }

    .request-row td.req-model {
      grid-area: model;
      font-size: 14px;
      font-weight: 700;
      color: #33374b;
      word-break: break-all;
    }

    .req-model-monogram,
    .req-model-logo {
      width: 26px;
      height: 26px;
    }

    .req-model-name {
      white-space: normal;
    }

    .request-row td.req-status {
      grid-area: status;
      display: flex;
      align-items: center;
      justify-self: end;
    }

    .request-row td.req-status .status-badge {
      padding: 2px 7px;
      border-radius: 6px;
      font-size: 12px;
      line-height: 1.3;
    }

    .request-row td.req-provider {
      grid-area: provider;
      color: #626679;
      font-size: 12px;
    }

    .request-row td.req-duration {
      grid-area: duration;
      display: flex;
      flex-direction: column;
      align-items: flex-end;
      justify-self: end;
      min-width: 0;
      color: #ea580c;
      font-weight: 600;
      font-family: var(--font-mono);
    }

    .req-duration-track {
      max-width: 72px;
    }

    .request-row td.req-key {
      grid-area: key;
      max-width: none;
      color: #85899b;
      font-size: 12px;
    }

    .request-row td.req-time {
      grid-area: time;
      justify-self: end;
      color: #a7a9b6;
      font-size: 12px;
    }

    .request-row td.req-alias {
      grid-area: alias;
      max-width: none;
    }

    .request-row td.req-tokens {
      grid-area: tokens;
      min-width: 0;
      padding-top: 4px;
    }

    .req-token-ratio {
      max-width: 100%;
    }

    .request-row td.req-error {
      grid-area: error;
      padding-top: 4px;
    }

    :global(:root[data-theme='dark']) .request-row {
      background: #181926;
      border-color: #202131;
      border-left-color: var(--request-row-accent);
    }

    :global(:root[data-theme='dark']) .request-row.selected,
    :global(:root[data-theme='dark']) .request-row.live-request {
      background: #261f1d;
    }

    :global(:root[data-theme='dark']) .request-row:active {
      background: #1f2032;
    }

    :global(:root[data-theme='dark']) .request-row td.req-model {
      color: #f0edff;
    }

    :global(:root[data-theme='dark']) .request-row td.req-provider,
    :global(:root[data-theme='dark']) .request-row td.req-key {
      color: #a4a7be;
    }
  }

  /* ─── Ultra-compact Displays (320px - 360px) ─── */
  @media (max-width: 360px) {
    .request-row {
      padding: 12px 10px;
      border-radius: 12px;
      gap: 5px 8px;
    }
    .request-row td.req-model {
      font-size: 13px;
    }
    .request-row td.req-time {
      font-size: 11px;
    }
  }
</style>
