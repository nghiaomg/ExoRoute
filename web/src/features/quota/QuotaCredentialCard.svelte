<script lang="ts">
  import {
    AlertTriangle,
    Clock,
    KeyRound,
    LoaderCircle,
    Power,
    PowerOff,
    Sparkles,
    Zap,
  } from '@lucide/svelte';
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import { api } from '../../lib/api';
  import { formatDate, type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { ProviderKey, ProviderUsageAccount, ProviderUsageQuota, ProviderUsageSnapshot } from '../../lib/types';
  import UsageBudgetEditor from './UsageBudgetEditor.svelte';

  export let providerId: string;
  export let key: ProviderKey;
  export let account: ProviderUsageAccount | undefined;
  export let locale: Locale;
  export let tr: Translate;
  export let onBudgetChanged: () => void;
  export let onStatusChanged: () => void;
  export let onToast: ((tone: 'success' | 'error', title: string, message: string) => void) | undefined = undefined;

  $: snapshot = account?.provider_quota?.snapshot ?? account?.snapshot ?? null;
  $: displayName = account?.name || key.name;
  $: hasSeparateAccountName = Boolean(account?.name && key.name && account.name !== key.name);

  let toggling = false;
  let toggleError = '';

  async function toggleStatus(): Promise<void> {
    if (toggling) return;
    const enabled = !key.enabled;
    toggling = true;
    toggleError = '';
    try {
      const result = await api.updateProviderKey(providerId, key.id, { enabled });
      key = { ...key, enabled: result.enabled };
      onStatusChanged();
      const title = tr(result.enabled ? 'Key enabled' : 'Key disabled');
      const message = tr(
        result.enabled
          ? 'Key “{name}” is now enabled.'
          : 'Key “{name}” is now disabled.',
        { name: displayName },
      );
      onToast?.('success', title, message);
    } catch (error) {
      const errorMsg = localizedError(error, 'Could not update this provider key.', tr);
      toggleError = errorMsg;
      onToast?.('error', tr('Could not update key'), errorMsg);
    } finally {
      toggling = false;
    }
  }

  function safePercent(value: number): number {
    return Number.isFinite(value) ? Math.min(100, Math.max(0, value)) : 0;
  }

  function usageStatusLabel(status: ProviderUsageAccount['status']): string {
    const labels: Record<ProviderUsageAccount['status'], string> = {
      fresh: 'Up to date',
      stale: 'Usage data is stale',
      partial: 'Partial data',
      unknown: 'No usage data available yet.',
      unavailable: 'Usage unavailable',
      reauth_required: 'Reconnect required',
      disabled: 'Account disabled',
    };
    return tr(labels[status] ?? status);
  }

  function localMeterStatusLabel(status: NonNullable<ProviderUsageAccount['local_meter']>['status']): string {
    const labels: Record<NonNullable<ProviderUsageAccount['local_meter']>['status'], string> = {
      fresh: 'Up to date',
      stale: 'Usage data is stale',
      partial: 'Partial data',
      unknown: 'No cost data yet',
    };
    return tr(labels[status] ?? status);
  }

  function quotaLabel(quota: ProviderUsageQuota): string {
    const labels: Record<string, string> = {
      five_hour: providerId === 'antigravity' ? '5h' : '5-hour window',
      session: 'Session',
      weekly: providerId === 'antigravity' ? '7d' : 'Weekly',
      gpt: 'GPT',
      claude: 'Claude',
      monthly: 'Monthly',
      code_review_session: 'Code review · Session',
      code_review_weekly: 'Code review · Weekly',
      code_review_monthly: 'Code review · Monthly',
      spark_session: 'Spark · Session',
      spark_weekly: 'Spark · Weekly',
      spark_monthly: 'Spark · Monthly',
      openrouter_key_spending_cap: 'API key spending cap',
      gemini_weekly: 'Gemini · Weekly',
      claude_gpt_weekly: 'Claude & GPT · Weekly',
    };
    return tr(labels[quota.id] ?? quota.label);
  }

  function formatAmount(value: number | null | undefined): string {
    if (value === null || value === undefined || !Number.isFinite(value)) return '0';
    return new Intl.NumberFormat(getIntlLocale(locale), { maximumFractionDigits: 2 }).format(value);
  }

  function formatTimestamp(seconds: number): string {
    if (!Number.isFinite(seconds) || seconds < 0) return tr('Not available');
    return formatDate(new Date(seconds * 1000).toISOString(), locale);
  }

  function formatResetTime(seconds?: number | null, period?: string | null): string {
    if (period) {
      const periodMap: Record<string, string> = {
        daily: 'Resets daily',
        weekly: 'Resets weekly',
        monthly: 'Resets monthly',
      };
      return tr(periodMap[period] ?? `Resets ${period}`);
    }
    if (!seconds || !Number.isFinite(seconds) || seconds <= 0) return '';
    const now = Date.now();
    const diffMs = seconds * 1000 - now;
    if (diffMs > 0 && diffMs < 24 * 3600 * 1000) {
      const hours = Math.floor(diffMs / (3600 * 1000));
      const minutes = Math.floor((diffMs % (3600 * 1000)) / (60 * 1000));
      if (hours > 0) {
        return tr('Resets in {hours}h {minutes}m', { hours, minutes });
      }
      return tr('Resets in {minutes}m', { minutes: Math.max(1, minutes) });
    }
    return tr('Resets {time}', { time: formatDate(new Date(seconds * 1000).toISOString(), locale) });
  }

  function getDisplayQuotas(snap: ProviderUsageSnapshot | null): ProviderUsageQuota[] {
    if (!snap) return [];
    const list: ProviderUsageQuota[] = [];
    const seen = new Set<string>();
    for (const quota of snap.quotas ?? []) {
      // Upstream aliases can normalize to the same model ID. Keep the first
      // row so the keyed each block remains stable even if the API is stale.
      if (seen.has(quota.id)) continue;
      seen.add(quota.id);
      list.push(quota);
    }
    if (!list.some((q) => q.id === 'monthly') && snap.credit_balance) {
      const cb = snap.credit_balance;
      if (
        cb.period_used !== undefined && cb.period_used !== null &&
        cb.monthly_remaining !== undefined && cb.monthly_remaining !== null
      ) {
        const total = cb.period_used + cb.monthly_remaining;
        if (total > 0) {
          const usedPct = (cb.period_used / total) * 100;
          const remainingPct = safePercent(100 - usedPct);
          list.push({
            id: 'monthly',
            label: 'Monthly',
            used_percent: usedPct,
            remaining_percent: remainingPct,
            used_amount: cb.period_used,
            limit_amount: total,
            unit: cb.unit ?? 'credits',
            reset_at: cb.period_ends_at,
            uncapped: false,
          });
        }
      }
    }
    return list;
  }

  function isAccountLimitReached(snap: ProviderUsageSnapshot | null): boolean {
    if (!snap) return false;
    const quotasToCheck = getDisplayQuotas(snap);
    if (quotasToCheck.length > 0) {
      const anyQuotaSaturated = quotasToCheck.some(
        (q: ProviderUsageQuota) => !q.uncapped && safePercent(q.remaining_percent) <= 0
          || (q.limit_amount != null && q.limit_amount > 0 && (q.used_amount ?? 0) >= q.limit_amount)
      );
      if (anyQuotaSaturated) return true;

      if (snap.credit_balance) {
        const cb = snap.credit_balance;
        const hasCredits = (cb.monthly_remaining ?? 0) > 0 || (cb.purchased_remaining ?? 0) > 0 || (cb.free_remaining ?? 0) > 0;
        if (hasCredits) {
          return false;
        }
        if (cb.monthly_remaining !== undefined && cb.monthly_remaining !== null && cb.monthly_remaining <= 0) {
          return true;
        }
      }

      return false;
    }

    if (snap.credit_balance) {
      const cb = snap.credit_balance;
      if (cb.monthly_remaining !== undefined && cb.monthly_remaining !== null && cb.monthly_remaining <= 0 && (cb.purchased_remaining ?? 0) <= 0 && (cb.free_remaining ?? 0) <= 0) {
        return true;
      }
    }

    return Boolean(snap.limit_reached);
  }

  $: displayQuotas = getDisplayQuotas(snapshot);
  $: limitReached = isAccountLimitReached(snapshot);
</script>

<article class="quota-credential-card" class:disabled={!key.enabled} class:limit-reached={limitReached}>
  <header class="quota-card-header">
    <div class="quota-card-identity">
      <div class="quota-card-avatar" aria-hidden="true">
        <KeyRound size={15} />
      </div>
      <div class="quota-card-titles">
        <div class="quota-card-title-row">
          <h3 class="quota-account-name" title={displayName}>{displayName}</h3>
        </div>
        <div class="quota-card-sub-row">
          {#if snapshot?.plan}
            <span class="quota-plan-badge">
              <Zap size={10} />
              <span>{snapshot.plan}</span>
            </span>
          {/if}
          {#if hasSeparateAccountName}
            <span class="quota-key-ref" title={key.name}>{key.name}</span>
            <span class="quota-dot-sep">·</span>
          {/if}
          <span class="quota-type-tag">{tr(key.credential_type === 'oauth' ? 'OAuth' : 'API key')}</span>
        </div>
      </div>
    </div>

    <div class="quota-header-badges">
      {#if account}
        <span class="quota-status-dot-wrap" title={usageStatusLabel(account.status)}>
          <span
            class="quota-status-dot"
            class:fresh={account.status === 'fresh'}
            class:stale={account.status === 'stale'}
            class:invalid={!key.enabled || account.status === 'reauth_required' || account.status === 'unavailable' || account.status === 'disabled'}
          ></span>
        </span>
      {/if}
      <button
        type="button"
        class="quota-compact-toggle-btn"
        class:enabled={key.enabled}
        disabled={toggling}
        onclick={() => { void toggleStatus(); }}
        aria-label={tr(key.enabled ? 'Disable key “{name}”' : 'Enable key “{name}”', { name: displayName })}
        title={tr(key.enabled ? 'Disable' : 'Enable')}
      >
        {#if toggling}
          <LoaderCircle size={12} class="spin" />
        {:else if key.enabled}
          <PowerOff size={12} />
        {:else}
          <Power size={12} />
        {/if}
      </button>
    </div>
  </header>

  {#if toggleError}
    <p class="provider-usage-request-error" role="alert">{toggleError}</p>
  {/if}

  {#if limitReached}
    <div class="quota-compact-alert" role="alert">
      <AlertTriangle size={13} class="quota-alert-icon" />
      <span>{tr('Usage limit reached')}</span>
    </div>
  {/if}

  <div class="quota-card-body">
    {#if !account}
      <div class="quota-empty-note">
        <Clock size={13} />
        <span>{tr('No usage data available yet.')}</span>
      </div>
    {:else if account.provider_quota?.status === 'unsupported'}
      <div class="quota-empty-note">
        <span>{tr(account.provider_quota.message ?? 'Upstream quota is not available for this provider.')}</span>
      </div>
    {:else if account.message && !snapshot}
      <div
        class="quota-empty-note"
        class:error={account.status === 'reauth_required' || account.status === 'unavailable'}
      >
        <span>{tr(account.message)}</span>
      </div>
    {:else if snapshot}
      {#if snapshot.reset_credits_available !== undefined && snapshot.reset_credits_available !== null}
        <div class="quota-compact-chip">
          <Sparkles size={11} />
          <span>{tr('Reset credits: {count}', { count: snapshot.reset_credits_available })}</span>
        </div>
      {/if}

      {#if snapshot.credit_balance}
        <div class="quota-compact-balance">
          <div class="quota-balance-row">
            <span class="quota-balance-label">{tr('Monthly credits')}</span>
            {#if snapshot.credit_balance.period_ends_at}
              <span class="quota-balance-expiry" title={formatTimestamp(snapshot.credit_balance.period_ends_at)}>
                <Clock size={10} />
                <span>{formatTimestamp(snapshot.credit_balance.period_ends_at)}</span>
              </span>
            {/if}
          </div>
          <div class="quota-balance-val-row">
            <strong class="quota-balance-main">{formatAmount(snapshot.credit_balance.monthly_remaining)}</strong>
            {#if snapshot.credit_balance.unit}
              <span class="quota-balance-unit">{snapshot.credit_balance.unit}</span>
            {/if}
            {#if snapshot.credit_balance.period_used !== undefined && snapshot.credit_balance.period_used !== null}
              <span class="quota-balance-used">({formatAmount(snapshot.credit_balance.period_used)} {tr('used')})</span>
            {/if}
          </div>
          {#if snapshot.credit_balance.purchased_remaining || snapshot.credit_balance.free_remaining}
            <div class="quota-balance-extra">
              {#if snapshot.credit_balance.purchased_remaining}
                <span class="quota-mini-chip">{formatAmount(snapshot.credit_balance.purchased_remaining)} {tr('purchased')}</span>
              {/if}
              {#if snapshot.credit_balance.free_remaining}
                <span class="quota-mini-chip">{formatAmount(snapshot.credit_balance.free_remaining)} {tr('free')}</span>
              {/if}
            </div>
          {/if}
        </div>
      {/if}

      {#if displayQuotas.length}
        <div class="quota-compact-windows">
          {#each displayQuotas as quota (quota.id)}
            {@const remainingPct = safePercent(quota.remaining_percent)}
            {@const isHigh = remainingPct <= 30 && remainingPct > 10}
            {@const isCritical = remainingPct <= 10}
            {@const resetStr = formatResetTime(quota.reset_at, quota.reset_period)}
            <div class="quota-window-item">
              <div class="quota-window-header">
                <span class="quota-window-title" title={quotaLabel(quota)}>{quotaLabel(quota)}</span>
                <span class="quota-window-stats">
                  {#if quota.uncapped}
                    <span class="quota-uncapped-badge">{tr('Uncapped')}</span>
                  {:else if quota.used_amount !== undefined && quota.used_amount !== null && quota.limit_amount !== undefined && quota.limit_amount !== null}
                    <span class="quota-window-counts"><strong>{formatAmount(quota.used_amount)}</strong>/{formatAmount(quota.limit_amount)}</span>
                    <span class="quota-window-percent" class:warning={isHigh} class:danger={isCritical}>({Math.round(remainingPct)}%) {tr('remaining')}</span>
                  {:else}
                    <span class="quota-window-percent" class:warning={isHigh} class:danger={isCritical}><strong>{Math.round(safePercent(quota.remaining_percent))}%</strong> {tr('remaining')}</span>
                  {/if}
                </span>
              </div>
              {#if !quota.uncapped}
                <div
                  class="quota-progress-track"
                  role="progressbar"
                  aria-label={quotaLabel(quota)}
                  aria-valuemin="0"
                  aria-valuemax="100"
                  aria-valuenow={Math.round(remainingPct)}
                >
                  <div
                    class="quota-progress-fill"
                    class:warning={isHigh}
                    class:danger={isCritical}
                    style="width: {remainingPct}%"
                  ></div>
                </div>
              {/if}
              {#if resetStr}
                <div class="quota-window-reset">
                  <Clock size={10} />
                  <span>{resetStr}</span>
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    {:else}
      <div class="quota-empty-note">
        <span>{tr('No usage data available yet.')}</span>
      </div>
    {/if}

    {#if snapshot && account?.message}
      <p class="provider-usage-note" class:provider-usage-error={account.status === 'reauth_required' || account.status === 'unavailable'}>
        {tr(account.message)}
      </p>
    {/if}

    {#if account?.local_meter}
      <UsageBudgetEditor {providerId} {key} {tr} onSaved={onBudgetChanged} />
    {/if}
  </div>

  <footer class="quota-card-compact-footer">
    {#if account?.fetched_at_ms}
      <span class="quota-footer-updated" title={formatDate(new Date(account.fetched_at_ms).toISOString(), locale)}>
        <Clock size={10} />
        <span>{formatDate(new Date(account.fetched_at_ms).toISOString(), locale)}</span>
      </span>
    {:else}
      <span></span>
    {/if}
    {#if !key.enabled}
      <span class="quota-footer-disabled-tag">{tr('Disabled')}</span>
    {/if}
  </footer>
</article>
