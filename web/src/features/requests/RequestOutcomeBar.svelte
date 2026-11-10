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
    border: var(--nb-border);
    border-radius: 12px;
    background: var(--surface);
    box-shadow: var(--nb-shadow-md);
  }

  .request-outcome-bar {
    display: flex;
    flex: 1 1 auto;
    min-width: 80px;
    height: 9px;
    overflow: hidden;
    border: none;
    border-radius: 999px;
    background: var(--surface-sunken);
  }

  .request-outcome-segment {
    height: 100%;
    min-width: 3px;
  }

  /* The three segment colours are the row stripe accents, so the legend and
     the rows always agree. */
  .request-outcome-segment:global(.active) {
    background: var(--accent);
  }

  .request-outcome-segment.success {
    background: var(--chart-good);
  }

  .request-outcome-segment.failure {
    background: var(--chart-critical);
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
    color: var(--muted);
    font-size: 12px;
    font-weight: 500;
  }

  .request-outcome-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    border: none;
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--ink) 6%, transparent);
  }

  .request-outcome-entry:global(.active) .request-outcome-dot {
    background: var(--accent);
  }

  .request-outcome-entry.success .request-outcome-dot {
    background: var(--chart-good);
  }

  .request-outcome-entry.failure .request-outcome-dot {
    background: var(--chart-critical);
  }

  .request-outcome-count {
    color: var(--ink);
    font-family: var(--font-mono);
    font-size: 12.5px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }

  @media (max-width: 650px) {
    .request-outcome {
      flex-direction: column;
      align-items: stretch;
      gap: 10px;
    }
  }
</style>
