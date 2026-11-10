<script lang="ts">
  import Button from "../../components/Button.svelte";
  import StatsCard from "../../components/StatsCard.svelte";
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import Input from '../../components/Input.svelte';
  import type { DashboardPage } from '../../lib/navigation';
  import type { Locale } from '../../lib/i18n';
  import type { Translate } from '../../lib/format';
  import { formatCount, formatCostMicroUsd, formatCompactTokenTotal, formatPercent, formatTokenCount } from '../../lib/format';
  import { proxyApi } from '../../lib/api/proxies';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import SelectOptions from '../../components/SelectOptions.svelte';
  import type { Overview, RequestLog, RequestStatistics } from '../../lib/types';
  import '../../styles/dashboard/pages/overview.css';

  interface Props {
    tr: Translate;
    locale: Locale;
    onNavigate: (page: DashboardPage) => void;
    onCreateRequest: (page: DashboardPage) => void;
    onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;
    onProviderCountChange: (count: number) => void;
    onGatewayAddressChange: (host: unknown, port: unknown) => void;
  }

  let {
    tr,
    locale,
    onNavigate,
    onCreateRequest: _onCreateRequest,
    onConnectionChange,
    onProviderCountChange,
    onGatewayAddressChange,
  }: Props = $props();

  let overview = $state<Overview | null>(null);
  let stats = $state<RequestStatistics | null>(null);
  let statisticsError = $state(false);
  let recent = $state<RequestLog[]>([]);
  let proxySummary = $state<{ total: number; enabled: number; endpoints: number } | null>(null);
  let loading = $state(true);
  let errorMessage = $state('');
  let generation = 0;

  // New API key dialog state
  let keyDialog = $state<HTMLDialogElement>();
  let keyName = $state('');
  let keyModels = $state('all');
  let keySecret = $state('');
  let keyWarning = $state('');
  let keyBusy = $state(false);
  let keyModelOptions = $state<string[]>([]);
  let chartTooltip = $state<{ text: string; left: number; top: number } | null>(null);
  let chartEl = $state<HTMLElement>();

  const statusGroups = $derived.by(() => {
    const groups = { ok: 0, client: 0, server: 0, other: 0 };
    if (!stats) return groups;
    for (const row of stats.statuses.rows) {
      const status = row.status || 0;
      if (status >= 200 && status < 300) groups.ok += row.requests;
      else if (status >= 400 && status < 500) groups.client += row.requests;
      else if (status >= 500 && status < 600) groups.server += row.requests;
      else groups.other += row.requests;
    }
    return groups;
  });

  const statusTotal = $derived(
    statusGroups.ok + statusGroups.client + statusGroups.server + statusGroups.other || stats?.total_requests || 0,
  );

  const statusBars = $derived.by(() => {
    const total = statusTotal || 1;
    const pct = (n: number) => Math.round((n / total) * 100);
    return [
      { label: '2xx', value: pct(statusGroups.ok), count: statusGroups.ok, cls: '' },
      { label: '4xx', value: pct(statusGroups.client), count: statusGroups.client, cls: '' },
      { label: '5xx', value: pct(statusGroups.server), count: statusGroups.server, cls: '' },
      { label: 'Other', value: pct(statusGroups.other), count: statusGroups.other, cls: '' },
    ];
  });

  const successRate = $derived(stats ? Math.round(stats.success_rate * 10) / 10 : null);
  const gaugeDash = $derived(`${Math.max(0, Math.min(100, successRate ?? 0))} 100`);
  const totalTokens = $derived(stats ? stats.input_tokens + stats.output_tokens : 0);

  function statusClass(request: RequestLog): string {
    const status = request.status ?? 0;
    if (status >= 200 && status < 300) return 'complete';
    if (status >= 400 && status < 500) return 'progress';
    return 'error';
  }

  function statusLabel(request: RequestLog): string {
    return request.status ? String(request.status) : '…';
  }

  function providerLabel(request: RequestLog): string {
    return request.provider_id || request.client_protocol || 'gateway';
  }

  function latencyLabel(request: RequestLog): string {
    const ms = request.duration_ms;
    if (ms == null) return '';
    return ms >= 1000 ? `${(ms / 1000).toFixed(1)} s` : `${Math.round(ms)} ms`;
  }

  function fmtInt(value: number | null | undefined): string {
    return formatCount(value ?? 0, locale);
  }

  function fmtCompactTokens(value: number): string {
    if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
    if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
    return String(value);
  }

  async function loadProxies(): Promise<void> {
    const requestGeneration = generation;
    try {
      const rows = await proxyApi.proxies();
      if (requestGeneration !== generation) return;
      proxySummary = {
        total: rows.length,
        enabled: rows.filter((proxy) => proxy.enabled).length,
        endpoints: rows.reduce((sum, proxy) => sum + (proxy.endpoints?.length ?? 0), 0),
      };
    } catch {
      // The overview stays useful without proxy counts; the card shows em dashes.
      if (requestGeneration === generation) proxySummary = null;
    }
  }

  async function load(): Promise<void> {
    const requestGeneration = ++generation;
    loading = true;
    errorMessage = '';
    onConnectionChange('loading');
    void loadProxies();
    try {
      const [summary, statistics, requestPage, settings] = await Promise.all([
        api.overview(),
        api.statistics('1d'),
        api.requests(),
        api.settings(),
      ]);
      if (requestGeneration !== generation) return;
      overview = summary;
      stats = statistics;
      statisticsError = false;
      recent = requestPage.requests.slice(0, 6);
      keyModelOptions = statistics.models.top.map((m) => m.label).slice(0, 12);
      onGatewayAddressChange(settings.host, settings.port);
      onProviderCountChange(summary.provider_count);
      onConnectionChange('loaded');
    } catch (error) {
      if (requestGeneration !== generation) return;
      statisticsError = true;
      errorMessage = error instanceof Error ? error.message : tr('Something went wrong while loading this page.');
      onConnectionChange('error');
    } finally {
      if (requestGeneration === generation) loading = false;
    }
  }

  async function createKey(): Promise<void> {
    const name = keyName.trim();
    if (!name || keyBusy) return;
    keyBusy = true;
    try {
      const scope = keyModels !== 'all' ? { allowed_provider_ids: [], allowed_models: [keyModels] } : undefined;
      const created = await api.createApiKey(name, scope);
      keySecret = created.key;
      keyWarning = created.warning || '';
      showDashboardToast(tr('API key "{name}" created', { name }));
    } catch (error) {
      showDashboardToast(error instanceof Error ? error.message : tr('Failed to create API key'));
    } finally {
      keyBusy = false;
    }
  }

  function closeKeyDialog() {
    keyDialog?.close();
  }

  function resetKeyDialog() {
    keyName = '';
    keyModels = 'all';
    keySecret = '';
    keyWarning = '';
  }

  function onChartMove(event: MouseEvent) {
    const bar = (event.target as HTMLElement).closest('.bar') as HTMLElement | null;
    if (!bar || !chartEl) { chartTooltip = null; return; }
    const labels: Record<string, string> = { '2xx': tr('2xx success'), '4xx': tr('4xx client error'), '5xx': tr('5xx server error'), 'Other': tr('Other') };
    const label = bar.dataset.barLabel ?? '';
    const pct = bar.dataset.barPct ?? '0';
    const count = Number(bar.dataset.barCount ?? 0);
    const rect = chartEl.getBoundingClientRect();
    chartTooltip = {
      text: tr('{label} — {pct}% ({count} requests)', { label: labels[label] ?? label, pct, count: formatCount(count, locale) }),
      left: Math.min(event.clientX - rect.left + 12, rect.width - 190),
      top: event.clientY - rect.top - 14,
    };
  }

  async function copyKey() {
    try {
      await navigator.clipboard.writeText(keySecret);
      showDashboardToast(tr('Key copied to clipboard'));
    } catch {
      showDashboardToast(tr('Clipboard unavailable'));
    }
  }

  function openDocs() {
    window.location.assign('/docs');
  }

  onMount(() => { void load(); });
  onDestroy(() => { generation += 1; });
