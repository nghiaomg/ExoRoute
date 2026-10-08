<script lang="ts">
  import { formatCount, type Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import type { RequestPageMetrics } from './request.metrics';

  export let tr: Translate;
  export let locale: Locale;
  export let metrics: RequestPageMetrics;

  $: divisor = Math.max(1, metrics.total);
  $: segments = [
    { key: 'active', label: tr('In progress'), count: metrics.counts.active },
    { key: 'success', label: tr('Successful'), count: metrics.counts.success },
    { key: 'failure', label: tr('Failed'), count: metrics.counts.failure },
  ].filter((segment) => segment.count > 0);
</script>

<section class="request-outcome" aria-label={tr('Outcome mix')}>
  <div class="request-outcome-bar" role="img" aria-label={tr('Outcome mix')}>
    {#each segments as segment (segment.key)}
      <span class="request-outcome-segment {segment.key}" style="width: {(segment.count / divisor) * 100}%"></span>
    {/each}
  </div>
  <ul class="request-outcome-legend">
    {#each segments as segment (segment.key)}
      <li class="request-outcome-entry {segment.key}">
        <span class="request-outcome-dot" aria-hidden="true"></span>
        <span class="request-outcome-label">{segment.label}</span>
        <strong class="request-outcome-count">{formatCount(segment.count, locale)}</strong>
      </li>
    {/each}
  </ul>
</section>

<style>
  .request-outcome {
    display: flex;
    align-items: center;
    gap: 16px;
    margin: 0 0 18px;
    padding: 12px 16px;
    border: 2px solid var(--ink);
    border-radius: 12px;
    background: #ffffff;
    box-shadow: 3px 3px 0 var(--ink);
  }

  .request-outcome-bar {
    display: flex;
    flex: 1 1 auto;
    min-width: 80px;
    height: 9px;
    overflow: hidden;
    border: none;
    border-radius: 999px;
    background: #eceef4;
  }

  .request-outcome-segment {
    height: 100%;
    min-width: 3px;
  }

  /* The three segment colours are the row stripe accents, so the legend and
     the rows always agree. */
  .request-outcome-segment.active {
    background: #f97316;
  }

  .request-outcome-segment.success {
    background: #36c59b;
  }

  .request-outcome-segment.failure {
    background: #db6683;
  }

  .request-outcome-legend {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 14px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .request-outcome-entry {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    color: #85899b;
    font-size: 12px;
    font-weight: 500;
  }

  .request-outcome-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    border: none;
    box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.06);
  }

  .request-outcome-entry.active .request-outcome-dot {
    background: #f97316;
  }

  .request-outcome-entry.success .request-outcome-dot {
    background: #36c59b;
  }

  .request-outcome-entry.failure .request-outcome-dot {
    background: #db6683;
  }

  .request-outcome-count {
    color: #33374b;
    font-family: var(--font-mono);
    font-size: 12.5px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }

  :global(:root[data-theme='dark']) .request-outcome {
    border-color: rgba(255, 255, 255, 0.08);
    background: #181926;
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.35);
  }

  :global(:root[data-theme='dark']) .request-outcome-bar {
    background: #101120;
  }

  :global(:root[data-theme='dark']) .request-outcome-entry {
    color: #8e93aa;
  }

  :global(:root[data-theme='dark']) .request-outcome-count {
    color: #f0edff;
  }

  @media (max-width: 650px) {
    .request-outcome {
      flex-direction: column;
      align-items: stretch;
      gap: 10px;
    }
  }
</style>
