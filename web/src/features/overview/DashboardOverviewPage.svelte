<script lang="ts">
  import Button from '../../components/ui/Button.svelte';
  import StatsCard from '../../components/ui/StatsCard.svelte';
  import Input from '../../components/ui/Input.svelte';
  import SelectOptions from '../../components/ui/SelectOptions.svelte';
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import { proxyApi } from '../../lib/api/proxies';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import { formatCount, formatCompactTokenTotal, formatGatewayEndpoint } from '../../lib/format';
  import type { Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import type { DashboardPage } from '../../lib/navigation';
  import type { GatewayCombo, Overview, Provider, ProviderQuotaResult, RequestLog, RequestStatistics } from '../../lib/types';
  import OverviewOpsBar from './OverviewOpsBar.svelte';
  import OverviewNotice from './OverviewNotice.svelte';
  import OverviewHealth from './OverviewHealth.svelte';
  import OverviewTrendChart from './OverviewTrendChart.svelte';
  import OverviewCharts from './OverviewCharts.svelte';
  import OverviewRequests from './OverviewRequests.svelte';
  import OverviewQuickstart from './OverviewQuickstart.svelte';
  import { comboTraffic, LATENCY_TARGET_MS, modelTraffic, providerHealth, RANGES, type RangeKey } from './overview.data';
  interface Props { tr: Translate; locale: Locale; onNavigate: (page: DashboardPage) => void; onCreateRequest: (page: DashboardPage) => void; onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void; onProviderCountChange: (count: number) => void; onGatewayAddressChange: (host: unknown, port: unknown) => void }
  let { tr, locale, onNavigate, onCreateRequest: _onCreateRequest, onConnectionChange, onProviderCountChange, onGatewayAddressChange }: Props = $props();
  // Kept for API compatibility; the page opens the key dialog directly.
  let overview = $state<Overview | null>(null);
  let stats = $state<RequestStatistics | null>(null);
  let recent = $state<RequestLog[]>([]);
  let providers = $state<Provider[]>([]);
  let combos = $state<GatewayCombo[]>([]);
  let quotas = $state<Map<string, ProviderQuotaResult>>(new Map());
  let proxyCount = $state(0);
  let proxiesEnabled = $state(0);
  let errorMessage = $state('');
  let noticeDismissed = $state(false);
  let quickstartDismissed = $state(false);
  let loading = $state(true);
  let refreshing = $state(false);
  let live = $state(false);
  let range = $state<RangeKey>('24H');
  let updatedAt = $state<Date | null>(null);
  let host = $state<string>('127.0.0.1');
  let port = $state<number>(8686);
  let generation = 0;
  let liveTimer: ReturnType<typeof setInterval> | undefined;
  let keyDialog = $state<HTMLDialogElement>();
  let keyName = $state('');
  let keyModels = $state('all');
  let keySecret = $state('');
  let keyWarning = $state('');
  let keyBusy = $state(false);
  let keyModelOptions = $state<string[]>([]);
  const endpoint = $derived.by(() => {
    const raw = formatGatewayEndpoint(host, port);
    return /^https?:\/\//i.test(raw) ? raw : `http://${raw}`;
  });
  const apiRange = $derived(RANGES.find((r) => r.key === range)?.apiRange ?? '1d');
  const rangeLabel = $derived(range === 'CUSTOM' ? tr('Custom') : range);
  const rangeSubtitle = $derived(tr('Total requests · {range}', { range: rangeLabel }));
  const successRate = $derived(stats ? Math.round(stats.success_rate * 10) / 10 : null);
  const health = $derived(providers.length ? providerHealth(providers, quotas) : null);
  const combosRollup = $derived(stats ? comboTraffic(stats, combos) : []);
  const modelsRollup = $derived(stats ? modelTraffic(stats) : []);
  const latencyWarning = $derived(Boolean(stats && stats.average_latency_ms > LATENCY_TARGET_MS));
  const partial = $derived(Boolean(stats && (!stats.complete || !stats.aggregation_current || stats.history_truncated || stats.status_history_incomplete || stats.logged_requests < stats.total_requests)));
  const noticeMessage = $derived.by(() => {
    if (!stats) return '';
    if (!stats.complete) return tr('{count} telemetry events were dropped. Displayed totals and percentages use the events that were saved.', { count: formatCount(stats.dropped_events, locale) });
    if (!stats.aggregation_current) return tr('The background worker is expiring older minute buckets. Values can temporarily include older traffic.');
    return tr('Live telemetry is warming up or some events were dropped, so totals may be incomplete.');
  });
  function splitCompact(value: string): [string, string | null] {
    const m = /^(\d*\.?\d+)([KMBT])$/.exec(value);
    return m ? [m[1], m[2]] : [value, null];
  }
  async function load(isRefresh = false): Promise<void> {
    const requestGeneration = ++generation;
    if (isRefresh) refreshing = true; else loading = true;
    errorMessage = '';
    onConnectionChange('loading');
    void Promise.allSettled([
      proxyApi.proxies().then((rows) => { if (requestGeneration === generation) { proxyCount = rows.length; proxiesEnabled = rows.filter((p) => p.enabled).length; } }),
      api.providers().then(async (rows) => {
        if (requestGeneration !== generation) return;
        providers = rows;
        const quotaTargets = rows.filter((p) => p.enabled).slice(0, 12);
        const results = await Promise.allSettled(quotaTargets.map((p) => api.providerQuota(p.id)));
        if (requestGeneration === generation) {
          // Key by the id we requested, not by position in the unfiltered list:
          // disabled providers and the >12 cut-off must never inherit a quota.
          const byId = new Map<string, ProviderQuotaResult>();
          results.forEach((result, index) => {
            if (result.status === 'fulfilled') byId.set(quotaTargets[index].id, result.value);
          });
          quotas = byId;
        }
      }),
      api.combos().then((rows) => { if (requestGeneration === generation) combos = rows; }),
    ]);
    try {
      const [summary, statistics, page, settings] = await Promise.all([api.overview(), api.statistics(apiRange), api.requests(), api.settings()]);
      if (requestGeneration !== generation) return;
      overview = summary; stats = statistics; recent = page.requests;
      keyModelOptions = statistics.models.top.map((m) => m.label).slice(0, 12);
      host = typeof settings.host === 'string' && settings.host.trim() ? settings.host : '127.0.0.1';
      port = typeof settings.port === 'number' ? settings.port : Number(settings.port) || 8686;
      onGatewayAddressChange(settings.host, settings.port);
      onProviderCountChange(summary.provider_count);
      onConnectionChange('loaded'); updatedAt = new Date(); noticeDismissed = false;
    } catch (error) {
      if (requestGeneration !== generation) return;
      errorMessage = error instanceof Error ? error.message : tr('Something went wrong while loading this page.');
      onConnectionChange('error');
    } finally {
      if (requestGeneration === generation) { loading = false; refreshing = false; }
    }
  }
  function changeRange(next: RangeKey): void { range = next; void load(); }
  function setLive(value: boolean): void {
    live = value;
    if (liveTimer) { clearInterval(liveTimer); liveTimer = undefined; }
    if (live) liveTimer = setInterval(() => void load(true), 30000);
  }
  async function createKey(): Promise<void> {
    const name = keyName.trim(); if (!name || keyBusy) return; keyBusy = true;
    try {
      const scope = keyModels !== 'all' ? { allowed_provider_ids: [], allowed_models: [keyModels] } : undefined;
      const created = await api.createApiKey(name, scope);
      keySecret = created.key; keyWarning = created.warning || '';
      showDashboardToast(tr('API key "{name}" created', { name }));
    } catch (error) { showDashboardToast(error instanceof Error ? error.message : tr('Failed to create API key')); }
    finally { keyBusy = false; }
  }
  async function copyKey(): Promise<void> {
    try { await navigator.clipboard.writeText(keySecret); showDashboardToast(tr('Key copied to clipboard')); }
    catch { showDashboardToast(tr('Clipboard unavailable')); }
  }
  onMount(() => { void load(); });
  onDestroy(() => { generation += 1; if (liveTimer) clearInterval(liveTimer); });
</script>
<div class="page-heading">
  <div class="heading-copy"><h1>{tr('Overview')}</h1><p>{tr('A clear view of your gateway, at a glance.')}</p></div>
  <div class="heading-actions">
    <Button class="action-button primary" type="button" data-open-modal="key-dialog"><span class="button-icon"><svg class="icon"><use href="#i-plus" /></svg></span><span class="button-label">{tr('New API key')}</span></Button>
    <Button class="action-button import-button" type="button" onclick={() => onNavigate('requests')}><span class="button-icon"><svg class="icon"><use href="#i-import" /></svg></span><span class="button-label">{tr('View requests')}</span></Button>
  </div>
</div>
<OverviewOpsBar {tr} {locale} {endpoint} {range} {live} loading={loading || refreshing} {updatedAt} onRangeChange={changeRange} onRefresh={() => void load(true)} onLiveChange={setLive} />
{#if errorMessage}
  <section class="course-card surface" aria-label={tr('Load error')}><div class="card-title-row"><h2>{tr('Could not load overview')}</h2></div><p class="card-subtitle">{errorMessage}</p><div class="modal-actions" style="padding-top:14px"><Button class="page-action primary" type="button" onclick={() => void load()}>{tr('Retry')}</Button></div></section>
{:else if loading || !overview || !stats}
  <section class="overview-kpis" aria-label={tr('Loading')}>{#each Array(8) as _, i (i)}<StatsCard skeleton />{/each}</section>
{:else}
  {#if partial && !noticeDismissed}<OverviewNotice {tr} message={noticeMessage} onRefresh={() => void load(true)} onDismiss={() => (noticeDismissed = true)} />{/if}
  {#if (providers.length === 0 || stats.total_requests === 0) && !quickstartDismissed}
    <OverviewQuickstart {tr} providers={overview.provider_count} combos={overview.combo_count} requests={stats.total_requests} {endpoint} {onNavigate} onDismiss={() => (quickstartDismissed = true)} />
  {/if}
  <section class="overview-kpis core-kpis" aria-label={tr('Gateway overview')}>
    <article class="core-kpi">
      <h2>{tr('Requests')}</h2>
      <strong class="numeric">{formatCount(stats.total_requests, locale)}</strong>
      <span class="core-sub">{rangeSubtitle}</span>
    </article>
    <article class="core-kpi">
      <h2>{tr('Success Rate')}</h2>
      <strong class="numeric" class:good={successRate != null && successRate >= 95} class:bad={successRate != null && successRate < 95}>{successRate === null ? '—' : successRate + '%'}</strong>
      <span class="core-sub">{formatCount(stats.failures, locale)} {tr('Failed requests')}</span>
    </article>
    <article class="core-kpi">
      <h2>{tr('Token Volume & Cache')}</h2>
      <strong class="numeric">{formatCompactTokenTotal(stats.input_tokens + stats.output_tokens + stats.cached_tokens, locale)}</strong>
      <span class="core-sub">{tr('Input / Output / Cached')} · {Math.round(stats.cache_ratio * 10) / 10}% {tr('Cache hit rate')}</span>
    </article>
    <article class="core-kpi" class:warning={latencyWarning}>
      <h2>{tr('Gateway latency')}</h2>
      <strong class="numeric">{formatCount(Math.round(stats.average_latency_ms), locale)} <small>ms</small></strong>
      <span class="core-sub">{tr('{count} active providers', { count: overview.provider_count })} · {overview.combo_count} {tr('Combos')} · {proxyCount} {tr('Proxies')}</span>
    </article>
  </section>
  <OverviewHealth {tr} {health} combos={combosRollup} models={modelsRollup} {onNavigate} />
  <div class="overview-chart-stack"><OverviewTrendChart {tr} {locale} {stats} /><OverviewCharts {tr} {locale} {stats} {health} /></div>
  <div class="overview-panels"><OverviewRequests {tr} {locale} requests={recent} totalRequests={stats.total_requests} {onNavigate} /><div class="overview-chart-placeholder" aria-hidden="true"></div></div>
{/if}
<dialog class="modal" id="key-dialog" bind:this={keyDialog} aria-labelledby="key-dialog-title" onclose={() => { keyName = ''; keyModels = 'all'; keySecret = ''; keyWarning = ''; }}>
  <form class="modal-card" onsubmit={(e) => { e.preventDefault(); if (keySecret) keyDialog?.close(); else void createKey(); }}>
    <div class="modal-heading"><div><h2 id="key-dialog-title">{tr('New API key')}</h2><p>{tr('Create a client key for the gateway. The secret is shown once.')}</p></div><Button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</Button></div>
    {#if keySecret}
      <div class="grid gap-[8px] rounded-[12px] border border-db-accent-line bg-db-accent-tint px-[15px] py-[13px]"><strong class="text-accent-deep text-label">{tr('Your new key')}</strong><p class="m-0 text-muted text-meta">{tr('Copy it now — it will not be shown again.')}{keyWarning ? ' ' + keyWarning : ''}</p><div class="mt-[4px] flex items-center gap-[10px]"><code class="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap rounded-[9px] border border-db-accent-line bg-surface px-[12px] py-[9px] text-ink text-meta">{keySecret}</code><Button class="page-action" type="button" onclick={copyKey}><svg class="icon w-[14px] h-[14px]"><use href="#i-copy" /></svg> {tr('Copy')}</Button></div></div>
      <div class="modal-actions"><Button class="page-action primary" type="submit">{tr('Done')}</Button></div>
    {:else}
      <div class="modal-fields">
        <label class="field">{tr('Key name')}<Input bind:value={keyName} type="text" required placeholder={tr('e.g. mobile-app')} /></label>
        <label class="field">{tr('Allowed model')}<SelectOptions ariaLabel={tr('Allowed model')} value={keyModels} items={[{ label: tr('All models'), value: 'all' }, ...keyModelOptions.map((model) => ({ label: model, value: model }))]} onValueChange={(value) => (keyModels = value)} /></label>
        <p class="form-note">{tr('Budget and expiry limits are managed on the API keys page.')}</p>
      </div>
      <div class="modal-actions"><Button class="page-action" type="button" data-close-modal>{tr('Cancel')}</Button><Button class="page-action primary" type="submit" disabled={keyBusy || !keyName.trim()}>{keyBusy ? tr('Creating…') : tr('Create key')}</Button></div>
    {/if}
  </form>
</dialog>