</script>

<div class="page-heading">
  <div class="heading-copy">
    <h1>{tr('Overview')}</h1>
    <p>{tr('A clear view of your gateway, at a glance.')}</p>
  </div>
  <div class="heading-actions">
    <Button class="action-button primary" type="button" data-open-modal="key-dialog">
      <span class="button-icon"><svg class="icon"><use href="#i-plus" /></svg></span>
      <span class="button-label">{tr('New API key')}</span>
    </Button>
    <Button class="action-button import-button" type="button" onclick={() => onNavigate('requests')}>
      <span class="button-icon"><svg class="icon"><use href="#i-import" /></svg></span>
      <span class="button-label">{tr('View requests')}</span>
    </Button>
  </div>
</div>

{#if errorMessage}
  <section class="course-card surface" aria-label={tr('Load error')}>
    <div class="card-title-row"><h2>{tr('Could not load overview')}</h2></div>
    <p class="card-subtitle">{errorMessage}</p>
    <div class="modal-actions" style="padding-top:14px">
      <Button class="page-action primary" type="button" onclick={load}>{tr('Retry')}</Button>
    </div>
  </section>
{:else if loading || !overview || !stats}
  <section class="stats-grid" aria-label={tr('Loading')}>
    {#each [0, 1, 2, 3] as i (i)}
      <StatsCard skeleton />
    {/each}
  </section>
{:else}
  {#if statisticsError || !stats.complete || !stats.aggregation_current || stats.history_truncated || stats.status_history_incomplete || stats.logged_requests < stats.total_requests}
    <div class="overview-statistics-warning" role="status">
      <strong>{tr('Statistics are incomplete')}</strong>
      <span>
        {#if statisticsError}
          {tr('Statistics are incomplete')}
        {:else if !stats.complete}
          {tr('{count} telemetry events were dropped. Displayed totals and percentages use the events that were saved.', { count: formatCount(stats.dropped_events, locale) })}
        {:else if !stats.aggregation_current}
          {tr('The background worker is expiring older minute buckets. Values can temporarily include older traffic.')}
        {:else}
          {tr('Statistics are incomplete')}
        {/if}
      </span>
    </div>
  {/if}
  <div class="dashboard-content">
    <section class="stats-grid" aria-label={tr('Gateway overview')}>
      <StatsCard icon="#i-clipboard" value={statisticsError ? '—' : fmtInt(stats.total_requests)} title={tr('Requests Today')} trend={tr('Last 24 hours')} actionLabel={tr('View Requests')} onAction={ () => onNavigate('requests') } />
      <StatsCard icon="#i-link" value={overview.provider_count} title={tr('Providers')} trend={tr('Configured upstreams')} actionLabel={tr('View Providers')} onAction={ () => onNavigate('providers') } />
      <StatsCard icon="#i-code" value={overview.combo_count} title={tr('Combos')} trend={tr('Routing rules in place')} actionLabel={tr('View Combos')} onAction={ () => onNavigate('combos') } />
      <StatsCard icon="#i-trend-up" value={successRate === null ? '—' : `${successRate}%`} title={tr('Success Rate')} trend={tr('Last 24 hours')} actionLabel={tr('View Requests')} onAction={ () => onNavigate('requests') } />
    </section>

    <section class="stats-grid metric-grid" aria-label={tr('Token usage')}>
      <StatsCard icon="#i-import" value={statisticsError ? '—' : formatCompactTokenTotal(stats.input_tokens, locale)} title={tr('Input tokens')} trend={tr('Last 24 hours')} actionLabel={tr('View Requests')} onAction={ () => onNavigate('requests') } />
      <StatsCard icon="#i-export" value={statisticsError ? '—' : formatCompactTokenTotal(stats.output_tokens, locale)} title={tr('Output tokens')} trend={tr('Last 24 hours')} actionLabel={tr('View Requests')} onAction={ () => onNavigate('requests') } />
      <StatsCard icon="#i-database" value={statisticsError ? '—' : formatCompactTokenTotal(stats.cached_tokens, locale)} title={tr('Cached tokens')} trend={`${tr('Cache ratio')} ${statisticsError ? '—' : `${Math.round(stats.cache_ratio * 10) / 10}%`}`} actionLabel={tr('View Requests')} onAction={ () => onNavigate('requests') } />
      <StatsCard icon="#i-clock" value={statisticsError || !stats.average_latency_ms ? '—' : `${Math.round(stats.average_latency_ms)} ms`} title={tr('Avg Latency')} trend={tr('p50 {value}', { value: statisticsError || !stats.latency.p50_ms ? '—' : `${Math.round(stats.latency.p50_ms)} ms` })} actionLabel={tr('View Requests')} onAction={ () => onNavigate('requests') } />
      <StatsCard icon="#i-link" value={proxySummary ? formatCount(proxySummary.total, locale) : '—'} title={tr('Proxies')} trend={proxySummary ? tr('{enabled} enabled · {endpoints} endpoints', { enabled: proxySummary.enabled, endpoints: proxySummary.endpoints }) : '—'} actionLabel={tr('View Proxies')} onAction={ () => onNavigate('proxies') } />
    </section>

    <div class="middle-grid">
      <section class="chart-card surface" aria-labelledby="status-heading">
        <div class="card-title-row">
          <h2 id="status-heading">{tr('Requests by Status')}</h2>
        </div>
        <p class="card-subtitle">{tr('HTTP status distribution, last 24 hours')}</p>
        <div class="chart-plot" role="img" aria-label={tr('Request distribution by HTTP status')} bind:this={chartEl} onmousemove={onChartMove} onmouseleave={() => (chartTooltip = null)}>
          {#if chartTooltip}
            <div class="chart-tooltip" style="left:{chartTooltip.left}px;top:{chartTooltip.top}px">{chartTooltip.text}</div>
          {/if}
          <div class="y-axis" aria-hidden="true">
            <span>100%</span><span>75%</span><span>50%</span><span>25%</span><span>0%</span>
          </div>
          <div class="bar-chart" aria-hidden="true">
            {#each statusBars as bar, i (bar.label)}
              <div class="bar" data-bar-index={i} data-bar-label={bar.label} data-bar-pct={bar.value} data-bar-count={bar.count}>
                <strong class="bar-value">{bar.value}%</strong>
                <span style="--bar: {bar.value}%"></span>
              </div>
            {/each}
          </div>
          <div class="bar-labels" aria-hidden="true">
            {#each statusBars as bar (bar.label)}<span>{bar.label}</span>{/each}
          </div>
        </div>
      </section>

      <section class="course-card surface" aria-labelledby="recent-heading">
        <div class="card-title-row">
          <h2 id="recent-heading">{tr('Recent Requests')}</h2>
          <Button class="mini-add" type="button" onclick={() => onNavigate('requests')}>{tr('View all')}</Button>
        </div>
        <div class="course-list" aria-live="polite">
          {#if recent.length === 0}
            <p class="card-subtitle" style="padding:18px 4px">{tr('No requests logged yet.')}</p>
          {:else}
            {#each recent as request (request.id ?? request.request_id ?? request.created_at)}
              <article class="course-row">
                <span class="course-icon"><svg class="icon"><use href={statusClass(request) === 'complete' ? '#i-play' : statusClass(request) === 'progress' ? '#i-zap' : '#i-x-circle'} /></svg></span>
                <div class="course-copy">
                  <strong>{request.model || request.route_alias || tr('request')}</strong>
                  <span>{providerLabel(request)}{latencyLabel(request) ? ` · ${latencyLabel(request)}` : ''}</span>
                </div>
                <span class="status {statusClass(request)}">{statusLabel(request)}</span>
              </article>
            {/each}
          {/if}
        </div>
      </section>
    </div>

    <div class="bottom-grid">
      <section class="assignment-card surface" aria-labelledby="success-heading">
        <h2 id="success-heading">{tr('Success Overview')}</h2>
        <p class="card-subtitle">{tr('Request outcomes and latency, last 24 hours')}</p>
        <div class="assignment-content">
          <div class="legend-list">
            <div class="legend-row">
              <span class="legend-dot orange"></span><strong>{fmtInt(stats.failures)}</strong><span>{tr('Failed Requests')}</span>
            </div>
            <div class="legend-row">
              <span class="legend-dot ink"></span><strong>{fmtInt(stats.successes)}</strong><span>{tr('Successful Requests')}</span>
            </div>
            <div class="legend-row">
              <span class="legend-dot green"></span><strong>{fmtCompactTokens(totalTokens)}</strong><span>{tr('Total Tokens')}</span>
            </div>
            <div class="legend-row">
              <span class="legend-dot blue"></span><strong>{stats.average_latency_ms ? `${Math.round(stats.average_latency_ms)} ms` : '—'}</strong><span>{tr('Average Latency')}</span>
            </div>
          </div>
          <svg class="gauge" viewBox="0 0 280 155" role="img" aria-label={tr('{percent} percent of requests successful', { percent: successRate ?? 0 })}>
            <defs>
              <linearGradient id="gauge-gradient" x1="0" y1="1" x2="1" y2="0">
                <stop offset="0%" stop-color="#f27b5e" />
                <stop offset="22%" stop-color="#313943" />
                <stop offset="48%" stop-color="#252d35" />
                <stop offset="72%" stop-color="#10c83b" />
                <stop offset="100%" stop-color="#9ed1ff" />
              </linearGradient>
            </defs>
            <path d="M28 142 A112 112 0 0 1 252 142" fill="none" style="stroke: var(--line)" stroke-width="17" />
            <path
              d="M28 142 A112 112 0 0 1 252 142"
              fill="none"
              pathLength="100"
              stroke="url(#gauge-gradient)"
              stroke-dasharray={gaugeDash}
              stroke-width="17"
            />
            <text x="140" y="116" text-anchor="middle" style="fill: var(--ink)" font-size="31" font-weight="700">{successRate ?? '—'}<tspan font-size="14">%</tspan></text>
            <text x="140" y="138" text-anchor="middle" style="fill: var(--muted)" font-size="9">{tr('Request Success')}</text>
          </svg>
        </div>
      </section>

      <section class="community-card surface" aria-labelledby="links-heading">
        <div class="community-heading">
          <h2 id="links-heading">{tr('Quick Links')}</h2>
          <Button class="icon-button" type="button" aria-label={tr('More links')} onclick={() => showDashboardToast(tr('Quick links'))}>
            <svg class="icon"><use href="#i-plus" /></svg>
          </Button>
        </div>
        <div class="community-link" role="button" tabindex="0" onclick={openDocs} onkeydown={(e) => e.key === 'Enter' && openDocs()}>
          <span class="community-logo" aria-hidden="true"><svg class="icon"><use href="#i-book" /></svg></span>
          <span class="community-copy">
            <strong>{tr('Documentation')}</strong>
            <span>{tr('API reference & guides')}</span>
          </span>
        </div>
        <div class="community-link" role="button" tabindex="0" onclick={() => onNavigate('chat')} onkeydown={(e) => e.key === 'Enter' && onNavigate('chat')}>
          <span class="community-logo discord" aria-hidden="true"><svg class="icon"><use href="#i-video" /></svg></span>
          <span class="community-copy">
            <strong>{tr('Workspace Chat')}</strong>
            <span>{tr('Try models in-browser')}</span>
          </span>
        </div>
        <div class="community-link" role="button" tabindex="0" onclick={() => onNavigate('api-keys')} onkeydown={(e) => e.key === 'Enter' && onNavigate('api-keys')}>
          <span class="community-logo" aria-hidden="true" style="color:#ed674d"><svg class="icon"><use href="#i-key" /></svg></span>
          <span class="community-copy">
            <strong>{tr('API Keys')}</strong>
            <span>{tr('Manage client API keys')}</span>
          </span>
        </div>
      </section>
    </div>
  </div>
{/if}

<dialog class="modal" id="key-dialog" bind:this={keyDialog} aria-labelledby="key-dialog-title" onclose={resetKeyDialog}>
  <form
    class="modal-card"
    onsubmit={(e) => { e.preventDefault(); if (keySecret) closeKeyDialog(); else void createKey(); }}
  >
    <div class="modal-heading">
      <div>
        <h2 id="key-dialog-title">{tr('New API key')}</h2>
        <p>{tr('Create a client key for the gateway. The secret is shown once.')}</p>
      </div>
      <Button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</Button>
    </div>
    {#if keySecret}
      <div class="key-reveal">
        <strong>{tr('Your new key')}</strong>
        <p>{tr('Copy it now — it will not be shown again.')}{#if keyWarning} {keyWarning}{/if}</p>
        <div class="key-reveal-row">
          <code>{keySecret}</code>
          <Button class="page-action" type="button" onclick={copyKey}>
            <svg class="icon"><use href="#i-copy" /></svg> {tr('Copy')}
          </Button>
        </div>
      </div>
      <div class="modal-actions">
        <Button class="page-action primary" type="submit">{tr('Done')}</Button>
      </div>
    {:else}
      <div class="modal-fields">
        <label class="field">
          {tr('Key name')}
          <Input bind:value={keyName} type="text" required placeholder={tr('e.g. mobile-app')} />
        </label>
        <label class="field">
          {tr('Allowed model')}
          <SelectOptions
            ariaLabel={tr('Allowed model')}
            value={keyModels}
            items={[{ label: tr('All models'), value: 'all' }, ...keyModelOptions.map((model) => ({ label: model, value: model }))]}
            onValueChange={(value) => (keyModels = value)}
          />
        </label>
        <p class="form-note">{tr('Budget and expiry limits are managed on the API keys page.')}</p>
      </div>
      <div class="modal-actions">
        <Button class="page-action" type="button" data-close-modal>{tr('Cancel')}</Button>
        <Button class="page-action primary" type="submit" disabled={keyBusy || !keyName.trim()}>{keyBusy ? tr('Creating…') : tr('Create key')}</Button>
      </div>
    {/if}
  </form>
</dialog>
