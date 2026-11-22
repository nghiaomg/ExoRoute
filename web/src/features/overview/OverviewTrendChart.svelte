<script lang="ts">
  import type { Translate } from '../../lib/format';
  import { formatCount } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import { trendPoints } from './overview.data';
  import type { RequestStatistics } from '../../lib/types';
  interface Props { tr: Translate; locale: Locale; stats: RequestStatistics | null }
  let { tr, locale, stats }: Props = $props();
  let metric = $state<'requests' | 'tokens' | 'latency'>('requests');
  let hover = $state<number | null>(null);
  const points = $derived(stats ? trendPoints(stats, metric) : []);
  const values = $derived(points.map((p) => metric === 'requests' ? p.requests : metric === 'tokens' ? p.tokens : p.latencyMs));
  const peak = $derived(Math.max(1, ...values));
  const W = 480; const H = 150; const PAD = 8;
  function x(i: number): number { return points.length <= 1 ? W / 2 : PAD + (i / (points.length - 1)) * (W - PAD * 2); }
  function y(v: number): number { return H - PAD - (v / peak) * (H - PAD * 2); }
  const path = $derived(values.map((v, i) => `${i === 0 ? 'M' : 'L'} ${x(i).toFixed(1)} ${y(v).toFixed(1)}`).join(' '));
  const area = $derived(`${path} L ${x(values.length - 1).toFixed(1)} ${H - PAD} L ${x(0).toFixed(1)} ${H - PAD} Z`);
  const active = $derived(hover != null ? points[hover] : null);
  const activeValue = $derived(hover != null ? values[hover] : null);
  function unit(v: number): string { return metric === 'latency' ? 'ms' : metric === 'tokens' ? tr('Tokens') : tr('requests'); }
</script>
<section class="chart-card" aria-label={tr('Traffic & token trend')}>
  <div class="chart-head"><h2>{tr('Traffic & token trend')}</h2><div class="metric-pills" role="group">
    {#each [{ k: 'requests', l: tr('Requests') }, { k: 'tokens', l: tr('Tokens') }, { k: 'latency', l: tr('Latency') }] as m (m.k)}
      <button type="button" aria-pressed={metric === m.k} class:active={metric === m.k} onclick={() => { metric = m.k as typeof metric; hover = null; }}>{m.l}</button>
    {/each}
  </div></div>
  {#if points.length === 0}<p class="chart-empty">{tr('No data in this window')}</p>
  {:else}
    <svg class="trend-svg" viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`${tr('Traffic & token trend')}: ${formatCount(values[values.length - 1], locale)} ${unit(values[values.length - 1])}`}>
      <path class="area" d={area} />
      <path class="line" d={path} />
      {#each values as v, i}
        <circle class="point" class:active={hover === i} cx={x(i)} cy={y(v)} r={hover === i ? 4 : 2} tabindex="0" role="button" aria-label={`${points[i].label}: ${formatCount(v, locale)} ${unit(v)}`} onmouseenter={() => (hover = i)} onfocus={() => (hover = i)} onmouseleave={() => (hover = null)} onblur={() => (hover = null)} />
      {/each}
    </svg>
    <div class="chart-foot"><span>{points[0].label}</span><span class="hover-readout">{#if active}{active.label} · <strong>{formatCount(activeValue ?? 0, locale)} {unit(activeValue ?? 0)}</strong>{/if}</span><span>{points[points.length - 1].label}</span></div>
  {/if}
</section>
<style>
  .chart-card { display:grid; gap:8px; padding:14px 16px; border:1px solid var(--line); border-radius:14px; background:var(--surface); min-width:0; }
  .chart-head { display:flex; flex-wrap:wrap; align-items:center; justify-content:space-between; gap:8px; }
  .chart-head h2 { margin:0; font-size:13px; color:var(--ink); }
  .metric-pills { display:inline-flex; gap:2px; padding:2px; border:1px solid var(--line); border-radius:999px; background:var(--surface-sunken); }
  .metric-pills button { border:0; border-radius:999px; padding:3px 9px; background:transparent; color:var(--muted); font-size:11px; cursor:pointer; }
  .metric-pills button.active { background:var(--accent); color:var(--accent-contrast); }
  .metric-pills button:focus-visible, .point:focus-visible { outline:2px solid var(--accent); outline-offset:2px; }
  .trend-svg { width:100%; height:auto; max-height:170px; overflow:visible; }
  .trend-svg .area { fill:color-mix(in srgb, var(--accent) 14%, transparent); }
  .trend-svg .line { fill:none; stroke:var(--accent); stroke-width:2; }
  .trend-svg .point { fill:var(--accent); cursor:pointer; }
  .trend-svg .point.active { fill:var(--accent-contrast); stroke:var(--accent); stroke-width:2; }
  .chart-foot { display:flex; justify-content:space-between; gap:8px; color:var(--muted); font-size:10px; }
  .hover-readout strong { color:var(--ink); font-variant-numeric:tabular-nums; }
  .chart-empty { margin:0; display:grid; place-items:center; min-height:150px; color:var(--muted); font-size:12px; }
</style>
