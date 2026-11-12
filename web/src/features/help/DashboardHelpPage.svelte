<script lang="ts">
  import Button from "../../components/Button.svelte";
  import { onDestroy, onMount } from 'svelte';
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
  let endpointCopied = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

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

  onDestroy(() => clearTimeout(copyTimer));

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

  async function copyEndpoint(): Promise<void> {
    try {
      await navigator.clipboard.writeText(endpointUrl);
      endpointCopied = true;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (endpointCopied = false), 1600);
    } catch {
      showDashboardToast(tr('Copy failed'));
    }
  }

  function scrollToSection(id: string): void {
    document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' });
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

<div class="page-surface max-nav:max-h-none! max-nav:overflow-visible! max-nav:content-start!">
  <section class="surface flex gap-[18px] p-4 ps-[22px] pe-[22px] max-[760px]:flex-col max-[760px]:gap-3 max-[760px]:p-4" aria-label={tr('Gateway endpoint')}>
    <div class="grid size-[42px] shrink-0 place-items-center rounded-[13px] bg-db-accent-tint text-accent [&_.icon]:size-5!"><svg class="icon" aria-hidden="true"><use href="#i-server" /></svg></div>
    <div class="flex min-w-0 flex-1 flex-col gap-1.5 [&>p]:m-0 [&>p]:text-muted [&>p]:text-[12.5px] [&>p]:leading-[1.5]">
      <div class="flex flex-wrap items-center gap-2.5">
        <strong class="text-[15px] text-ink">{tr('Gateway endpoint')}</strong>
        <span class="inline-flex items-center gap-1.5 rounded-full ps-[7px] pe-[9px] py-0.5 text-caption font-semibold {gatewayStatus === 'unavailable' ? 'bg-db-danger-tint text-db-danger' : 'bg-db-success-tint text-db-success'}">
          <span class="size-[7px] shrink-0 rounded-full bg-current {gatewayStatus === 'unavailable' ? '' : 'animate-[help-pulse_2.2s_ease-in-out_infinite]'}" aria-hidden="true"></span>
          {gatewayStatus === 'running'
            ? tr('Running')
            : gatewayStatus === 'unavailable'
              ? tr('Gateway unavailable')
              : '—'}
        </span>
      </div>
      <p>{tr('Point any OpenAI- or Anthropic-compatible client at this base URL, then send the matching protocol path.')}</p>
      <div class="mt-0.5 flex flex-wrap items-center gap-2.5">
        <code class="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap rounded-[10px] border border-line bg-db-tint-strong px-[13px] py-[9px] font-mono text-ink text-[12.5px]">{endpointUrl}</code>
        <Button class="mini-add h-[38px]! rounded-[10px]! px-3.5! {endpointCopied ? 'bg-db-success-tint! text-db-success!' : ''}" type="button" onclick={copyEndpoint}>
          <svg class="icon" aria-hidden="true"><use href={endpointCopied ? '#i-check' : '#i-copy'} /></svg>{endpointCopied ? tr('Copied') : tr('Copy')}
        </Button>
      </div>
      <div class="mt-1.5 grid grid-cols-1 gap-2.5 min-[681px]:grid-cols-2 min-[1100px]:grid-cols-4">
        {#each [
          { method: 'POST', path: '/v1/chat/completions', label: tr('Chat Completions') },
          { method: 'POST', path: '/v1/responses', label: tr('Responses') },
          { method: 'POST', path: '/v1/messages', label: tr('Anthropic Messages') },
          { method: 'GET', path: '/v1/models', label: tr('Models') },
        ] as item (item.path)}
          <button
            type="button"
            class="group flex min-w-0 cursor-pointer flex-col gap-1.5 rounded-[10px] border border-line bg-surface px-3 py-2.5 text-left transition-colors duration-150 hover:border-accent hover:bg-db-tint-strong"
            aria-label={`${item.method} ${endpointBase}${item.path}`}
            onclick={() => void copyText(`${endpointBase}${item.path}`, tr('Endpoint copied'))}
          >
            <span class="flex min-w-0 items-center gap-2">
              <span class="shrink-0 rounded-[5px] px-[6px] py-px font-mono text-[10px] font-bold {item.method === 'GET' ? 'bg-db-success-tint text-db-success' : 'bg-db-accent-tint text-accent-strong'}">{item.method}</span>
              <code class="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap font-mono text-ink text-[11.5px]">{item.path}</code>
              <svg class="icon size-3.5! shrink-0 text-muted opacity-0 transition-opacity duration-150 group-hover:opacity-100" aria-hidden="true"><use href="#i-copy" /></svg>
            </span>
            <span class="text-muted text-caption">{item.label}</span>
          </button>
        {/each}
      </div>
    </div>
  </section>

  <section class="grid grid-cols-1 gap-2.5 sm:grid-cols-2 lg:grid-cols-3" aria-label={tr('System info')}>
    <article class="flex min-w-0 flex-col gap-1 rounded-[12px] border border-line bg-surface px-3.5 py-3">
      <span class="text-muted text-[11px] leading-tight font-medium tracking-[0.04em] uppercase">{tr('Uptime')}</span>
      <strong class="text-ink text-2xl leading-tight font-semibold tabular-nums">{overview ? formatUptime(overview.uptime_seconds, tr) : '—'}</strong>
      <small class="text-muted text-xs">{tr('Current process')}</small>
    </article>
    <article class="flex min-w-0 flex-col gap-1 rounded-[12px] border border-line bg-surface px-3.5 py-3">
      <span class="text-muted text-[11px] leading-tight font-medium tracking-[0.04em] uppercase">{tr('Requests')}</span>
      <strong class="text-ink text-2xl leading-tight font-semibold tabular-nums">{overview ? overview.request_count.toLocaleString() : '—'}</strong>
      <small class="text-muted text-xs">{tr('Request log retention stored in LMDB.')}</small>
    </article>
    <article class="flex min-w-0 flex-col gap-1 rounded-[12px] border border-line bg-surface px-3.5 py-3">
      <span class="text-muted text-[11px] leading-tight font-medium tracking-[0.04em] uppercase">{tr('Success rate')}</span>
      <strong class="text-ink text-2xl leading-tight font-semibold tabular-nums">{successRate}</strong>
      <small class="text-muted text-xs">{tr('Last 24 hours')}</small>
    </article>
    <article class="flex min-w-0 flex-col gap-1 rounded-[12px] border border-line bg-surface px-3.5 py-3">
      <span class="text-muted text-[11px] leading-tight font-medium tracking-[0.04em] uppercase">{tr('Average latency')}</span>
      <strong class="text-ink text-2xl leading-tight font-semibold tabular-nums">{latency}</strong>
      <small class="text-muted text-xs">{tr('Last 24 hours')}</small>
    </article>
    <article class="flex min-w-0 flex-col gap-1 rounded-[12px] border border-line bg-surface px-3.5 py-3">
      <span class="text-muted text-[11px] leading-tight font-medium tracking-[0.04em] uppercase">{tr('Providers')}</span>
      <strong class="text-ink text-2xl leading-tight font-semibold tabular-nums">{overview ? overview.provider_count : '—'}</strong>
      <small class="text-muted text-xs">{overview ? tr('{count} combos configured', { count: overview.combo_count }) : tr('Configured upstreams')}</small>
    </article>
    <article class="flex min-w-0 flex-col gap-1 rounded-[12px] border border-line bg-surface px-3.5 py-3">
      <span class="text-muted text-[11px] leading-tight font-medium tracking-[0.04em] uppercase">{tr('Storage')}</span>
      <strong class="text-ink text-2xl leading-tight font-semibold tabular-nums">LMDB</strong>
      <small class="text-muted text-xs">{tr('Local embedded environment')}</small>
    </article>
  </section>

  <div class="grid grid-cols-[minmax(0,1.62fr)_minmax(0,1fr)] gap-[18px] items-start max-[1100px]:grid-cols-1">
    <section class="course-card surface flex flex-col max-[760px]:[&_.card-title-row]:flex-col! max-[760px]:[&_.card-title-row]:items-start!" aria-labelledby="help-reference-heading">
      <div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-3">
        <div class="min-w-0">
          <h2 id="help-reference-heading" class="m-0 overflow-hidden text-ellipsis whitespace-nowrap text-ink text-section-title font-semibold [letter-spacing:var(--tracking-tight)]">{tr('Troubleshooting')}</h2>
          <p class="card-subtitle whitespace-normal!">
            {query
              ? tr('{count} of {total} topics match your search.', { count: matchCount, total: topics.length + codes.length })
              : tr('Common questions, then the status codes the gateway can return.')}
          </p>
        </div>
        <div class="flex min-w-0 flex-1 flex-wrap items-center justify-end gap-2 max-[760px]:w-full max-[760px]:flex-none max-[760px]:justify-start">
          {#if visibleTopics.length > 0 && visibleCodes.length > 0}
            <div class="flex shrink-0 items-center gap-0.5 rounded-full border border-line bg-surface-sunken p-[3px]" role="group" aria-label={tr('Jump to section')}>
              <button type="button" class="rounded-full px-2.5 py-1 text-caption font-medium text-muted transition-colors duration-150 hover:bg-surface hover:text-ink" onclick={() => scrollToSection('help-topics')}>{tr('Questions')}</button>
              <button type="button" class="rounded-full px-2.5 py-1 text-caption font-medium text-muted transition-colors duration-150 hover:bg-surface hover:text-ink" onclick={() => scrollToSection('help-codes')}>{tr('Status codes')}</button>
            </div>
          {/if}
          <label class="flex min-w-[min(280px,100%)] max-w-[440px] flex-1 items-center gap-[7px] rounded-full border border-line bg-surface px-[13px] py-[9px] text-muted transition-[border-color,box-shadow] duration-150 focus-within:border-accent focus-within:ring-3 focus-within:ring-db-accent-ring [&_.icon]:size-3.5! [&_.icon]:shrink-0 max-[760px]:w-full max-[760px]:min-w-0 max-[760px]:max-w-none">
            <svg class="icon" aria-hidden="true"><use href="#i-search" /></svg>
            <Input
              type="search"
              bare
              class="w-full appearance-none border-0 bg-transparent! p-0 text-sm text-ink outline-none placeholder:text-muted"
              bind:value={query}
              placeholder={tr('Search topics and status codes…')}
              aria-label={tr('Search help topics')}
            />
            {#if query}
              <Button class="grid size-[18px]! shrink-0 place-items-center rounded-full! border-0! p-0! bg-transparent! text-muted! cursor-pointer hover:bg-line! hover:text-ink! [&_.icon]:size-[11px]!" type="button" aria-label={tr('Clear search')} onclick={() => (query = '')}>
                <svg class="icon" aria-hidden="true"><use href="#i-close" /></svg>
              </Button>
            {/if}
          </label>
        </div>
      </div>

      {#if matchCount === 0}
        <div class="flex flex-col items-center gap-2 px-4 pt-[30px] pb-[26px] text-center [&>.icon]:size-6! [&>.icon]:text-muted">
          <svg class="icon" aria-hidden="true"><use href="#i-search" /></svg>
          <strong class="text-ink text-[13.5px]">{tr('No topic matches that search.')}</strong>
          <span class="max-w-[340px] text-muted text-[12.5px] leading-[1.5]">{tr('Try a status code, a provider name, or a feature such as backup.')}</span>
          <Button class="page-action" type="button" onclick={() => (query = '')}>{tr('Clear search')}</Button>
        </div>
      {:else}
        {#if visibleTopics.length > 0}
          <h3 id="help-topics" class="mt-3.5 mb-0.5 scroll-mt-4 text-muted text-xs font-medium tracking-wider uppercase">{tr('Questions')}</h3>
          <div class="faq-list">
            {#each visibleTopics as topic (topic.id)}
              {@const target = topic.target}
              <details class="group/faq border-b border-line last:border-b-0">
                <summary class="-mx-3 flex items-center justify-between gap-3 rounded-lg px-3 py-3 text-ink text-[13.5px] font-medium cursor-pointer list-none transition-colors duration-150 hover:bg-db-tint-strong hover:text-accent [&::-webkit-details-marker]:hidden">{topic.title}<span class="size-2 shrink-0 border-e-2 border-b-2 border-muted rotate-45 transition-transform duration-200 group-open/faq:-rotate-[135deg]"></span></summary>
                <div class="flex flex-col items-start gap-1.5 px-1 pb-3.5 [&>p]:m-0 [&>p]:text-muted [&>p]:text-[12.5px] [&>p]:leading-[1.6] [&_.details-link]:mt-0">
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
          <h3 id="help-codes" class="mt-3.5 mb-0.5 scroll-mt-4 text-muted text-xs font-medium tracking-wider uppercase">{tr('Status codes')}</h3>
          <dl class="mt-1 mb-0 flex flex-col">
            {#each visibleCodes as code (code.status)}
              <div class="flex items-baseline gap-3.5 border-b border-line py-[9px] px-1 last:border-b-0 max-[760px]:flex-col max-[760px]:items-start max-[760px]:gap-1">
                <dt class="flex min-w-[132px] shrink-0 items-center gap-2 max-[760px]:min-w-0">
                  <span class="inline-grid min-w-[38px] place-items-center rounded-[7px] px-[7px] py-0.5 font-mono text-[11.5px] font-medium {statusTone(code.status) === 'critical' ? 'bg-db-danger-tint text-db-danger' : statusTone(code.status) === 'warn' ? 'bg-db-accent-tint text-accent-strong' : 'bg-db-tint-strong text-muted'}">{code.status}</span>
                  <span class="text-ink text-[12.5px] font-medium">{code.reason}</span>
                </dt>
                <dd class="m-0 text-muted text-[12.5px] leading-[1.55]">{code.detail}</dd>
              </div>
            {/each}
          </dl>
        {/if}
      {/if}
    </section>

    <div class="flex flex-col gap-[18px]">
      <section class="course-card surface flex flex-col gap-0.5">
        <div class="card-title-row"><h2>{tr('Common tasks')}</h2></div>
        <div class="mt-1.5 mb-0 flex flex-col gap-1">
          {#each guides as guide (guide.title)}
            <Button class="group/hlink flex min-w-0 items-center gap-2.5 rounded-xl! border-0! bg-transparent! p-2.5 text-ink! text-left cursor-pointer font-inherit no-underline transition-colors duration-150 hover:bg-db-tint-strong! focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-db-accent-ring [&>span:nth-child(2)]:flex [&>span:nth-child(2)]:min-w-0 [&>span:nth-child(2)]:flex-1 [&>span:nth-child(2)]:flex-col [&>span:nth-child(2)]:gap-px [&_.help-arrow]:text-muted! [&_.help-arrow]:transition-[color,transform] [&_.help-arrow]:duration-150 hover:[&_.help-arrow]:text-accent! hover:[&_.help-arrow]:translate-x-1" type="button" onclick={() => onNavigate(guide.page)}>
              <span class="flex size-[34px] shrink-0 items-center justify-center rounded-[10px] bg-db-accent-tint text-accent transition-colors duration-200 group-hover/hlink:bg-accent group-hover/hlink:text-accent-contrast [&_.icon]:size-[17px]!"><svg class="icon"><use href={`#${guide.icon}`} /></svg></span>
              <span><strong class="text-[13px] leading-[1.35] font-medium">{guide.title}</strong><small class="text-muted text-xs leading-[1.35]">{guide.hint}</small></span>
              <svg class="icon help-arrow size-[15px]!"><use href="#i-arrow-right" /></svg>
            </Button>
          {/each}
        </div>
      </section>

      <section class="course-card surface flex flex-col gap-0.5">
        <div class="card-title-row"><h2>{tr('Documentation')}</h2></div>
        <div class="mt-1.5 mb-0 flex flex-col gap-1">
          {#each docsLinks as link (link.title)}
            <a class="group/hlink flex min-w-0 items-center gap-2.5 rounded-xl border-0 bg-transparent p-2.5 text-ink text-left no-underline transition-colors duration-150 hover:bg-db-tint-strong [&>span:nth-child(2)]:flex [&>span:nth-child(2)]:min-w-0 [&>span:nth-child(2)]:flex-1 [&>span:nth-child(2)]:flex-col [&>span:nth-child(2)]:gap-px [&_.help-arrow]:text-muted [&_.help-arrow]:transition-[color,transform] [&_.help-arrow]:duration-150 hover:[&_.help-arrow]:text-accent hover:[&_.help-arrow]:translate-x-1" href={link.href}>
              <span class="flex size-[34px] shrink-0 items-center justify-center rounded-[10px] bg-db-accent-tint text-accent-strong transition-colors duration-200 group-hover/hlink:bg-accent group-hover/hlink:text-accent-contrast [&_.icon]:size-[17px]!"><svg class="icon"><use href={`#${link.icon}`} /></svg></span>
              <span><strong class="text-[13px] leading-[1.35] font-medium">{link.title}</strong><small class="text-muted text-xs leading-[1.35]">{link.hint}</small></span>
              <svg class="icon help-arrow size-[15px]!"><use href="#i-arrow-right" /></svg>
            </a>
          {/each}
        </div>
      </section>

      <section class="course-card surface flex flex-col gap-0.5 [&_.page-action]:self-start">
        <div class="card-title-row"><h2>{tr('System info')}</h2></div>
        <dl class="mt-1.5 mb-2.5 flex flex-col gap-[7px] [&>div]:flex [&>div]:justify-between [&>div]:gap-3 [&>div]:rounded-[10px] [&>div]:bg-db-tint-strong [&>div]:px-[11px] [&>div]:py-2 [&>div]:text-[12.5px] [&_dt]:text-muted [&_dd]:m-0 [&_dd]:overflow-hidden [&_dd]:text-ink [&_dd]:font-mono [&_dd]:text-[11.5px] [&_dd]:font-semibold [&_dd]:text-ellipsis [&_dd]:whitespace-nowrap">
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
