<script lang="ts">
  import { formatCount } from '../../lib/format';
  import type { Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';

  interface Slice {
    id: string;
    label: string;
    value: number;
    color: string;
  }

  interface Props {
    title: string;
    slices: Slice[];
    tr: Translate;
    locale: Locale;
    centerValue?: string;
    centerLabel?: string;
    description?: string;
    emptyLabel?: string;
  }

  let { title, slices, tr, locale, centerValue = '', centerLabel = '', description = '', emptyLabel }: Props = $props();

  const size = 164;
  const stroke = 20;
  const radius = (size - stroke) / 2;
  const circumference = 2 * Math.PI * radius;
  const headingId = $derived('donut-' + title.replace(/[^a-zA-Z0-9]/g, ''));

  const rows = $derived.by(() => {
    const positive = slices.filter((slice) => slice.value > 0);
    const total = positive.reduce((sum, slice) => sum + slice.value, 0);
    let offset = 0;
    return positive.map((slice) => {
      const share = total > 0 ? slice.value / total : 0;
      const dash = share * circumference;
      const arc = { ...slice, share, percent: share * 100, dash, offset };
      offset += dash;
      return arc;
    });
  });
  const total = $derived(rows.reduce((sum, row) => sum + row.value, 0));
  const chartLabel = $derived(title + ': ' + rows.map((row) => row.label + ' ' + formatCount(row.value, locale)).join(', '));
</script>

<section class="donut-card">
  <div class="donut-card-head">
    <h3 id={headingId}>{title}</h3>
    {#if description}<p>{description}</p>{/if}
  </div>
  {#if rows.length}
    <div class="donut-layout">
      <svg class="donut" viewBox={'0 0 ' + size + ' ' + size} role="img" aria-label={chartLabel}>
        <circle class="donut-track" cx={size / 2} cy={size / 2} r={radius} fill="none" stroke-width={stroke} />
        {#each rows as row (row.id)}
          <circle
            class="donut-arc"
            cx={size / 2}
            cy={size / 2}
            r={radius}
            fill="none"
            stroke={row.color}
            stroke-width={stroke}
            stroke-dasharray={row.dash + ' ' + (circumference - row.dash)}
            stroke-dashoffset={-row.offset}
            transform={'rotate(-90 ' + (size / 2) + ' ' + (size / 2) + ')'}
          />
        {/each}
        <text class="donut-center-value" x={size / 2} y={size / 2 - 3} text-anchor="middle">{centerValue || formatCount(total, locale)}</text>
        {#if centerLabel}<text class="donut-center-label" x={size / 2} y={size / 2 + 15} text-anchor="middle">{centerLabel}</text>{/if}
      </svg>
      <ul class="donut-legend">
        {#each rows as row (row.id)}
          <li>
            <span class="legend-dot" style={'background:' + row.color} aria-hidden="true"></span>
            <span class="legend-label" title={row.label}>{row.label}</span>
            <strong class="numeric">{formatCount(row.value, locale)}</strong>
            <span class="legend-share numeric">{row.percent.toFixed(1)}%</span>
          </li>
        {/each}
      </ul>
    </div>
  {:else}
    <p class="donut-empty">{emptyLabel ?? tr('No traffic for this API key in the selected period.')}</p>
  {/if}
</section>

<style>
  .donut-card { display:grid; gap:12px; min-width:0; padding:16px; border:1px solid var(--line); border-radius:14px; background:var(--surface); }
  .donut-card-head h3 { margin:0; color:var(--ink); font:650 13px var(--font-sans); }
  .donut-card-head p { margin:4px 0 0; color:var(--muted); font-size:11px; line-height:1.4; }
  .donut-layout { display:grid; grid-template-columns:auto minmax(0,1fr); align-items:center; gap:14px; }
  .donut { width:120px; height:120px; flex:0 0 120px; }
  .donut-track { stroke:var(--surface-sunken); }
  .donut-arc { stroke-linecap:round; }
  .donut-center-value { fill:var(--ink); font:700 20px var(--font-sans); }
  .donut-center-label { fill:var(--muted); font:600 9px var(--font-sans); text-transform:uppercase; letter-spacing:.08em; }
  .donut-legend { display:grid; gap:6px; min-width:0; margin:0; padding:0; list-style:none; }
  .donut-legend li { display:grid; grid-template-columns:8px minmax(0,1fr) auto auto; align-items:center; gap:6px; color:var(--ink); font-size:11px; }
  .legend-dot { width:8px; height:8px; border-radius:50%; }
  .legend-label { overflow:hidden; color:var(--muted); text-overflow:ellipsis; white-space:nowrap; }
  .donut-legend strong { font-weight:650; }
  .legend-share { color:var(--muted); }
  .numeric { font-variant-numeric:tabular-nums; }
  .donut-empty { margin:0; padding:22px 10px; color:var(--muted); font-size:12px; text-align:center; }
  @media (max-width:520px){ .donut-layout { grid-template-columns:1fr; justify-items:center; } .donut-legend { width:100%; } }
</style>
