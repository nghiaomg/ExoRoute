<script lang="ts">
  import Button from "../../components/Button.svelte";
  import { onMount } from 'svelte';
  import { api } from '../../lib/api';
  import Input from '../../components/Input.svelte';
  import type { DashboardPage } from '../../lib/navigation';
  import type { Locale } from '../../lib/i18n';
  import { formatUptime, formatGatewayEndpoint } from '../../lib/format';
  import type { Translate } from '../../lib/format';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import type { Overview, RequestStatistics, PublicSettings } from '../../lib/types';
  import { DASHBOARD_VERSION } from '../../lib/version';
  import { errorCodes, faqTopics, filterHelpItems } from './helpTopics';
  import '../../styles/dashboard/pages/help.css';

  interface Props {
    tr: Translate;
    locale: Locale;
    onNavigate: (page: DashboardPage) => void;
  }
  let { tr, locale: _locale, onNavigate }: Props = $props();
  let overview = $state<Overview | null>(null);
  let stats = $state<RequestStatistics | null>(null);
  let settings = $state<PublicSettings>({});
  let gatewayStatus = $state<'loading' | 'running' | 'unavailable'>('loading');
  let query = $state('');

  onMount(() => {
    void api.overview()
      .then((value) => {
        overview = value;
        gatewayStatus = 'running';
      })
      .catch(() => (gatewayStatus = 'unavailable'));
    void api.statistics('1d').then((value) => (stats = value)).catch(() => {});
    void api.settings().then((value) => (settings = value)).catch(() => {});
  });

  const pageLabels: Record<DashboardPage, string> = {
    overview: 'Overview',
    providers: 'Providers',
    combos: 'Combos',
    proxies: 'Proxies',
    quota: 'Quota',
    requests: 'Requests',
    'api-keys': 'API keys',
    settings: 'Settings',
    chat: 'Chat',
    help: 'Help Center',
  };

  function targetLabel(page: DashboardPage): string {
    return pageLabels[page];
  }

  const topics = $derived(faqTopics(tr));
  const codes = $derived(errorCodes(tr));
  const endpoint = $derived(
    formatGatewayEndpoint(
      settings.host,
      settings.port,
      typeof window !== 'undefined' ? window.location.origin : null,
    ),
  );
  const endpointBase = $derived(endpoint.endsWith('/v1') ? endpoint.slice(0, -3) : endpoint);
  const endpointUrl = $derived(`${endpointBase}/v1`);
  const visibleTopics = $derived(
    filterHelpItems(topics, query, (topic) => `${topic.title} ${topic.body} ${topic.keywords}`),
  );
  const visibleCodes = $derived(
    filterHelpItems(codes, query, (code) => `${code.status} ${code.reason} ${code.detail} ${code.keywords}`),
  );
  const matchCount = $derived(visibleTopics.length + visibleCodes.length);

  const successRate = $derived(stats ? `${stats.success_rate.toFixed(1)}%` : '—');
  const latency = $derived(
    stats && stats.average_latency_ms > 0 ? `${Math.round(stats.average_latency_ms)} ms` : '—',
  );

  async function copyText(value: string, message: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(value);
      showDashboardToast(message);
    } catch {
      showDashboardToast(tr('Copy failed'));
    }
  }

  function copyDiagnostics(): void {
    void copyText(
      [
        `ExoRoute ${DASHBOARD_VERSION}`,
        `endpoint: ${endpointBase}`,
        `gateway API: ${endpointUrl}`,
        `providers: ${overview?.provider_count ?? '?'} combos: ${overview?.combo_count ?? '?'}`,
        `requests: ${overview?.request_count ?? '?'}`,
        `24h success rate: ${stats ? stats.success_rate.toFixed(1) : '?'}%`,
        `24h average latency: ${stats ? Math.round(stats.average_latency_ms) : '?'} ms`,
      ].join(' · '),
      tr('Diagnostics copied'),
    );
  }

  const guides = $derived<Array<{ title: string; hint: string; icon: string; page: DashboardPage }>>([
    { title: tr('Connect a provider'), hint: tr('Add an upstream and at least one API key.'), icon: 'i-link', page: 'providers' },
    { title: tr('Create a combo'), hint: tr('Group models behind one name with failover.'), icon: 'i-layers', page: 'combos' },
    { title: tr('View requests'), hint: tr('Inspect outcomes, latency, and upstream errors.'), icon: 'i-clipboard', page: 'requests' },
    { title: tr('Try in Chat'), hint: tr('Send a prompt through the gateway from the dashboard.'), icon: 'i-terminal', page: 'chat' },
  ]);

  const docsLinks = $derived<Array<{ title: string; hint: string; icon: string; href: string }>>([
    { title: tr('Quickstart'), hint: tr('First request in five minutes'), icon: 'i-play', href: '/docs/quickstart' },
    { title: tr('API reference'), hint: tr('Every endpoint and header'), icon: 'i-key', href: '/docs/reference' },
    { title: tr('Integrations'), hint: tr('Clients and SDK setup'), icon: 'i-zap', href: '/docs/integrations' },
  ]);

  function statusTone(status: number): string {
    if (status >= 500) return 'critical';
    if (status === 429) return 'warn';
    return 'client';
  }
