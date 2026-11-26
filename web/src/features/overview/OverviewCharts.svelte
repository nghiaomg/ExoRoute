<script lang="ts">
  import type { Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import { formatCount, formatCompactTokenTotal } from '../../lib/format';
  import type { ProviderHealth } from './overview.data';
  import type { RequestStatistics } from '../../lib/types';
  interface Props { tr: Translate; locale: Locale; stats: RequestStatistics | null; health: ProviderHealth | null }
  let { tr, locale, stats, health }: Props = $props();
  const SIZE = 120; const STROKE = 16; const R = (SIZE - STROKE) / 2; const C = 2 * Math.PI * R;
  type Slice = { label: string; value: number; color: string; text: string };
  function total(s: Slice[]): number { return s.reduce((sum, x) => sum + x.value, 0); }
  function arcs(slices: Slice[], center: string, sub = '') {
    const t = total(slices);
    return `<circle cx="${SIZE/2}" cy="${SIZE/2}" r="${R}" fill="none" stroke="var(--surface-sunken)" stroke-width="${STROKE}" />` + (t > 0 ? slices.map((slice, i) => {
      const before = slices.slice(0, i).reduce((sum, x) => sum + (x.value / t) * C, 0);
      const dash = Math.max(0, (slice.value / t) * C - 2);
      return `<circle cx="${SIZE/2}" cy="${SIZE/2}" r="${R}" fill="none" stroke="${slice.color}" stroke-width="${STROKE}" stroke-dasharray="${dash} ${C}" stroke-dashoffset="${-before}" transform="rotate(-90 ${SIZE/2} ${SIZE/2})"><title>${slice.label}: ${slice.text || slice.value}</title></circle>`;
    }).join('') : '') + `<text x="50%" y="54%" text-anchor="middle" fill="var(--ink)" font-size="11" font-weight="700">${center}</text>` + (sub ? `<text x="50%" y="68%" text-anchor="middle" fill="var(--muted)" font-size="8">${sub}</text>` : '');
  }
  const tokenSlices = $derived<Slice[]>(stats ? [
    { label: tr('Input tokens'), value: stats.input_tokens, color: 'var(--accent)', text: formatCompactTokenTotal(stats.input_tokens, locale) },
    { label: tr('Output tokens'), value: stats.output_tokens, color: 'var(--info)', text: formatCompactTokenTotal(stats.output_tokens, locale) },
    { label: tr('Cached tokens'), value: stats.cached_tokens, color: 'var(--chart-good)', text: formatCompactTokenTotal(stats.cached_tokens, locale) }
  ] : []);
  const statusSlices = $derived<Slice[]>(stats ? [
    { label: '2xx', value: stats.statuses.rows.filter((r) => r.status >= 200 && r.status < 300).reduce((s, r) => s + r.requests, 0), color: 'var(--chart-good)', text: '' },
    { label: '4xx', value: stats.statuses.rows.filter((r) => r.status >= 400 && r.status < 500).reduce((s, r) => s + r.requests, 0), color: 'var(--accent-strong)', text: '' },
    { label: '5xx', value: stats.statuses.rows.filter((r) => r.status >= 500 && r.status < 600).reduce((s, r) => s + r.requests, 0), color: 'var(--chart-critical)', text: '' }
  ] : []);
  const providerSlices = $derived<Slice[]>(health ? [
    { label: tr('Active'), value: health.active, color: 'var(--chart-good)', text: '' },
    { label: tr('Degraded'), value: health.degraded, color: 'var(--accent-strong)', text: '' },
    { label: tr('Disabled'), value: health.disabled, color: 'var(--muted)', text: '' }
  ] : []);
</script>
<section class="chart-grid" aria-label={tr('Token & cache distribution')}>
  <article class="mini-card"><h3>{tr('Token & cache distribution')}</h3><div class="mini-body">
    <svg class="donut" viewBox={`0 0 ${SIZE} ${SIZE}`} role="img" aria-label={tr('Token & cache distribution')}>{@html arcs(tokenSlices, stats ? Math.round(stats.cache_ratio * 1000) / 10 + '%' : '—', tr('Cache'))}</svg>
    <ul class="mini-legend">{#each tokenSlices as slice (slice.label)}<li><i style={`background:${slice.color}`}></i><span>{slice.label}</span><strong>{slice.text}</strong></li>{/each}</ul>
  </div></article>
  <article class="mini-card"><h3>{tr('HTTP status distribution')}</h3><div class="mini-body">
    <svg class="donut" viewBox={`0 0 ${SIZE} ${SIZE}`} role="img" aria-label={tr('HTTP status distribution')}>{@html arcs(statusSlices, formatCount(total(statusSlices), locale))}</svg>
    <ul class="mini-legend">{#each statusSlices as slice (slice.label)}<li><i style={`background:${slice.color}`}></i><span>{slice.label}</span><strong>{formatCount(slice.value, locale)}</strong></li>{/each}</ul>
  </div></article>
  <article class="mini-card"><h3>{tr('Provider health breakdown')}</h3><div class="mini-body">
    <svg class="donut" viewBox={`0 0 ${SIZE} ${SIZE}`} role="img" aria-label={tr('Provider health breakdown')}>{@html arcs(providerSlices, formatCount(total(providerSlices), locale))}</svg>
    <ul class="mini-legend">{#each providerSlices as slice (slice.label)}<li><i style={`background:${slice.color}`}></i><span>{slice.label}</span><strong>{formatCount(slice.value, locale)}</strong></li>{/each}</ul>
  </div></article>
</section>
<style>
  .chart-grid { display:grid; grid-template-columns:repeat(3,minmax(0,1fr)); gap:12px; }
  .mini-card { display:grid; gap:8px; padding:14px 16px; border:1px solid var(--line); border-radius:14px; background:var(--surface); min-width:0; }
  .mini-card h3 { margin:0; font-size:12px; color:var(--ink); }
  .mini-body { display:flex; align-items:center; gap:12px; min-width:0; }
  .donut { width:110px; height:110px; flex:0 0 110px; }
  .mini-legend { display:grid; gap:5px; margin:0; padding:0; list-style:none; min-width:0; flex:1; }
  .mini-legend li { display:flex; align-items:center; gap:6px; min-width:0; color:var(--muted); font-size:11px; }
  .mini-legend i { width:8px; height:8px; border-radius:2px; flex:0 0 8px; }
  .mini-legend span { flex:1; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .mini-legend strong { color:var(--ink); font-variant-numeric:tabular-nums; }
  @media (max-width:1100px){ .chart-grid { grid-template-columns:1fr; } }
</style>
