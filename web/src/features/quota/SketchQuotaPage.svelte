<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../../lib/api';
  import type { DashboardPage } from '../../lib/navigation';
  import type { Locale } from '../../lib/i18n';
  import type { Translate } from '../../lib/format';
  import { formatCount } from '../../lib/format';
  import { showSketchToast } from '../../lib/sketch';
  import type { Provider, ProviderUsageAccount, ProviderUsageQuota } from '../../lib/types';
  import '../../styles/sketch/pages/quota.css';

  interface Props {
    tr: Translate;
    locale: Locale;
    onNavigate: (page: DashboardPage) => void;
    onConnectionChange: (state: 'idle' | 'loading' | 'loaded' | 'error') => void;
    onProviderCountChange: (count: number) => void;
  }

  let { tr: _tr, locale, onNavigate, onConnectionChange, onProviderCountChange: _opc }: Props = $props();

  interface ProviderQuotaView {
    provider: Provider;
    accounts: ProviderUsageAccount[];
    quotas: ProviderUsageQuota[];
    plan: string;
    maxUsed: number | null;
    hasData: boolean;
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
  const avgUtil = $derived.by(() => {
    const values = reporting.flatMap((v) => v.quotas.map((q) => q.used_percent)).filter((n) => Number.isFinite(n));
    if (!values.length) return null;
    return Math.round(values.reduce((a, b) => a + b, 0) / values.length);
  });
  const nearLimit = $derived(reporting.filter((v) => v.maxUsed !== null && v.maxUsed >= 80));
  const missingKeys = $derived(views.filter((v) => v.provider.enabled && !(v.provider.api_key_count ?? 0)));
  const localProviders = $derived(views.filter((v) => isLocal(v.provider)));

  function isLocal(provider: Provider): boolean {
    const id = `${provider.adapter_id} ${provider.name} ${provider.base_url}`.toLowerCase();
    return id.includes('ollama') || id.includes('lmstudio') || id.includes('lm studio') || id.includes('local')
      || /^https?:\/\/(127\.|localhost|0\.0\.0\.0|\[::1\])/.test(provider.base_url);
  }

  function providerIcon(provider: Provider): string {
    const id = `${provider.adapter_id} ${provider.name}`.toLowerCase();
    if (id.includes('anthropic') || id.includes('claude')) return 'i-link';
    if (id.includes('google') || id.includes('gemini')) return 'i-zap';
    if (id.includes('openrouter')) return 'i-layers';
    if (isLocal(provider)) return 'i-cpu';
    return 'i-zap';
  }

  function quotaValue(quota: ProviderUsageQuota): string {
    if (quota.uncapped) return quota.used_amount != null ? `${formatAmount(quota.used_amount, quota.unit)} used` : 'uncapped';
    if (quota.used_amount != null && quota.limit_amount != null) {
      return `${formatAmount(quota.used_amount, quota.unit)} / ${formatAmount(quota.limit_amount, quota.unit)}`;
    }
    return `${Math.round(quota.used_percent)}% used`;
  }

  function formatAmount(value: number, unit?: string | null): string {
    const formatted = value >= 1000 ? formatCount(Math.round(value), locale) : value.toLocaleString(locale, { maximumFractionDigits: 1 });
    return unit ? `${formatted} ${unit}` : formatted;
  }

  function resetLabel(quota: ProviderUsageQuota): string | null {
    if (!quota.reset_at) return null;
    const diffSec = quota.reset_at - Date.now() / 1000;
    if (diffSec <= 0) return 'reset due';
    const h = Math.floor(diffSec / 3600);
    const m = Math.floor((diffSec % 3600) / 60);
    if (h > 24) return `resets in ${Math.floor(h / 24)}d`;
    if (h > 0) return `resets in ${h}h ${m}m`;
    return `resets in ${m}m`;
  }

  function statusClass(view: ProviderQuotaView): string {
    if (!view.hasData) return 'pending';
    if (view.maxUsed !== null && view.maxUsed >= 90) return 'error';
    if (view.maxUsed !== null && view.maxUsed >= 70) return 'progress';
    return 'complete';
  }

  async function load(): Promise<void> {
    const gen = ++generation;
    loading = true;
    errorMessage = '';
    onConnectionChange('loading');
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
          error: '',
        };
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
          };
        } catch (error) {
          return { ...base, error: error instanceof Error ? error.message : 'unavailable' };
        }
      }));
      if (gen !== generation) return;
      views = results;
      onConnectionChange('loaded');
    } catch (error) {
      if (gen !== generation) return;
      errorMessage = error instanceof Error ? error.message : 'Could not load quota data.';
      onConnectionChange('error');
    } finally {
      if (gen === generation) loading = false;
    }
  }

  async function refresh(): Promise<void> {
    if (refreshing) return;
    refreshing = true;
    showSketchToast('Refreshing quota…');
    try {
      await Promise.all(views.filter((v) => v.hasData || v.provider.api_key_count).map((v) =>
        api.refreshProviderUsage(v.provider.id).catch(() => null),
      ));
      await load();
      showSketchToast('Quota refreshed');
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
    <h1>Quota</h1>
    <p>Monitor provider quota and local usage from one place.</p>
  </div>
</div>

{#if errorMessage}
  <section class="course-card surface" aria-label="Load error">
    <div class="card-title-row"><h2>Could not load quota</h2></div>
    <p class="card-subtitle">{errorMessage}</p>
    <div class="modal-actions" style="padding-top:14px">
      <button class="page-action primary" type="button" onclick={load}>Retry</button>
    </div>
  </section>
{:else if loading}
  <section class="stats-grid" aria-label="Loading">
    {#each [0, 1, 2, 3] as i (i)}<article class="stat-card surface" style="min-height:132px"></article>{/each}
  </section>
{:else}
  <div class="page-surface">
    <section class="stats-grid" aria-label="Quota overview">
      <article class="stat-card surface">
        <div class="stat-top">
          <span class="stat-icon"><svg class="icon"><use href="#i-users" /></svg></span>
          <span class="stat-number">{views.length}</span>
        </div>
        <h2>Providers Monitored</h2>
        <div class="stat-trend">{reporting.length} reporting, {missingKeys.length} missing keys</div>
        <button class="details-link" type="button" onclick={() => onNavigate('providers')}>View providers <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top">
          <span class="stat-icon"><svg class="icon"><use href="#i-chart" /></svg></span>
          <span class="stat-number">{avgUtil === null ? '—' : `${avgUtil}%`}</span>
        </div>
        <h2>Avg Utilization</h2>
        <div class="stat-trend">Across reporting providers</div>
        <button class="details-link" type="button" onclick={() => document.getElementById('quota-board')?.scrollIntoView({ behavior: 'smooth' })}>See breakdown <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top">
          <span class="stat-icon"><svg class="icon"><use href="#i-alert" /></svg></span>
          <span class="stat-number">{nearLimit.length}</span>
        </div>
        <h2>Near Limit</h2>
        <div class="stat-trend">{nearLimit.length ? `${nearLimit[0].provider.name} · ${nearLimit[0].maxUsed}%` : 'All providers healthy'}</div>
        <button class="details-link" type="button" onclick={() => nearLimit[0] && openDetail(nearLimit[0])}>Inspect {nearLimit[0]?.provider.name ?? 'quota'} <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
      <article class="stat-card surface">
        <div class="stat-top">
          <span class="stat-icon"><svg class="icon"><use href="#i-cpu" /></svg></span>
          <span class="stat-number">{localProviders.length}</span>
        </div>
        <h2>Local Providers</h2>
        <div class="stat-trend">{localProviders.map((v) => v.provider.name).join(', ') || 'None configured'}</div>
        <button class="details-link" type="button" onclick={() => onNavigate('chat')}>Open chat <svg class="icon"><use href="#i-arrow-right" /></svg></button>
      </article>
    </section>

    <section class="course-card surface quota-board" id="quota-board" aria-labelledby="quota-heading">
      <div class="card-title-row">
        <h2 id="quota-heading">Quota by provider</h2>
        <button class="mini-add" type="button" onclick={refresh} disabled={refreshing}>
          <svg class="icon"><use href="#i-refresh" /></svg>
          {refreshing ? 'Refreshing…' : 'Refresh'}
        </button>
      </div>
      <p class="card-subtitle">Upstream quotas are informational — they never change provider routing.</p>
      <div class="quota-grid">
        {#each views as view (view.provider.id)}
          <article class="quota-card" class:warn={(view.maxUsed ?? 0) >= 90} id="quota-card-{view.provider.id}">
            <div class="quota-head">
              <span class="course-icon"><svg class="icon"><use href="#{providerIcon(view.provider)}" /></svg></span>
              <div class="course-copy">
                <strong>{view.provider.name}</strong>
                <span>{view.plan ? `${view.plan} plan` : view.provider.base_url}{view.quotas[0] ? ` · ${resetLabel(view.quotas[0]) ?? ''}` : ''}</span>
              </div>
              <span class="status {statusClass(view)}">{view.maxUsed === null ? '—' : `${view.maxUsed}%`}</span>
            </div>
            <div class="quota-meters">
              {#if view.quotas.length}
                {#each view.quotas.slice(0, 3) as quota, i (i)}
                  <div class="quota-meter">
                    <div class="quota-meter-top"><span>{quota.label}</span><strong>{quotaValue(quota)}</strong></div>
                    <div class="progress-track"><div class="progress-fill" class:blue={i === 1} class:green={i === 2} style="width:{Math.min(100, Math.max(0, quota.used_percent))}%"></div></div>
                  </div>
                {/each}
              {:else}
                <div class="quota-meter">
                  <div class="quota-meter-top">
                    <span>Status</span>
                    <strong>{view.error ? view.error : !(view.provider.api_key_count ?? 0) ? 'Add an API key to monitor quota' : 'No quota data reported'}</strong>
                  </div>
                  <div class="progress-track"><div class="progress-fill" style="width:0%"></div></div>
                </div>
              {/if}
            </div>
            <button class="details-link quota-detail" type="button" onclick={() => openDetail(view)}>View details <svg class="icon"><use href="#i-arrow-right" /></svg></button>
          </article>
        {:else}
          <div class="empty-state">No providers configured.</div>
        {/each}
      </div>
    </section>
  </div>
{/if}

<!-- Quota detail dialog -->
<dialog class="modal modal-wide" bind:this={detailDialog} aria-labelledby="quota-dialog-title">
  <div class="modal-card">
    <div class="modal-heading">
      <div>
        <h2 id="quota-dialog-title">{detailView?.provider.name ?? 'Provider'} quota</h2>
        <p>{detailView?.plan ? `${detailView.plan} plan` : detailView?.provider.base_url}</p>
      </div>
      <button class="modal-close" type="button" data-close-modal aria-label="Close dialog">×</button>
    </div>
    <div class="quota-detail-body">
      {#if detailView?.quotas.length}
        {#each detailView.quotas as quota, i (i)}
          <div class="quota-detail-row">
            <div class="quota-meter-top">
              <span>{quota.label}{resetLabel(quota) ? ` · ${resetLabel(quota)}` : ''}</span>
              <strong>{quotaValue(quota)}</strong>
            </div>
            <div class="progress-track">
              <div class="progress-fill" style="width:{Math.min(100, Math.max(0, quota.used_percent))}%"></div>
            </div>
          </div>
        {/each}
      {:else}
        <p class="card-subtitle">{detailView?.error ?? 'No quota data reported for this provider.'}</p>
      {/if}
      {#each detailView?.accounts ?? [] as account (account.key_id)}
        {#if account.local_meter}
          <div class="quota-detail-row">
            <div class="quota-meter-top"><span>Local usage · {account.name || 'key'}</span><strong>{Object.keys(account.local_meter.windows).length} windows</strong></div>
            {#each Object.entries(account.local_meter.windows) as [window, data] (window)}
              <div class="quota-meter-top" style="padding-left:14px">
                <span>{window} · {data.requests} requests · {formatCount(data.input_tokens + data.output_tokens, locale)} tokens</span>
                <strong>{data.cost_micro_usd != null ? `$${(data.cost_micro_usd / 1_000_000).toFixed(2)}` : '—'}</strong>
              </div>
            {/each}
          </div>
        {/if}
      {/each}
    </div>
    <p class="form-note">Quota values are informational and never change provider routing.</p>
    <div class="modal-actions">
      <button class="page-action" type="button" data-close-modal>Close</button>
    </div>
  </div>
</dialog>
