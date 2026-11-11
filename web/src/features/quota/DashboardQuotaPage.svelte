<script lang="ts">
  import Button from "../../components/Button.svelte";
  import StatsCard from "../../components/StatsCard.svelte";
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import type { DashboardPage } from '../../lib/navigation';
  import type { Locale } from '../../lib/i18n';
  import type { Translate } from '../../lib/format';
  import { formatCount } from '../../lib/format';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import { providerLogo } from '../providers/provider-logo';
  import type { Provider, ProviderUsageAccount, ProviderUsageQuota } from '../../lib/types';

  interface Props {
    tr: Translate;
    locale: Locale;
    onNavigate: (page: DashboardPage) => void;
    onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;
    onProviderCountChange: (count: number) => void;
  }

  let { tr, locale, onNavigate, onConnectionChange, onProviderCountChange: _opc }: Props = $props();

  interface ProviderQuotaView {
    provider: Provider;
    accounts: ProviderUsageAccount[];
    quotas: ProviderUsageQuota[];
    plan: string;
    maxUsed: number | null;
    hasData: boolean;
    usageIncomplete: boolean;
    error: string;
  }

  let views = $state<ProviderQuotaView[]>([]);
  let loading = $state(true);
  let refreshing = $state(false);
  let errorMessage = $state('');
  let generation = 0;

  let detailDialog = $state<HTMLDialogElement>();
  let detailView = $state<ProviderQuotaView | null>(null);

  const reporting = $derived(views.filter((v) => v.hasData));
  const quotaCapableProviders = $derived(views.filter((view) =>
    exposesUsage(view.provider) && (view.provider.api_key_count ?? 0) > 0,
  ));
  const allQuotaProvidersFresh = $derived(
    quotaCapableProviders.length > 0 && quotaCapableProviders.every(hasFreshQuotaData),
  );
  const incompleteQuotaProviders = $derived(quotaCapableProviders.filter((view) => view.usageIncomplete));
  // Keyless providers cannot report quotas: keep them off the board entirely
  // instead of rendering a placeholder card.
  const boardViews = $derived(views.filter((view) =>
    exposesUsage(view.provider) && (view.provider.api_key_count ?? 0) > 0 && view.hasData,
  ));
  const avgUtil = $derived.by(() => {
    const values = reporting.flatMap((v) => v.quotas.map((q) => q.used_percent)).filter((n) => Number.isFinite(n));
    if (!values.length) return null;
    return Math.round(values.reduce((a, b) => a + b, 0) / values.length);
  });
  const nearLimit = $derived(reporting.filter((v) => v.maxUsed !== null && v.maxUsed >= 80));
  const missingQuotaProviders = $derived(views.filter((view) =>
    exposesUsage(view.provider) && (view.provider.api_key_count ?? 0) > 0 && !view.hasData,
  ));
  const missingKeys = $derived(views.filter((v) => v.provider.enabled && !(v.provider.api_key_count ?? 0)));
  const localProviders = $derived(views.filter((v) => isLocal(v.provider)));
  const nearLimitTrend = $derived.by(() => {
    if (nearLimit.length) return tr('{name} · {percent}% remaining', { name: nearLimit[0].provider.name, percent: remainingPercent(nearLimit[0].maxUsed ?? 0) });
    if (missingQuotaProviders.length || incompleteQuotaProviders.length) return tr('No quota data reported');
    if (allQuotaProvidersFresh) return tr('All providers healthy');
    return tr('No configured provider reports usage data.');
  });

  function hasFreshQuotaData(view: ProviderQuotaView): boolean {
    return view.hasData
      && !view.usageIncomplete
      && view.accounts.length > 0
      && view.accounts.every((account) => account.status === 'fresh');
  }

  function isLocal(provider: Provider): boolean {
    const id = `${provider.adapter_id} ${provider.name} ${provider.base_url}`.toLowerCase();
    return id.includes('ollama') || id.includes('lmstudio') || id.includes('lm studio') || id.includes('local')
      || /^https?:\/\/(127\.|localhost|0\.0\.0\.0|\[::1\])/.test(provider.base_url);
  }

  /** Mirrors the admin usage gate: adapters without any usage capability
      reject the usage endpoints, so their cards stay off the board. */
  function exposesUsage(provider: Provider): boolean {
    const caps = provider.capabilities;
    return !!caps && (caps.usage_limits || caps.api_key_usage || caps.local_usage_meter === true);
  }

  function providerIcon(provider: Provider): string {
    const id = `${provider.adapter_id} ${provider.name}`.toLowerCase();
    if (id.includes('anthropic') || id.includes('claude')) return 'i-link';
    if (id.includes('google') || id.includes('gemini')) return 'i-zap';
    if (id.includes('openrouter')) return 'i-layers';
    if (isLocal(provider)) return 'i-cpu';
    return 'i-zap';
  }

  /** Same logo source as the Providers page: configured logo_url first,
      then the built-in adapter assets from assets/providers + assets/models. */
  function providerCardLogo(provider: Provider): string | null {
    return providerLogo(provider.logo_url, provider.adapter_id, provider.id, provider.name);
  }

  /** Provider data uses used-percent; render the remaining quota from left to right. */
  function remainingPercent(usedPercent: number): number {
    return Math.round(Math.min(100, Math.max(0, 100 - usedPercent)));
  }

  function quotaBarStyle(usedPercent: number): string {
    const remaining = Math.min(100, Math.max(0, 100 - usedPercent));
    // Filled segment fades from deep orange on the left to soft orange on the right.
    return `background: linear-gradient(to right, var(--quota-fill-deep, #c2410c) 0, var(--quota-fill-soft, #fdba74) ${remaining}%, var(--surface-sunken) ${remaining}% 100%)`;
  }

  function quotaValue(quota: ProviderUsageQuota): string {
    if (quota.uncapped) return quota.used_amount != null ? `${formatAmount(quota.used_amount, quota.unit)} ${tr('used')}` : tr('uncapped');
    if (quota.used_amount != null && quota.limit_amount != null) {
      const remaining = Math.max(0, quota.limit_amount - quota.used_amount);
      return `${formatAmount(remaining, quota.unit)} / ${formatAmount(quota.limit_amount, quota.unit)} ${tr('remaining')}`;
    }
    return tr('{percent}% remaining', { percent: remainingPercent(quota.used_percent) });
  }

  function formatAmount(value: number, unit?: string | null): string {
    const formatted = value >= 1000 ? formatCount(Math.round(value), locale) : value.toLocaleString(locale, { maximumFractionDigits: 1 });
    return `${formatted} ${unit?.trim() || tr('credits')}`;
  }

  function resetLabel(quota: ProviderUsageQuota): string | null {
    if (!quota.reset_at) return null;
    const diffSec = quota.reset_at - Date.now() / 1000;
    if (diffSec <= 0) return tr('reset due');
    const h = Math.floor(diffSec / 3600);
    const m = Math.floor((diffSec % 3600) / 60);
    if (h > 24) return tr('resets in {days}d', { days: Math.floor(h / 24) });
    if (h > 0) return tr('resets in {h}h {m}m', { h, m });
    return tr('resets in {m}m', { m });
  }

  function statusClass(view: ProviderQuotaView): string {
    if (!view.hasData || !hasFreshQuotaData(view)) return 'pending';
    if (view.maxUsed !== null && view.maxUsed >= 90) return 'error';
    if (view.maxUsed !== null && view.maxUsed >= 70) return 'progress';
    return 'complete';
  }

  async function load(): Promise<void> {
    const gen = ++generation;
    loading = true;
    errorMessage = '';
    onConnectionChange('loading');
    const accountUsages = new Map<string, ProviderUsageAccount[]>();
    try {
      const providerRows = await api.providers();
      if (gen !== generation) return;
      const results = await Promise.all(providerRows.map(async (provider) => {
        const base: ProviderQuotaView = {
          provider,
          accounts: [],
          quotas: [],
          plan: '',
          maxUsed: null,
          hasData: false,
          usageIncomplete: false,
          error: '',
        };
        if (!exposesUsage(provider)) return base;
        try {
          const usage = await api.providerUsage(provider.id);
          const accounts = usage.accounts ?? [];
          const quotas = accounts.flatMap((a) => a.snapshot?.quotas ?? a.provider_quota?.snapshot?.quotas ?? []);
          const maxUsed = quotas.length ? Math.max(...quotas.map((q) => q.used_percent)) : null;
          return {
            ...base,
            accounts,
            quotas,
            plan: accounts.map((a) => a.snapshot?.plan).find(Boolean) ?? '',
            maxUsed,
            hasData: quotas.length > 0,
            usageIncomplete: !!usage.next_cursor || (accounts.length > 0 && accounts.some((account) => account.status !== 'fresh')),
          };
        } catch (error) {
          return { ...base, error: error instanceof Error ? error.message : tr('unavailable') };
        }
      }));
      if (gen !== generation) return;
      views = results;
      onConnectionChange('loaded');
    } catch (error) {
      if (gen !== generation) return;
      errorMessage = error instanceof Error ? error.message : tr('Could not load quota data.');
      onConnectionChange('error');
    } finally {
      if (gen === generation) loading = false;
    }
  }

  async function refresh(): Promise<void> {
    if (refreshing) return;
    refreshing = true;
    showDashboardToast(tr('Refreshing quota…'));
    try {
      await Promise.all(views.filter((v) => exposesUsage(v.provider) && (v.hasData || v.provider.api_key_count)).map((v) =>
        api.refreshProviderUsage(v.provider.id).catch(() => null),
      ));
      await load();
      showDashboardToast(tr('Quota refreshed'));
    } finally {
      refreshing = false;
    }
  }

  function openDetail(view: ProviderQuotaView): void {
    detailView = view;
    detailDialog?.showModal();
  }

  function scrollToCard(providerId: string) {
    document.getElementById(`quota-card-${providerId}`)?.scrollIntoView({ behavior: 'smooth', block: 'center' });
  }

  onMount(() => { void load(); });
  onDestroy(() => { generation += 1; });
