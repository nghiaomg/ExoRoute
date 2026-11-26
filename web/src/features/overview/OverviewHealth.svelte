<script lang="ts">
  import type { Translate } from '../../lib/format';
  import type { DashboardPage } from '../../lib/navigation';
  import type { ProviderHealth, TrafficRow } from './overview.data';
  interface Props { tr: Translate; health: ProviderHealth | null; combos: TrafficRow[]; models: TrafficRow[]; onNavigate: (page: DashboardPage) => void }
  let { tr, health, combos, models, onNavigate }: Props = $props();
</script>
<section class="overview-health" aria-label={tr('Provider health')}>
  <div class="health-card">
    <div class="card-title-row"><h2>{tr('Provider health')}</h2><button type="button" class="mini-link" onclick={() => onNavigate('providers')}>{tr('Providers')} →</button></div>
    {#if health}
      <div class="health-metrics">
        <div><strong class="tone-good">{health.active}</strong><span>{tr('Active')}</span></div>
        <div><strong class="tone-warn">{health.degraded}</strong><span>{tr('Degraded')}</span></div>
        <div><strong>{health.disabled}</strong><span>{tr('Disabled')}</span></div>
        <div><strong class="tone-warn">{health.invalidKeys}</strong><span>{tr('Invalid keys')}</span></div>
      </div>
      <button type="button" class="mini-link quota-link" onclick={() => onNavigate('quota')}>{tr('Quota')} →</button>
    {:else}<div class="health-empty"><p>{tr('No providers configured yet')} — {tr('Connect an AI provider to start routing requests.')}</p><button type="button" class="mini-link" onclick={() => onNavigate('providers')}>{tr('Add provider')} →</button></div>{/if}
  </div>
  <div class="health-card">
    <div class="card-title-row"><h2>{tr('Routing & traffic')}</h2><button type="button" class="mini-link" onclick={() => onNavigate('combos')}>{tr('Combo')} →</button></div>
    <div class="health-columns">
      <div><h3>{tr('Top combos')}</h3>
        {#if combos.length}
          {#each combos as row (row.id)}
            <div class="traffic-row" title={row.label}><span class="traffic-label">{row.label}</span><span class="traffic-bar"><i style="width:{Math.min(100, row.share)}%"></i></span><span class="numeric">{row.share}%</span><span class="numeric muted">{row.successRate}%</span></div>
          {/each}
        {:else}<div class="health-empty"><p>{tr('No combos or model traffic yet')}</p><button type="button" class="mini-link" onclick={() => onNavigate('combos')}>{tr('Configure combo')} →</button></div>{/if}
      </div>
      <div><h3>{tr('Top models')}</h3>
        {#if models.length}
          {#each models as row (row.id)}
            <div class="traffic-row" title={row.label}><span class="traffic-label">{row.label}</span><span class="traffic-bar"><i style="width:{Math.min(100, row.share)}%"></i></span><span class="numeric">{row.share}%</span><span class="numeric muted">{row.cacheHitRate === null ? '—' : row.cacheHitRate + '%'}</span></div>
          {/each}
        {:else}<p class="card-subtitle">{tr('No model traffic data')}</p>{/if}
      </div>
    </div>
  </div>
<style>
  .health-empty { display:grid; gap:6px; padding:8px 0; }
  .health-empty p { margin:0; color:var(--muted); font-size:12px; letter-spacing:0.01em; }
</style>
</section>
