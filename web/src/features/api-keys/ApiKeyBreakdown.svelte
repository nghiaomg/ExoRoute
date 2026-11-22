<script lang="ts">
  import { api } from '../../lib/api';
  import { formatCount, formatCompactTokenTotal } from '../../lib/format';
  import type { Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import type { GatewayApiKey } from '../../lib/types';
  import { EMPTY_METRICS, KEY_RANGES, loadKeyMetrics, type KeyMetrics, type KeyRange } from './api-keys.data';
  interface Props { tr: Translate; locale: Locale; keys: GatewayApiKey[]; range?: KeyRange; onRangeChange?: (r: KeyRange) => void; onInspect: (key: GatewayApiKey) => void }
  let { tr, locale, keys, range = '1d', onRangeChange = () => {}, onInspect }: Props = $props();
  type Metric = 'requests' | 'tokens' | 'failures';
  let metric = $state<Metric>('requests');
  let active = $state<string | null>(null);
  let metrics = $state<Record<string, KeyMetrics>>({});
  let loading = $state(false);
  let partialError = $state(false);
  const SIZE = 180; const STROKE = 22; const R = (SIZE - STROKE) / 2; const CIRC = 2 * Math.PI * R;
  const COLORS = ['var(--accent)','var(--chart-good)','var(--info)','var(--accent-strong)','var(--mint)','var(--chart-critical)','var(--accent-glow)','var(--muted)'];
  const metricLabel = $derived(metric === 'requests' ? tr('Requests') : metric === 'tokens' ? tr('Tokens') : tr('Failed requests'));
  const metricUnit = $derived(metric === 'requests' ? tr('requests') : metric === 'tokens' ? tr('Tokens') : tr('failed'));
  const values = $derived(keys.map((key) => ({ id: key.id, name: key.name, value: metric === 'requests' ? (metrics[key.id]?.requests ?? 0) : metric === 'tokens' ? (metrics[key.id]?.tokens ?? 0) : (metrics[key.id]?.failures ?? 0) })).filter((row) => metrics[row.id]?.loaded));
  const total = $derived(values.reduce((sum, row) => sum + row.value, 0));
  const slices = $derived.by(() => {
    const sorted = [...values].filter((row) => row.value > 0).sort((a, b) => b.value - a.value);
    const top = sorted.slice(0, 7);
    const tail = sorted.slice(7);
    const rows = tail.length ? [...top, { id: '__other', name: tr('Other'), value: tail.reduce((s, row) => s + row.value, 0) }] : top;
    let offset = 0;
    return rows.map((row, i) => {
      const share = total > 0 ? row.value / total : 0;
      const dash = share * CIRC;
      const slice = { ...row, share, pct: Math.round(share * 1000) / 10, offset, color: COLORS[i % COLORS.length] };
      offset += dash;
      return slice;
    });
  });
  const activeSlice = $derived(slices.find((s) => s.id === active) ?? null);
  const centerValue = $derived(activeSlice ? `${formatMetric(activeSlice.value)} ${metricUnit}` : `${formatMetric(total)} ${metricUnit}`);
  function formatMetric(value: number): string { return metric === 'tokens' ? formatCompactTokenTotal(value, locale) : formatCount(value, locale); }
  function onEnter(id: string): void { active = id; }
  function onLeave(): void { active = null; }
  function select(id: string): void { const key = keys.find((k) => k.id === id); if (key) onInspect(key); }
  // Each dependency change starts a fresh cycle and aborts the previous one;
  // cleanup also covers unmount so no superseded request keeps running.
  $effect(() => {
    const cycleKeys = keys;
    const cycleRange = range;
    const controller = new AbortController();
    if (!cycleKeys.length) {
      metrics = {}; partialError = false; loading = false;
      return () => controller.abort();
    }
    loading = true; partialError = false;
    void loadKeyMetrics(
      cycleKeys,
      cycleRange,
      (id, r, signal) => api.apiKeyStatistics(id, r, signal),
      controller.signal,
    ).then((result) => {
      if (controller.signal.aborted) return;
      metrics = result.metrics; partialError = result.partialError;
    }).finally(() => {
      if (!controller.signal.aborted) loading = false;
    });
    return () => controller.abort();
  });
</script>
<section class="breakdown" aria-label={tr('Key usage breakdown')}>
  <div class="breakdown-head">
    <h2>{tr('Key usage breakdown')}</h2>
    <div class="breakdown-controls">
      <div class="metric-switch" role="group" aria-label={metricLabel}>
        {#each [{ k: 'requests', l: tr('Requests') }, { k: 'tokens', l: tr('Tokens') }, { k: 'failures', l: tr('Failed requests') }] as option (option.k)}
          <button type="button" aria-pressed={metric === option.k} class:active={metric === option.k} onclick={() => { metric = option.k as Metric; active = null; }}>{option.l}</button>
        {/each}
      </div>
      <div class="range-inline"><span>{tr('Time window')}</span><select aria-label={tr('Time window')} value={range} onchange={(e) => onRangeChange(e.currentTarget.value as KeyRange)}>{#each KEY_RANGES as option (option.key)}<option value={option.key}>{tr(option.label === '24H' ? '24 hours' : option.label === '7D' ? '7 days' : '30 days')}</option>{/each}</select></div>
    </div>
  </div>
  {#if partialError}<p class="breakdown-note">{tr('Telemetry temporarily unavailable for some keys.')}</p>{/if}
  {#if loading && !Object.values(metrics).some((m) => m.loaded)}
    <p class="breakdown-note">{tr('Loading statistics')}…</p>
  {:else if total === 0}
    <div class="donut-layout empty-layout">
      <svg class="donut" viewBox={`0 0 ${SIZE} ${SIZE}`} role="img" aria-label={tr('No usage data')}><circle cx={SIZE/2} cy={SIZE/2} r={R} fill="none" stroke="var(--line)" stroke-width={STROKE} /><text x="50%" y="50%" text-anchor="middle" dominant-baseline="middle" fill="var(--muted)" font-size="11">{tr('No usage data')}</text></svg>
      <p class="breakdown-hint">{tr('Hover or focus a segment to inspect')}</p>
    </div>
  {:else}
    <div class="donut-layout">
      <div class="donut-wrap">
        <svg class="donut" viewBox={`0 0 ${SIZE} ${SIZE}`} role="img" aria-label={`${metricLabel}: ${centerValue}`}>
          <circle cx={SIZE/2} cy={SIZE/2} r={R} fill="none" stroke="var(--surface-sunken)" stroke-width={STROKE} />
          {#each slices as slice (slice.id)}
            <circle class="slice" class:dim={active !== null && active !== slice.id} class:active={active === slice.id} cx={SIZE/2} cy={SIZE/2} r={R} fill="none" stroke={slice.color} stroke-width={active === slice.id ? STROKE + 4 : STROKE} stroke-dasharray={`${Math.max(0, slice.share * CIRC - 2)} ${CIRC}`} stroke-dashoffset={-slice.offset} transform={`rotate(-90 ${SIZE/2} ${SIZE/2})`} tabindex="0" role="button" aria-label={`${slice.name} · ${formatMetric(slice.value)} ${metricUnit} · ${slice.pct}%`} onmouseenter={() => onEnter(slice.id)} onfocus={() => onEnter(slice.id)} onmouseleave={onLeave} onblur={onLeave} />
          {/each}
        </svg>
        <div class="donut-center"><strong>{activeSlice ? activeSlice.name : metricLabel}</strong><span>{centerValue}</span><small>{activeSlice ? `${activeSlice.pct}% ${tr('of total')}` : ''}</small></div>
      </div>
      <div class="legend">
        {#each slices as slice (slice.id)}
          <button type="button" class="legend-row" class:active={active === slice.id} onmouseenter={() => onEnter(slice.id)} onfocus={() => onEnter(slice.id)} onmouseleave={onLeave} onblur={onLeave} onclick={() => select(slice.id)}>
            <span class="swatch" style={`background:${slice.color}`} aria-hidden="true"></span>
            <span class="legend-name" title={slice.name}>{slice.name}</span>
            <span class="legend-value">{formatMetric(slice.value)} {metricUnit}</span>
            <span class="legend-share">{slice.pct}%</span>
          </button>
        {/each}
      </div>
    </div>
  {/if}
</section>
<style>
  .breakdown { padding:14px 16px; border:1px solid var(--line); border-radius:var(--radius-lg, 12px); background:var(--surface); }
  .breakdown-head { display:flex; flex-wrap:wrap; align-items:center; justify-content:space-between; gap:8px; }
  .breakdown-head h2 { margin:0; font-size:13px; color:var(--ink); }
  .breakdown-controls { display:flex; flex-wrap:wrap; align-items:center; gap:8px; }
  .metric-switch { display:inline-flex; gap:2px; padding:2px; border:1px solid var(--line); border-radius:999px; background:var(--surface-sunken); }
  .metric-switch button { border:0; border-radius:999px; padding:3px 9px; background:transparent; color:var(--muted); font-size:11px; cursor:pointer; }
  .metric-switch button.active { background:var(--accent); color:var(--accent-contrast); }
  .metric-switch button:focus-visible, .legend-row:focus-visible, .slice:focus-visible { outline:2px solid var(--accent); outline-offset:2px; }
  .range-inline { display:inline-flex; align-items:center; gap:6px; color:var(--muted); font-size:11px; }
  .range-inline select { border:1px solid var(--line); border-radius:8px; background:var(--surface-sunken); color:var(--ink); padding:4px 8px; font-size:11px; }
  .breakdown-note, .breakdown-hint { margin:10px 0 0; color:var(--muted); font-size:12px; }
  .donut-layout { display:grid; grid-template-columns:200px minmax(0,1fr); gap:18px; align-items:center; margin-top:12px; }
  .donut-wrap { position:relative; width:200px; height:200px; }
  .donut { width:100%; height:100%; overflow:visible; }
  .slice { cursor:pointer; transition:stroke-width .15s ease, opacity .15s ease; }
  .slice.dim { opacity:.45; }
  .donut-center { position:absolute; inset:0; display:grid; align-content:center; justify-items:center; gap:2px; pointer-events:none; text-align:center; }
  .donut-center strong { max-width:150px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:var(--ink); font-size:12px; }
  .donut-center span { color:var(--ink); font-size:16px; font-weight:700; font-variant-numeric:tabular-nums; }
  .donut-center small { color:var(--muted); font-size:10px; }
  .legend { display:grid; gap:3px; max-height:210px; overflow:auto; }
  .legend-row { display:grid; grid-template-columns:10px minmax(0,1fr) auto 42px; align-items:center; gap:8px; width:100%; padding:6px 8px; border:1px solid transparent; border-radius:8px; background:transparent; color:var(--ink); font-size:11px; text-align:left; cursor:pointer; }
  .legend-row:hover, .legend-row.active { border-color:var(--line); background:var(--surface-sunken); }
  .swatch { width:9px; height:9px; border-radius:3px; }
  .legend-name { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .legend-value, .legend-share { font-variant-numeric:tabular-nums; }
  .legend-share { text-align:right; color:var(--muted); }
  .empty-layout { grid-template-columns:200px minmax(0,1fr); }
  @media (max-width:700px){ .donut-layout, .empty-layout { grid-template-columns:1fr; justify-items:center; } .donut-wrap { width:180px; height:180px; } .legend { width:100%; } }
</style>
