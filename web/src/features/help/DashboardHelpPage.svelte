<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../../lib/api';
  import type { DashboardPage } from '../../lib/navigation';
  import { formatUptime } from '../../lib/format';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import type { Overview, RequestStatistics } from '../../lib/types';
  import '../../styles/dashboard/pages/help.css';

  interface Props {
    tr: (key: string) => string;
    onNavigate: (page: DashboardPage) => void;
  }
  let { tr, onNavigate }: Props = $props();

  let overview = $state<Overview | null>(null);
  let stats = $state<RequestStatistics | null>(null);

  onMount(() => {
    void api.overview().then((o) => (overview = o)).catch(() => {});
    void api.statistics('30d').then((s) => (stats = s)).catch(() => {});
  });

  function copyDiagnostics() {
    const info = [
      `ExoRoute dashboard`,
      `uptime: ${overview ? formatUptime(overview.uptime_seconds, tr) : 'unknown'}`,
      `providers: ${overview?.provider_count ?? '?'} combos: ${overview?.combo_count ?? '?'}`,
      stats ? `30d success rate: ${stats.success_rate.toFixed(1)}%` : '',
    ].filter(Boolean).join(' · ');
    void navigator.clipboard?.writeText(info).catch(() => {});
    showDashboardToast('Diagnostics copied — paste into your support message');
  }
</script>

<div class="page-heading">
  <div class="heading-copy">
    <h1>Help Center</h1>
    <p>Guides, troubleshooting, and system status for your ExoRoute gateway.</p>
  </div>
  <div class="heading-actions">
    <button class="action-button" type="button" onclick={() => showDashboardToast('Support: support@exoroute.local — include a request ID')}>
      <span class="button-icon"><svg class="icon"><use href="#i-mail" /></svg></span>
      <span class="button-label">Contact support</span>
    </button>
  </div>
</div>

