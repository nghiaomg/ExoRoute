<script lang="ts">
  import type { Translate } from '../../lib/format';
  import type { StatisticsDimension } from '../../lib/types';

  export let title: string;
  export let items: StatisticsDimension[];
  export let others: number;
  export let total: number;
  export let tr: Translate;
  export let formatCount: (value: number) => string;

  const radius = 43;
  const circumference = 2 * Math.PI * radius;
  const colors = ['#f97316', '#36c59b', '#f0a347', '#db6683', '#56a5dc', '#a5a9bb'];

  function buildSegments(): Array<StatisticsDimension & { color: string; offset: number }> {
    const segments: StatisticsDimension[] = [...items];
    if (others > 0) {
      segments.push({ id: 'others', label: tr('Others'), requests: others, percentage: total > 0 ? others / total * 100 : 0 });
    }
    let offset = 0;
    return segments.map((segment, index) => {
      const percent = Math.max(0, Math.min(100, segment.percentage));
      const result = { ...segment, percentage: percent, color: colors[index % colors.length], offset };
      offset += circumference * percent / 100;
      return result;
    });
  }

  $: segments = buildSegments();
</script>

<section class="donut-card" aria-label={title}>
  <div class="donut-heading"><h2>{title}</h2><span>{tr('Top 5 + Others')}</span></div>
  <div class="donut-body">
    <div class="donut-visual">
      <svg viewBox="0 0 110 110" role="img" aria-label={tr('{count} requests', { count: formatCount(total) })}>
        <circle cx="55" cy="55" r={radius} fill="none" stroke="var(--line)" stroke-width="14" />
        {#each segments as segment (segment.id)}
          <circle
            class="segment-circle"
            cx="55" cy="55" r={radius} fill="none" stroke={segment.color} stroke-width="14"
            stroke-dasharray={`${circumference * segment.percentage / 100} ${circumference}`}
            stroke-dashoffset={-segment.offset} transform="rotate(-90 55 55)"
          />
        {/each}
        <text x="55" y="53" text-anchor="middle" class="donut-total">{formatCount(total)}</text>
        <text x="55" y="67" text-anchor="middle" class="donut-caption">{tr('requests')}</text>
      </svg>
    </div>
    <div class="donut-legend">
      {#if segments.length}
        {#each segments as segment (segment.id)}
          <div class="legend-row">
            <span class="legend-dot" style={`--segment-color:${segment.color}`}></span>
            <span class="legend-label" title={segment.label}>{segment.label}</span>
            <span class="legend-value">{formatCount(segment.requests)}</span>
            <span class="legend-percent">{segment.percentage.toFixed(1)}%</span>
          </div>
        {/each}
      {:else}
        <p class="donut-empty">{tr('No traffic in this period.')}</p>
      {/if}
    </div>
  </div>
</section>

<style>
  .donut-card { min-width: 0; padding: 20px; border: 1px solid var(--line); border-radius: 14px; background: var(--paper); }
  .donut-heading { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; margin-bottom: 16px; }
  .donut-heading h2 { margin: 0; color: var(--ink); font: 650 15px/1.35 var(--font-sans); }
  .donut-heading > span { color: var(--muted); font-size: 12px; white-space: nowrap; }
  .donut-body { display: grid; grid-template-columns: 132px minmax(0, 1fr); align-items: center; gap: 16px; }
  .donut-visual { width: 132px; }
  .donut-visual svg { display: block; width: 100%; overflow: visible; }
  .segment-circle { transition: stroke-dasharray 0.35s cubic-bezier(0.4, 0, 0.2, 1), stroke-dashoffset 0.35s cubic-bezier(0.4, 0, 0.2, 1); }
  .donut-total { fill: var(--ink); font: 700 15px var(--font-mono); }
  .donut-caption { fill: var(--muted); font: 500 12px var(--font-sans); }
  .donut-legend { min-width: 0; display: grid; gap: 10px; }
  .legend-row { min-width: 0; display: grid; grid-template-columns: 8px minmax(0, 1fr) auto 42px; align-items: center; gap: 8px; font-size: 12px; }
  .legend-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--segment-color); }
  .legend-label { overflow: hidden; color: var(--ink); text-overflow: ellipsis; white-space: nowrap; }
  .legend-value { color: var(--ink); font-family: var(--font-mono); }
  .legend-percent { color: var(--muted); text-align: right; font-family: var(--font-mono); }
  .donut-empty { margin: 0; color: var(--muted); font-size: 12px; }
  @media (max-width: 700px) { .donut-body { grid-template-columns: 105px minmax(0, 1fr); gap: 10px; } .donut-visual { width: 105px; } .legend-row { grid-template-columns: 8px minmax(0, 1fr) auto; } .legend-percent { display: none; } }
</style>
