<script lang="ts">
  import Button from '../../components/ui/Button.svelte';
  import { formatCompactTokenTotal, formatCount } from '../../lib/format';
  import type { Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import type { DashboardPage } from '../../lib/navigation';
  import type { RequestLog, RequestStatistics } from '../../lib/types';

  interface Props {
    tr: Translate;
    locale: Locale;
    stats: RequestStatistics;
    recent: RequestLog[];
    onNavigate: (page: DashboardPage) => void;
  }
  let { tr, locale, stats, recent, onNavigate }: Props = $props();

  type TopModelRow = {
    key: string;
    model: string;
    provider: string;
    requests: number;
    share: number;
    tokens: number;
    latencyMs: number | null;
  };

  // Latency comes from the already-loaded recent logs. A model with no sample
  // stays blank instead of borrowing another model's timing.
  const latencySamples = $derived.by(() => {
    const samples = new Map<string, { total: number; count: number }>();
    for (const request of recent) {
      if (!request.model || request.duration_ms == null || !Number.isFinite(request.duration_ms)) continue;
      const key = `${request.model}\u0000${request.provider_id ?? ''}`;
      const sample = samples.get(key) ?? { total: 0, count: 0 };
      sample.total += request.duration_ms;
      sample.count += 1;
      samples.set(key, sample);
    }
    return samples;
  });

  const rows = $derived.by<TopModelRow[]>(() => {
    const total = stats.model_breakdown.reduce((sum, row) => sum + row.requests, 0);
    return [...stats.model_breakdown]
      .sort((a, b) => b.requests - a.requests)
      .slice(0, 8)
      .map((row, index) => {
        const key = `${row.model}\u0000${row.provider ?? ''}`;
        const sample = latencySamples.get(key);
        const share = total > 0 ? (row.requests / total) * 100 : 0;
        return {
          key: `${key}\u0000${index}`,
          model: row.model,
          provider: row.provider ?? '—',
          requests: row.requests,
          share: Math.round(Math.min(100, Math.max(0, share)) * 10) / 10,
          tokens: row.input_tokens + row.output_tokens,
          latencyMs: sample && sample.count > 0 ? Math.round(sample.total / sample.count) : null,
        };
      });
  });
</script>

<section class="recent-card" aria-label={tr('Top models')}>
  <div class="card-title-row"><h2>{tr('Top models')}</h2><Button class="mini-add" type="button" onclick={() => onNavigate('requests')}>{tr('View requests')}</Button></div>
  <div class="model-head" aria-hidden="true">
    <span>{tr('Model')}</span><span>{tr('Requests')}</span><span>{tr('Share')}</span><span>{tr('Tokens')}</span><span>{tr('Average latency')}</span>
  </div>
  <div class="model-list">
    {#if !rows.length}<p class="card-subtitle">{tr('No requests recorded yet')}</p>{/if}
    {#each rows as row (row.key)}
      <div class="model-row">
        <span class="model-name"><strong title={row.model}>{row.model}</strong><span class="provider-badge">{row.provider}</span></span>
        <span class="metric tabular-nums">{formatCount(row.requests, locale)}</span>
        <span class="share"><span class="share-track" aria-hidden="true"><i style={`width:${row.share}%`}></i></span><span class="share-value tabular-nums">{row.share}%</span></span>
        <span class="metric tabular-nums">{formatCompactTokenTotal(row.tokens, locale)}</span>
        <span class="metric tabular-nums">{row.latencyMs === null ? '—' : `${formatCount(row.latencyMs, locale)} ms`}</span>
      </div>
    {/each}
  </div>
</section>

<style>
  .model-head,
  .model-row { display:grid; grid-template-columns:minmax(0,1.5fr) 50px minmax(70px,0.9fr) 58px 74px; align-items:center; gap:8px; }
  .model-head { margin:10px 0 2px; color:var(--muted); font-size:var(--text-2xs, 10px); }
  .model-head span:not(:first-child) { text-align:right; }
  .model-list { display:flex; flex-direction:column; min-width:0; }
  .model-row { padding:8px 0; border-top:1px solid var(--border-subtle, var(--line)); font-size:var(--text-2xs, 11px); }
  .model-row:first-child { border-top:0; }
  .model-name { display:flex; flex-direction:column; gap:2px; min-width:0; }
  .model-name strong { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:var(--ink); font-weight:600; }
  .provider-badge { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:var(--muted); font-size:var(--text-2xs, 10px); }
  .metric { color:var(--ink); text-align:right; }
  .share { display:flex; flex-direction:column; align-items:flex-end; gap:3px; min-width:0; }
  .share-track { display:block; width:100%; height:5px; overflow:hidden; border-radius:999px; background:var(--surface-sunken); }
  .share-track i { display:block; height:100%; border-radius:999px; background:var(--accent); }
  .share-value { color:var(--muted); font-size:var(--text-2xs, 10px); }
</style>