<div class="page-surface">
  <section class="status-banner surface" aria-label="System status">
    <span class="status-pulse"></span>
    <div class="status-banner-copy">
      <strong>Gateway reachable</strong>
      <span>{overview ? `Uptime ${formatUptime(overview.uptime_seconds, tr)}` : 'Loading status…'}</span>
    </div>
    <button class="mini-add" type="button" onclick={() => location.reload()}>Refresh</button>
  </section>

  <section class="stats-grid" aria-label="Help overview">
    <article class="stat-card surface">
      <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-book" /></svg></span><span class="stat-number">{overview ? overview.provider_count + overview.combo_count : '—'}</span></div>
      <h2>Resources</h2>
      <div class="stat-trend">{overview ? `${overview.provider_count} providers · ${overview.combo_count} combos` : 'providers + combos'}</div>
      <button class="details-link" type="button" onclick={() => onNavigate('providers')}>Open providers <svg class="icon"><use href="#i-arrow-right" /></svg></button>
    </article>
    <article class="stat-card surface">
      <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-help" /></svg></span><span class="stat-number">{overview?.dropped_request_logs ?? '—'}</span></div>
      <h2>Dropped Logs</h2>
      <div class="stat-trend">Request logs dropped by retention</div>
      <button class="details-link" type="button" onclick={() => onNavigate('requests')}>View requests <svg class="icon"><use href="#i-arrow-right" /></svg></button>
    </article>
    <article class="stat-card surface">
      <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-server" /></svg></span><span class="stat-number">{stats ? `${stats.success_rate.toFixed(1)}%` : '—'}</span></div>
      <h2>30d Success</h2>
      <div class="stat-trend">{stats ? `${stats.total_requests.toLocaleString()} requests` : 'request success rate'}</div>
      <button class="details-link" type="button" onclick={() => onNavigate('requests')}>View requests <svg class="icon"><use href="#i-arrow-right" /></svg></button>
    </article>
    <article class="stat-card surface">
      <div class="stat-top"><span class="stat-icon"><svg class="icon"><use href="#i-database" /></svg></span><span class="stat-number">LMDB</span></div>
      <h2>Storage</h2>
      <div class="stat-trend">Local embedded environment</div>
      <button class="details-link" type="button" onclick={() => onNavigate('settings')}>Settings <svg class="icon"><use href="#i-arrow-right" /></svg></button>
    </article>
  </section>

  <div class="help-grid">
    <section class="course-card surface help-faq" aria-labelledby="faq-heading">
      <div class="card-title-row"><h2 id="faq-heading">Frequently asked</h2></div>
      <p class="card-subtitle">Common questions about providers, combos, and routing.</p>
      <div class="faq-list">
        <details class="faq-item">
          <summary>How does failover between providers work?</summary>
          <p>When a combo has multiple targets, ExoRoute tries them in priority order. On retryable errors (429, 5xx, timeouts) it moves to the next target. Non-retryable errors like 400 stop immediately. See the <a href="/docs">Docs</a>.</p>
        </details>
        <details class="faq-item">
          <summary>Are my provider API keys stored securely?</summary>
          <p>Keys are stored in the local LMDB environment under <code>~/.exoroute</code>, never sent to the browser after creation, and masked everywhere in the UI. The database file is owned by a single ExoRoute process — keep it on local disk only.</p>
        </details>
        <details class="faq-item">
          <summary>What happens when a provider quota runs out?</summary>
          <p>The circuit breaker trips for that provider, combos reroute to healthy targets automatically, and you'll see the provider marked on the <button class="details-link" type="button" onclick={() => onNavigate('quota')}>Quota</button> page. Once quota resets or you top up, the breaker half-opens and traffic resumes.</p>
        </details>
        <details class="faq-item">
          <summary>Can I use a different protocol upstream?</summary>
          <p>Yes — ExoRoute translates between OpenAI and Anthropic formats. Set the provider type to Anthropic or OpenAI-compatible, and clients can keep using whichever format they prefer.</p>
        </details>
        <details class="faq-item">
          <summary>Why did a request return 429?</summary>
          <p>Either the client API key hit its rate limit, or all combo targets are quota-limited/circuit-broken. Check the <button class="details-link" type="button" onclick={() => onNavigate('requests')}>Requests</button> page — each row shows which provider and target were tried.</p>
        </details>
        <details class="faq-item">
          <summary>How do I back up my configuration?</summary>
          <p>Use <strong>Export backup</strong> under <button class="details-link" type="button" onclick={() => onNavigate('settings')}>Settings → Data &amp; backup</button> for a snapshot of the LMDB environment.</p>
        </details>
      </div>
    </section>

    <div class="help-side">
      <section class="course-card surface help-card">
        <div class="card-title-row"><h2>Guides</h2></div>
        <div class="help-links">
          <a class="help-link" href="/docs/quickstart"><span class="help-link-icon"><svg class="icon"><use href="#i-play" /></svg></span><span><strong>Quickstart</strong><small>First request in 5 minutes</small></span><svg class="icon help-arrow"><use href="#i-arrow-right" /></svg></a>
          <a class="help-link" href="/docs/reference"><span class="help-link-icon"><svg class="icon"><use href="#i-key" /></svg></span><span><strong>API reference</strong><small>Endpoints &amp; headers</small></span><svg class="icon help-arrow"><use href="#i-arrow-right" /></svg></a>
          <a class="help-link" href="/docs/integrations"><span class="help-link-icon"><svg class="icon"><use href="#i-zap" /></svg></span><span><strong>Integrations</strong><small>Clients &amp; SDKs</small></span><svg class="icon help-arrow"><use href="#i-arrow-right" /></svg></a>
          <button class="help-link" type="button" onclick={() => onNavigate('chat')}><span class="icon help-link-icon warn"><svg class="icon"><use href="#i-terminal" /></svg></span><span><strong>Try in Chat</strong><small>Test a model end-to-end</small></span><svg class="icon help-arrow"><use href="#i-arrow-right" /></svg></button>
        </div>
      </section>

      <section class="course-card surface help-card">
        <div class="card-title-row"><h2>Still stuck?</h2></div>
        <p class="card-subtitle">Include your request ID from the Requests page — it lets us trace the exact upstream attempt.</p>
        <div class="help-contact">
          <button class="page-action" type="button" onclick={() => showDashboardToast('Opening mail client…')}><svg class="icon"><use href="#i-mail" /></svg> Email support</button>
          <button class="page-action" type="button" onclick={copyDiagnostics}><svg class="icon"><use href="#i-clipboard" /></svg> Copy diagnostics</button>
        </div>
      </section>

      <section class="course-card surface help-card help-sysinfo">
        <div class="card-title-row"><h2>System info</h2></div>
        <dl class="sysinfo">
          <div><dt>Version</dt><dd>ExoRoute dashboard</dd></div>
          <div><dt>Uptime</dt><dd>{overview ? formatUptime(overview.uptime_seconds, tr) : '—'}</dd></div>
          <div><dt>Requests</dt><dd>{overview ? overview.request_count.toLocaleString() : '—'}</dd></div>
          <div><dt>Routes</dt><dd>{overview ? overview.route_count.toLocaleString() : '—'}</dd></div>
          <div><dt>Protocols</dt><dd>OpenAI · Anthropic</dd></div>
        </dl>
      </section>
    </div>
  </div>
</div>
