<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import type { DashboardPage } from '../../lib/navigation';
  import type { Locale } from '../../lib/i18n';
  import type { Translate } from '../../lib/format';
  import { formatCount, formatCostMicroUsd, formatPercent, formatTokenCount } from '../../lib/format';
  import { showSketchToast } from '../../lib/sketch';
  import type { Overview, RequestLog, RequestStatistics } from '../../lib/types';
  import '../../styles/sketch/pages/overview.css';

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
  let recent = $state<RequestLog[]>([]);
  let activeKeys = $state(0);
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

  async function load(): Promise<void> {
    const requestGeneration = ++generation;
    loading = true;
    errorMessage = '';
    onConnectionChange('loading');
    try {
      const [summary, statistics, requestPage, keyPage, settings] = await Promise.all([
        api.overview(),
        api.statistics('1d'),
        api.requests(),
        api.apiKeys(),
        api.settings(),
      ]);
      if (requestGeneration !== generation) return;
      overview = summary;
      stats = statistics;
      recent = requestPage.requests.slice(0, 6);
      activeKeys = keyPage.api_keys.filter((key) => key.enabled).length;
      keyModelOptions = statistics.models.top.map((m) => m.label).slice(0, 12);
      onGatewayAddressChange(settings.host, settings.port);
      onProviderCountChange(summary.provider_count);
      onConnectionChange('loaded');
    } catch (error) {
      if (requestGeneration !== generation) return;
      errorMessage = error instanceof Error ? error.message : 'Something went wrong while loading this page.';
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
      activeKeys += 1;
      showSketchToast(`API key "${name}" created`);
    } catch (error) {
      showSketchToast(error instanceof Error ? error.message : 'Failed to create API key');
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
    const labels: Record<string, string> = { '2xx': '2xx success', '4xx': '4xx client error', '5xx': '5xx server error', 'Other': 'Other' };
    const label = bar.dataset.barLabel ?? '';
    const pct = bar.dataset.barPct ?? '0';
    const count = Number(bar.dataset.barCount ?? 0);
    const rect = chartEl.getBoundingClientRect();
    chartTooltip = {
      text: `${labels[label] ?? label} — ${pct}% (${formatCount(count, locale)} requests)`,
      left: Math.min(event.clientX - rect.left + 12, rect.width - 190),
      top: event.clientY - rect.top - 14,
    };
  }

  async function copyKey() {
    try {
      await navigator.clipboard.writeText(keySecret);
      showSketchToast('Key copied to clipboard');
    } catch {
      showSketchToast('Clipboard unavailable');
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
    <h1>Overview</h1>
    <p>A clear view of your gateway, at a glance.</p>
  </div>
  <div class="heading-actions">
    <button class="action-button primary" type="button" data-open-modal="key-dialog">
      <span class="button-icon"><svg class="icon"><use href="#i-plus" /></svg></span>
      <span class="button-label">New API key</span>
    </button>
    <button class="action-button import-button" type="button" onclick={() => onNavigate('requests')}>
      <span class="button-icon"><svg class="icon"><use href="#i-import" /></svg></span>
      <span class="button-label">View requests</span>
    </button>
  </div>
</div>

{#if errorMessage}
  <section class="course-card surface" aria-label="Load error">
    <div class="card-title-row"><h2>Could not load overview</h2></div>
    <p class="card-subtitle">{errorMessage}</p>
    <div class="modal-actions" style="padding-top:14px">
      <button class="page-action primary" type="button" onclick={load}>Retry</button>
    </div>
  </section>
{:else if loading || !overview || !stats}
  <section class="stats-grid" aria-label="Loading">
    {#each [0, 1, 2, 3] as i (i)}
      <article class="stat-card surface" style="min-height:132px"></article>
    {/each}
  </section>
{:else}
  <div class="dashboard-content">
    <section class="stats-grid" aria-label="Gateway overview">
      <article class="stat-card surface">
        <div class="stat-top">
          <span class="stat-icon"><svg class="icon"><use href="#i-clipboard" /></svg></span>
          <span class="stat-number">{fmtInt(overview.request_count)}</span>
        </div>
        <h2>Total Requests</h2>
        <div class="stat-trend">All time on this instance</div>
        <button class="details-link" type="button" onclick={() => onNavigate('requests')}>View Requests <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>

      <article class="stat-card surface">
        <div class="stat-top">
          <span class="stat-icon"><svg class="icon"><use href="#i-link" /></svg></span>
          <span class="stat-number">{overview.provider_count}</span>
        </div>
        <h2>Providers</h2>
        <div class="stat-trend">Configured upstreams</div>
        <button class="details-link" type="button" onclick={() => onNavigate('providers')}>View Providers <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>

      <article class="stat-card surface">
        <div class="stat-top">
          <span class="stat-icon"><svg class="icon"><use href="#i-code" /></svg></span>
          <span class="stat-number">{overview.combo_count}</span>
        </div>
        <h2>Combos</h2>
        <div class="stat-trend">Routing rules in place</div>
        <button class="details-link" type="button" onclick={() => onNavigate('combos')}>View Combos <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>

      <article class="stat-card surface">
        <div class="stat-top">
          <span class="stat-icon"><svg class="icon"><use href="#i-trend-up" /></svg></span>
          <span class="stat-number">{successRate === null ? '—' : `${successRate}%`}</span>
        </div>
        <h2>Success Rate</h2>
        <div class="stat-trend">Last 24 hours</div>
        <button class="details-link" type="button" onclick={() => onNavigate('requests')}>View Requests <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
    </section>

    <div class="middle-grid">
      <section class="chart-card surface" aria-labelledby="status-heading">
        <div class="card-title-row">
          <h2 id="status-heading">Requests by Status</h2>
        </div>
        <p class="card-subtitle">HTTP status distribution, last 24 hours</p>
        <div class="chart-plot" role="img" aria-label="Request distribution by HTTP status" bind:this={chartEl} onmousemove={onChartMove} onmouseleave={() => (chartTooltip = null)}>
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
          <h2 id="recent-heading">Recent Requests</h2>
          <button class="mini-add" type="button" onclick={() => onNavigate('requests')}>View all</button>
        </div>
        <div class="course-list" aria-live="polite">
          {#if recent.length === 0}
            <p class="card-subtitle" style="padding:18px 4px">No requests logged yet.</p>
          {:else}
            {#each recent as request (request.id ?? request.request_id ?? request.created_at)}
              <article class="course-row">
                <span class="course-icon"><svg class="icon"><use href={statusClass(request) === 'complete' ? '#i-play' : statusClass(request) === 'progress' ? '#i-zap' : '#i-x-circle'} /></svg></span>
                <div class="course-copy">
                  <strong>{request.model || request.route_alias || 'request'}</strong>
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
        <h2 id="success-heading">Success Overview</h2>
        <p class="card-subtitle">Request outcomes and latency, last 24 hours</p>
        <div class="assignment-content">
          <div class="legend-list">
            <div class="legend-row">
              <span class="legend-dot orange"></span><strong>{fmtInt(stats.failures)}</strong><span>Failed Requests</span>
            </div>
            <div class="legend-row">
              <span class="legend-dot ink"></span><strong>{fmtInt(stats.successes)}</strong><span>Successful Requests</span>
            </div>
            <div class="legend-row">
              <span class="legend-dot green"></span><strong>{fmtCompactTokens(totalTokens)}</strong><span>Total Tokens</span>
            </div>
            <div class="legend-row">
              <span class="legend-dot blue"></span><strong>{stats.average_latency_ms ? `${Math.round(stats.average_latency_ms)} ms` : '—'}</strong><span>Average Latency</span>
            </div>
          </div>
          <svg class="gauge" viewBox="0 0 280 155" role="img" aria-label="{successRate ?? 0} percent of requests successful">
            <defs>
              <linearGradient id="gauge-gradient" x1="0" y1="1" x2="1" y2="0">
                <stop offset="0%" stop-color="#f27b5e" />
                <stop offset="22%" stop-color="#313943" />
                <stop offset="48%" stop-color="#252d35" />
                <stop offset="72%" stop-color="#10c83b" />
                <stop offset="100%" stop-color="#9ed1ff" />
              </linearGradient>
            </defs>
            <path d="M28 142 A112 112 0 0 1 252 142" fill="none" stroke="#edf1f3" stroke-width="17" />
            <path
              d="M28 142 A112 112 0 0 1 252 142"
              fill="none"
              pathLength="100"
              stroke="url(#gauge-gradient)"
              stroke-dasharray={gaugeDash}
              stroke-width="17"
            />
            <text x="140" y="116" text-anchor="middle" fill="#293341" font-size="31" font-weight="700">{successRate ?? '—'}<tspan font-size="14">%</tspan></text>
            <text x="140" y="138" text-anchor="middle" fill="#78838c" font-size="9">Request Success</text>
          </svg>
        </div>
      </section>

      <section class="community-card surface" aria-labelledby="links-heading">
        <div class="community-heading">
          <h2 id="links-heading">Quick Links</h2>
          <button class="icon-button" type="button" aria-label="More links" onclick={() => showSketchToast('Quick links')}>
            <svg class="icon"><use href="#i-plus" /></svg>
          </button>
        </div>
        <div class="community-link" role="button" tabindex="0" onclick={openDocs} onkeydown={(e) => e.key === 'Enter' && openDocs()}>
          <span class="community-logo" aria-hidden="true"><svg class="icon"><use href="#i-book" /></svg></span>
          <span class="community-copy">
            <strong>Documentation</strong>
            <span>API reference &amp; guides</span>
          </span>
        </div>
        <div class="community-link" role="button" tabindex="0" onclick={() => onNavigate('chat')} onkeydown={(e) => e.key === 'Enter' && onNavigate('chat')}>
          <span class="community-logo discord" aria-hidden="true"><svg class="icon"><use href="#i-video" /></svg></span>
          <span class="community-copy">
            <strong>Workspace Chat</strong>
            <span>Try models in-browser</span>
          </span>
        </div>
        <div class="community-link" role="button" tabindex="0" onclick={() => onNavigate('api-keys')} onkeydown={(e) => e.key === 'Enter' && onNavigate('api-keys')}>
          <span class="community-logo" aria-hidden="true" style="color:#ed674d"><svg class="icon"><use href="#i-key" /></svg></span>
          <span class="community-copy">
            <strong>API Keys</strong>
            <span>{activeKeys} active {activeKeys === 1 ? 'key' : 'keys'}</span>
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
        <h2 id="key-dialog-title">New API key</h2>
        <p>Create a client key for the gateway. The secret is shown once.</p>
      </div>
      <button class="modal-close" type="button" data-close-modal aria-label="Close dialog">×</button>
    </div>
    {#if keySecret}
      <div class="key-reveal">
        <strong>Your new key</strong>
        <p>Copy it now — it will not be shown again.{#if keyWarning} {keyWarning}{/if}</p>
        <div class="key-reveal-row">
          <code>{keySecret}</code>
          <button class="page-action" type="button" onclick={copyKey}>
            <svg class="icon"><use href="#i-copy" /></svg> Copy
          </button>
        </div>
      </div>
      <div class="modal-actions">
        <button class="page-action primary" type="submit">Done</button>
      </div>
    {:else}
      <div class="modal-fields">
        <label class="field">
          Key name
          <input bind:value={keyName} type="text" required placeholder="e.g. mobile-app" />
        </label>
        <label class="field">
          Allowed model
          <select bind:value={keyModels}>
            <option value="all">All models</option>
            {#each keyModelOptions as model (model)}
              <option value={model}>{model}</option>
            {/each}
          </select>
        </label>
        <p class="form-note">Budget and expiry limits are managed on the API keys page.</p>
      </div>
      <div class="modal-actions">
        <button class="page-action" type="button" data-close-modal>Cancel</button>
        <button class="page-action primary" type="submit" disabled={keyBusy || !keyName.trim()}>{keyBusy ? 'Creating…' : 'Create key'}</button>
      </div>
    {/if}
  </form>
</dialog>
