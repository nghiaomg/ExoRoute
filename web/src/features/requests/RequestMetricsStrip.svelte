<script lang="ts">
  import { Activity, ListChecks, Percent, Timer } from '@lucide/svelte';
  import { formatCount, formatPercent, type Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import type { RequestPageMetrics } from './request.metrics';

  export let tr: Translate;
  export let locale: Locale;
  export let metrics: RequestPageMetrics;
  export let activeCount: number;
</script>

<!-- Page-scoped KPIs: every number describes the rows this page shows (plus the
     server's live active count), never a lifetime total. -->
<section class="request-metrics" aria-label={tr('Requests')}>
  <article class="request-metric active">
    <span class="request-metric-icon" aria-hidden="true"><Activity size={15} /></span>
    <span class="request-metric-label">{tr('Active now')}</span>
    <strong class="request-metric-value">{formatCount(activeCount, locale)}</strong>
  </article>
  <article class="request-metric total">
    <span class="request-metric-icon" aria-hidden="true"><ListChecks size={15} /></span>
    <span class="request-metric-label">{tr('Requests on this page')}</span>
    <strong class="request-metric-value">{formatCount(metrics.total, locale)}</strong>
  </article>
  <article class="request-metric success">
    <span class="request-metric-icon" aria-hidden="true"><Percent size={15} /></span>
    <span class="request-metric-label">{tr('Success rate')}</span>
    <strong class="request-metric-value">{formatPercent(metrics.successRate, locale)}</strong>
  </article>
  <article class="request-metric duration">
    <span class="request-metric-icon" aria-hidden="true"><Timer size={15} /></span>
    <span class="request-metric-label">{tr('p95 duration')}</span>
    <strong class="request-metric-value">
      {metrics.p95DurationMs == null ? '—' : `${metrics.p95DurationMs} ms`}
    </strong>
  </article>
</section>

<style>
  .request-metrics {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 12px;
    margin: 0 0 18px;
  }

  .request-metric {
    position: relative;
    display: grid;
    grid-template-columns: auto 1fr;
    grid-template-areas:
      'icon label'
      'icon value';
    align-items: center;
    gap: 3px 12px;
    padding: 14px 16px;
    border: var(--nb-border);
    border-radius: 12px;
    background: var(--surface);
    box-shadow: var(--nb-shadow-md);
    transition: transform 0.16s ease, box-shadow 0.16s ease, border-color 0.16s ease;
  }

  @media (hover: hover) {
    .request-metric:hover {
      transform: translate(-1px, -1px);
      box-shadow: var(--nb-shadow-lg);
    }
  }

  /* Icon tiles carry the same ink frame and hard offset as `.combo-icon`. */
  .request-metric-icon {
    grid-area: icon;
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border-radius: 9px;
    border: var(--nb-border);
    box-shadow: var(--nb-shadow-sm);
    transition: transform 0.16s ease;
  }

  .request-metric.active .request-metric-icon {
    background: color-mix(in srgb, var(--accent) 7%, var(--surface));
    color: var(--accent-strong);
  }

  .request-metric.total .request-metric-icon {
    background: color-mix(in srgb, var(--info) 10%, var(--surface));
    color: var(--info);
  }

  /* Project mint (--mint), so the "Successful" tile agrees with the outcome
     bar segment and the row accent. */
  .request-metric.success .request-metric-icon {
    background: color-mix(in srgb, var(--chart-good) 10%, var(--surface));
    color: var(--chart-good);
  }

  .request-metric.duration .request-metric-icon {
    background: var(--surface-sunken);
    color: var(--muted);
  }

  .request-metric-label {
    grid-area: label;
    color: var(--muted);
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.5px;
    text-transform: uppercase;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .request-metric-value {
    grid-area: value;
    color: var(--ink);
    font-family: var(--font-heading);
    font-size: 22px;
    font-weight: 800;
    font-variant-numeric: tabular-nums;
    line-height: 1.2;
  }

  @media (max-width: 1050px) {
    .request-metrics {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  @media (max-width: 420px) {
    .request-metric {
      padding: 10px 12px;
      gap: 2px 10px;
    }
    .request-metric-value {
      font-size: 18px;
    }
  }
</style>