</script>

<div class="page-heading">
  <div class="heading-copy">
    <h1>{tr('Quota')}</h1>
    <p>{tr('Monitor provider quota and local usage from one place.')}</p>
  </div>
</div>

{#if errorMessage}
  <section class="course-card surface" aria-label={tr('Load error')}>
    <div class="card-title-row"><h2>{tr('Could not load quota')}</h2></div>
    <p class="card-subtitle">{errorMessage}</p>
    <div class="modal-actions" style="padding-top:14px">
      <Button class="page-action primary" type="button" onclick={load}>{tr('Retry')}</Button>
    </div>
  </section>
{:else if loading}
  <section class="stats-grid" aria-label={tr('Loading')}>
    {#each [0, 1, 2, 3] as i (i)}<StatsCard skeleton />{/each}
  </section>
{:else}
  <div class="page-surface">
    <section class="stats-grid" aria-label={tr('Quota overview')}>
      <StatsCard icon="#i-users" value={views.length} title={tr('Providers Monitored')} trend={tr('{reporting} reporting, {missing} missing keys', { reporting: reporting.length, missing: missingKeys.length })} actionLabel={tr('View providers')} onAction={ () => onNavigate('providers') } />
      <StatsCard icon="#i-chart" value={avgUtil === null ? '—' : `${avgUtil}%`} title={tr('Avg Utilization')} trend={tr('Across reporting providers')} actionLabel={tr('See breakdown')} onAction={ () => document.getElementById('quota-board')?.scrollIntoView({ behavior: 'smooth' }) } />
      <StatsCard icon="#i-alert" value={nearLimit.length} title={tr('Near Limit')} trend={nearLimitTrend} actionLabel={nearLimit[0] ? tr('Inspect {name}', { name: nearLimit[0].provider.name }) : tr('See breakdown')} onAction={ () => nearLimit[0] ? openDetail(nearLimit[0]) : document.getElementById('quota-board')?.scrollIntoView({ behavior: 'smooth' }) } />
      <StatsCard icon="#i-cpu" value={localProviders.length} title={tr('Local Providers')} trend={localProviders.map((v) => v.provider.name).join(', ') || tr('None configured')} actionLabel={tr('Open chat')} onAction={ () => onNavigate('chat') } />
    </section>

    <section class="course-card surface flex flex-col" id="quota-board" aria-labelledby="quota-heading">
      <div class="card-title-row">
        <h2 id="quota-heading">{tr('Quota by provider')}</h2>
        <Button class="mini-add disabled:opacity-55 disabled:cursor-default" type="button" onclick={refresh} disabled={refreshing}>
          <svg class="icon"><use href="#i-refresh" /></svg>
          {refreshing ? tr('Refreshing…') : tr('Refresh')}
        </Button>
      </div>
      <p class="card-subtitle">{tr('Upstream quotas are informational — they never change provider routing.')}</p>
      <div class="mt-3.5 grid grid-cols-[repeat(auto-fill,minmax(320px,1fr))] gap-3 max-nav:grid-cols-1">
        {#each boardViews as view (view.provider.id)}
      <article
        class="flex min-w-0 flex-col gap-3.5 rounded-[14px] bg-db-tint-strong p-[18px] transition-[background-color,transform,box-shadow] duration-200 hover:bg-surface-sunken {(view.maxUsed ?? 0) >= 90 ? 'bg-db-accent-tint hover:bg-db-accent-tint' : ''}"
        class:pending={view.usageIncomplete}
        id="quota-card-{view.provider.id}"
      >
            <div class="flex items-center gap-[11px]">
              <span class="course-icon">
                {#if providerCardLogo(view.provider)}
                  <img src={providerCardLogo(view.provider)} alt="" aria-hidden="true" class="block size-full p-[5px] object-contain" />
                {:else}
                  <svg class="icon"><use href="#{providerIcon(view.provider)}" /></svg>
                {/if}
              </span>
              <div class="course-copy">
                <strong>{view.provider.name}</strong>
                <span>{view.plan ? tr('{plan} plan', { plan: view.plan }) : view.provider.base_url}{view.quotas[0] ? ` · ${resetLabel(view.quotas[0]) ?? ''}` : ''}</span>
              </div>
              <span class="status {statusClass(view)}" title={tr('Remaining quota')}>{view.maxUsed === null ? '—' : `${remainingPercent(view.maxUsed)}%`}</span>
            </div>
            <div class="grid gap-3">
              {#if view.quotas.length}
                {#each view.quotas.slice(0, 3) as quota, i (i)}
                  <div>
                    <div class="mb-1.5 flex items-center justify-between gap-2 text-muted text-meta"><span>{quota.label}</span><strong class="text-ink tabular-nums">{quotaValue(quota)}</strong></div>
                    <div class="progress-track bg-[color-mix(in_srgb,var(--text-dim)_58%,var(--surface-sunken))]!" role="img" aria-label={tr('{percent}% remaining', { percent: remainingPercent(quota.used_percent) })}><div class="progress-fill h-full rounded-[inherit] bg-accent! transition-[background-size] duration-200" style={quotaBarStyle(quota.used_percent)}></div></div>
                  </div>
                {/each}
              {:else}
                <div>
                  <div class="mb-1.5 flex items-center justify-between gap-2 text-muted text-meta">
                    <span>{tr('Status')}</span>
                    <strong class="text-ink tabular-nums">{view.error ? view.error : tr('No quota data reported')}</strong>
                  </div>
                  <div class="progress-track"><div class="progress-fill" style="width:0%"></div></div>
                </div>
              {/if}
            </div>
            <Button class="details-link self-start" type="button" onclick={() => openDetail(view)}>{tr('View details')} <svg class="icon"><use href="#i-arrow-right" /></svg></Button>
          </article>
        {/each}
        {#if !boardViews.length}
          <div class="empty-state">{views.length ? tr('No configured provider reports usage data.') : tr('No providers configured.')}</div>
        {/if}
      </div>
    </section>
  </div>
{/if}

<!-- Quota detail dialog -->
<dialog class="modal modal-wide" bind:this={detailDialog} aria-labelledby="quota-dialog-title">
  <div class="modal-card">
    <div class="modal-heading">
      <div>
        <h2 id="quota-dialog-title">{tr('{name} quota', { name: detailView?.provider.name ?? tr('Provider') })}</h2>
        <p>{detailView?.plan ? tr('{plan} plan', { plan: detailView.plan }) : detailView?.provider.base_url}</p>
      </div>
      <Button class="modal-close" type="button" data-close-modal aria-label={tr('Close dialog')}>×</Button>
    </div>
    <div class="grid gap-3.5">
      {#if detailView?.quotas.length}
        {#each detailView.quotas as quota, i (i)}
          <div>
            <div class="mb-1.5 flex items-center justify-between gap-2 text-muted text-meta">
              <span>{quota.label}{resetLabel(quota) ? ` · ${resetLabel(quota)}` : ''}</span>
              <strong class="text-ink tabular-nums">{quotaValue(quota)}</strong>
            </div>
            <div class="progress-track">
              <div class="progress-fill h-full rounded-[inherit] bg-accent! transition-[background-size] duration-200" style={quotaBarStyle(quota.used_percent)}></div>
            </div>
          </div>
        {/each}
      {:else}
        <p class="card-subtitle">{detailView?.error ?? tr('No quota data reported for this provider.')}</p>
      {/if}
      {#each detailView?.accounts ?? [] as account (account.key_id)}
        {#if account.local_meter}
          <div>
            <div class="mb-1.5 flex items-center justify-between gap-2 text-muted text-meta"><span>{tr('Local usage · {name}', { name: account.name || tr('key') })}</span><strong class="text-ink tabular-nums">{tr('{count} windows', { count: Object.keys(account.local_meter.windows).length })}</strong></div>
            {#each Object.entries(account.local_meter.windows) as [window, data] (window)}
              <div class="mb-1.5 flex items-center justify-between gap-2 ps-3.5 text-muted text-meta">
                <span>{window} · {tr('{count} requests', { count: data.requests })} · {tr('{count} tokens', { count: formatCount(data.input_tokens + data.output_tokens, locale) })}</span>
                <strong class="text-ink tabular-nums">{data.cost_micro_usd != null ? `$${(data.cost_micro_usd / 1_000_000).toFixed(2)}` : '—'}</strong>
              </div>
            {/each}
          </div>
        {/if}
      {/each}
    </div>
    <p class="form-note">{tr('Quota values are informational and never change provider routing.')}</p>
    <div class="modal-actions">
      <Button class="page-action" type="button" data-close-modal>{tr('Close')}</Button>
    </div>
  </div>
</dialog>