</script>

<div class="page-heading">
  <div class="heading-copy">
    <h1>{tr('Help Center')}</h1>
    <p>{tr('Guides, troubleshooting, and system status for your ExoRoute gateway.')}</p>
  </div>
  <div class="heading-actions">
    <Button class="action-button" type="button" onclick={copyDiagnostics}>
      <span class="button-icon"><svg class="icon"><use href="#i-clipboard" /></svg></span>
      <span class="button-label">{tr('Copy diagnostics')}</span>
    </Button>
  </div>
</div>

<div class="page-surface">
  <section class="help-lead surface" aria-label={tr('Gateway endpoint')}>
    <div class="help-lead-mark"><svg class="icon" aria-hidden="true"><use href="#i-wifi" /></svg></div>
    <div class="help-lead-copy">
      <div class="help-lead-heading">
        <strong>{tr('Gateway endpoint')}</strong>
        <span class="help-live" class:unavailable={gatewayStatus === 'unavailable'}>
          <span class="status-pulse" aria-hidden="true"></span>
          {gatewayStatus === 'running'
            ? tr('Running')
            : gatewayStatus === 'unavailable'
              ? tr('Gateway unavailable')
              : '—'}
        </span>
      </div>
      <p>{tr('Point any OpenAI- or Anthropic-compatible client at this base URL, then send the matching protocol path.')}</p>
      <div class="help-endpoint-row">
        <code class="help-endpoint">{endpointUrl}</code>
        <Button class="mini-add" type="button" onclick={() => void copyText(endpointUrl, tr('Endpoint copied'))}>
          <svg class="icon" aria-hidden="true"><use href="#i-copy" /></svg>{tr('Copy')}
        </Button>
      </div>
      <div class="help-path-list">
        <span><code>POST /v1/chat/completions</code>{tr('Chat Completions')}</span>
        <span><code>POST /v1/responses</code>{tr('Responses')}</span>
        <span><code>POST /v1/messages</code>{tr('Anthropic Messages')}</span>
        <span><code>GET /v1/models</code>{tr('Models')}</span>
      </div>
    </div>
  </section>

  <section class="help-facts" aria-label={tr('System info')}>
    <article class="surface help-fact">
      <span class="help-fact-label">{tr('Uptime')}</span>
      <strong>{overview ? formatUptime(overview.uptime_seconds, tr) : '—'}</strong>
      <small>{tr('Current process')}</small>
    </article>
    <article class="surface help-fact">
      <span class="help-fact-label">{tr('Requests')}</span>
      <strong>{overview ? overview.request_count.toLocaleString() : '—'}</strong>
      <small>{tr('Request log retention stored in LMDB.')}</small>
    </article>
    <article class="surface help-fact">
      <span class="help-fact-label">{tr('Success rate')}</span>
      <strong>{successRate}</strong>
      <small>{tr('Last 24 hours')}</small>
    </article>
    <article class="surface help-fact">
      <span class="help-fact-label">{tr('Average latency')}</span>
      <strong>{latency}</strong>
      <small>{tr('Last 24 hours')}</small>
    </article>
    <article class="surface help-fact">
      <span class="help-fact-label">{tr('Providers')}</span>
      <strong>{overview ? overview.provider_count : '—'}</strong>
      <small>{overview ? tr('{count} combos configured', { count: overview.combo_count }) : tr('Configured upstreams')}</small>
    </article>
    <article class="surface help-fact">
      <span class="help-fact-label">{tr('Storage')}</span>
      <strong>LMDB</strong>
      <small>{tr('Local embedded environment')}</small>
    </article>
  </section>

  <div class="help-layout">
    <section class="course-card surface help-reference" aria-labelledby="help-reference-heading">
      <div class="card-title-row">
        <h2 id="help-reference-heading">{tr('Troubleshooting')}</h2>
        <label class="help-search">
          <svg class="icon" aria-hidden="true"><use href="#i-search" /></svg>
          <Input
            type="search"
            bind:value={query}
            placeholder={tr('Search topics and status codes…')}
            aria-label={tr('Search help topics')}
          />
          {#if query}
            <Button class="help-search-clear" type="button" aria-label={tr('Clear search')} onclick={() => (query = '')}>
              <svg class="icon" aria-hidden="true"><use href="#i-close" /></svg>
            </Button>
          {/if}
        </label>
      </div>
      <p class="card-subtitle">
        {query
          ? tr('{count} of {total} topics match your search.', { count: matchCount, total: topics.length + codes.length })
          : tr('Common questions, then the status codes the gateway can return.')}
      </p>

      {#if matchCount === 0}
        <div class="help-empty">
          <svg class="icon" aria-hidden="true"><use href="#i-search" /></svg>
          <strong>{tr('No topic matches that search.')}</strong>
          <span>{tr('Try a status code, a provider name, or a feature such as backup.')}</span>
          <Button class="page-action" type="button" onclick={() => (query = '')}>{tr('Clear search')}</Button>
        </div>
      {:else}
        {#if visibleTopics.length > 0}
          <h3 class="help-group-heading">{tr('Questions')}</h3>
          <div class="faq-list">
            {#each visibleTopics as topic (topic.id)}
              {@const target = topic.target}
              <details class="faq-item">
                <summary>{topic.title}</summary>
                <div class="faq-body">
                  <p>{topic.body}</p>
                  {#if target}
                    <Button class="details-link" type="button" onclick={() => onNavigate(target)}>
                      {tr('Open {page}', { page: tr(targetLabel(target)) })} <svg class="icon"><use href="#i-arrow-right" /></svg>
                    </Button>
                  {/if}
                </div>
              </details>
            {/each}
          </div>
        {/if}

        {#if visibleCodes.length > 0}
          <h3 class="help-group-heading">{tr('Status codes')}</h3>
          <dl class="help-codes">
            {#each visibleCodes as code (code.status)}
              <div class="help-code">
                <dt>
                  <span class="help-code-status" class:critical={statusTone(code.status) === 'critical'} class:warn={statusTone(code.status) === 'warn'}>{code.status}</span>
                  <span class="help-code-reason">{code.reason}</span>
                </dt>
                <dd>{code.detail}</dd>
              </div>
            {/each}
          </dl>
        {/if}
      {/if}
    </section>

    <div class="help-side">
      <section class="course-card surface help-card">
        <div class="card-title-row"><h2>{tr('Common tasks')}</h2></div>
        <div class="help-links">
          {#each guides as guide (guide.title)}
            <Button class="help-link" type="button" onclick={() => onNavigate(guide.page)}>
              <span class="help-link-icon"><svg class="icon"><use href={`#${guide.icon}`} /></svg></span>
              <span><strong>{guide.title}</strong><small>{guide.hint}</small></span>
              <svg class="icon help-arrow"><use href="#i-arrow-right" /></svg>
            </Button>
          {/each}
        </div>
      </section>

      <section class="course-card surface help-card">
        <div class="card-title-row"><h2>{tr('Documentation')}</h2></div>
        <div class="help-links">
          {#each docsLinks as link (link.title)}
            <a class="help-link" href={link.href}>
              <span class="help-link-icon warn"><svg class="icon"><use href={`#${link.icon}`} /></svg></span>
              <span><strong>{link.title}</strong><small>{link.hint}</small></span>
              <svg class="icon help-arrow"><use href="#i-arrow-right" /></svg>
            </a>
          {/each}
        </div>
      </section>

      <section class="course-card surface help-card help-sysinfo">
        <div class="card-title-row"><h2>{tr('System info')}</h2></div>
        <dl class="sysinfo">
          <div><dt>{tr('Version')}</dt><dd>{DASHBOARD_VERSION}</dd></div>
          <div><dt>{tr('Endpoint')}</dt><dd>{endpointBase}</dd></div>
          <div><dt>{tr('Protocols')}</dt><dd>OpenAI · Anthropic</dd></div>
          <div><dt>{tr('Database')}</dt><dd>LMDB</dd></div>
          <div><dt>{tr('Dropped request logs')}</dt><dd>{overview ? overview.dropped_request_logs.toLocaleString() : '—'}</dd></div>
        </dl>
        <Button class="page-action" type="button" onclick={copyDiagnostics}>
          <svg class="icon" aria-hidden="true"><use href="#i-clipboard" /></svg>{tr('Copy diagnostics')}
        </Button>
      </section>
    </div>
  </div>
</div>
