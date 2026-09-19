<script lang="ts">
  import { AlertTriangle, Gauge, LoaderCircle, RefreshCw, Save } from '@lucide/svelte';
  import { onDestroy } from 'svelte';
  import { api } from '../../lib/api';
  import { formatDate, type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { Locale } from '../../lib/i18n';
  import type { Provider, ProviderQuotaResult } from '../../lib/types';

  export let provider: Provider;
  export let tr: Translate;
  export let locale: Locale;
  export let onTargetSaved: (providerId: string, target: number) => void;

  type LoadState = 'idle' | 'loading' | 'ready' | 'error';

  let quota: ProviderQuotaResult | null = null;
  let loadState: LoadState = 'idle';
  let errorMessage = '';
  let saveError = '';
  let activeProviderId = '';
  let requestGeneration = 0;
  let inFlight = false;
  let savingProviderId = '';
  let saveGeneration = 0;
  let targetDraft = '40';
  let targetDirty = false;
  let pollTimer: ReturnType<typeof setInterval> | undefined;
  let abortController: AbortController | undefined;

  $: if (provider?.id && activeProviderId !== provider.id) startPolling(provider.id);
  $: saving = provider?.id === savingProviderId;

  function startPolling(providerId: string): void {
    stopPolling();
    activeProviderId = providerId;
    quota = null;
    loadState = 'idle';
    errorMessage = '';
    saveError = '';
    saveGeneration += 1;
    savingProviderId = '';
    targetDraft = String(provider.local_rpm_target ?? 40);
    targetDirty = false;
    void loadQuota(true);
    pollTimer = setInterval(() => void loadQuota(false), 5_000);
  }

  function stopPolling(): void {
    requestGeneration += 1;
    if (pollTimer) {
      clearInterval(pollTimer);
      pollTimer = undefined;
    }
    abortController?.abort();
    abortController = undefined;
    inFlight = false;
    activeProviderId = '';
  }

  async function loadQuota(initial: boolean): Promise<void> {
    if (!provider?.id || inFlight || provider.id !== activeProviderId || savingProviderId === provider.id) return;
    inFlight = true;
    const generation = ++requestGeneration;
    const controller = new AbortController();
    abortController = controller;
    if (initial || !quota) loadState = 'loading';
    try {
      const result = await api.providerQuota(provider.id, controller.signal);
      if (generation !== requestGeneration || provider.id !== activeProviderId) return;
      quota = result;
      loadState = 'ready';
      errorMessage = '';
      if (!targetDirty) targetDraft = String(result.target_rpm);
    } catch (error) {
      if (controller.signal.aborted || generation !== requestGeneration || provider.id !== activeProviderId) return;
      loadState = 'error';
      errorMessage = localizedError(error, 'Could not load local quota.', tr);
    } finally {
      if (generation === requestGeneration) {
        inFlight = false;
        if (abortController === controller) abortController = undefined;
      }
    }
  }

  async function saveTarget(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!provider?.id || savingProviderId === provider.id) return;
    const providerId = provider.id;
    const value = targetDraft.trim();
    if (!/^\d+$/.test(value)) {
      saveError = tr('RPM target must be a positive whole number.');
      return;
    }
    const target = Number(value);
    if (!Number.isSafeInteger(target) || target < 1 || target > 4_294_967_295) {
      saveError = tr('RPM target must be between 1 and 4,294,967,295.');
      return;
    }

    saveError = '';
    const generation = ++saveGeneration;
    savingProviderId = providerId;
    abortController?.abort();
    abortController = undefined;
    requestGeneration += 1;
    inFlight = false;
    let saved = false;
    try {
      const result = await api.updateProviderQuotaTarget(providerId, target);
      if (providerId !== activeProviderId || generation !== saveGeneration) return;
      targetDraft = String(result.local_rpm_target);
      targetDirty = false;
      if (quota) {
        const usedPercent = Math.min(100, (quota.requests_last_60_seconds / result.local_rpm_target) * 100);
        quota = {
          ...quota,
          target_rpm: result.local_rpm_target,
          remaining_percent: Math.max(0, 100 - usedPercent),
          used_percent: usedPercent,
        };
      }
      onTargetSaved(providerId, result.local_rpm_target);
      saved = true;
    } catch (error) {
      if (providerId === activeProviderId && generation === saveGeneration) {
        saveError = localizedError(error, 'Could not update local RPM target.', tr);
      }
    } finally {
      if (generation === saveGeneration) {
        savingProviderId = '';
        if (saved && providerId === activeProviderId) void loadQuota(false);
      }
    }
  }

  function safePercent(value: number): number {
    return Number.isFinite(value) ? Math.min(100, Math.max(0, value)) : 0;
  }

  function statusLabel(status: ProviderQuotaResult['status']): string {
    const statusKey = ({ ready: 'Tracking active', warming_up: 'Warming up', capacity_limited: 'Tracking capacity limited' })[status];
    return statusKey ? tr(statusKey) : status;
  }

  function sampleTime(value: number): string {
    if (!Number.isFinite(value)) return tr('Not available');
    return formatDate(new Date(value).toISOString(), locale);
  }

  onDestroy(stopPolling);
</script>

<section class="provider-quota-panel quota-local-panel" aria-labelledby="local-quota-title">
  <div class="provider-quota-heading">
    <div>
      <h3 id="local-quota-title"><Gauge size={16} />{tr('Local RPM tracking')}</h3>
      <p>{tr('ExoRoute local estimate across every key and inference attempt.')}</p>
    </div>
    {#if loadState === 'loading'}<LoaderCircle size={15} class="spin" aria-label={tr('Loading local quota…')} />{:else}<button type="button" class="row-icon" aria-label={tr('Refresh local quota')} title={tr('Refresh local quota')} onclick={() => void loadQuota(false)} disabled={inFlight}><RefreshCw size={14} /></button>{/if}
  </div>

  {#if loadState === 'loading' && !quota}
    <div class="provider-quota-state"><LoaderCircle size={15} class="spin" /><span>{tr('Loading local quota…')}</span></div>
  {:else if loadState === 'error' && !quota}
    <div class="provider-quota-state error" role="alert"><AlertTriangle size={15} /><span>{errorMessage}</span><button type="button" class="secondary-button compact" onclick={() => void loadQuota(true)}>{tr('Retry')}</button></div>
  {:else if quota}
    {@const remainingPercent = safePercent(quota.remaining_percent)}
    {@const quotaExhausted = remainingPercent <= 0}
    {@const overTarget = quota.requests_last_60_seconds > quota.target_rpm}
    {#if loadState === 'error'}
      <div class="provider-quota-state error" role="alert"><AlertTriangle size={15} /><span>{errorMessage}</span><button type="button" class="secondary-button compact" onclick={() => void loadQuota(true)}>{tr('Retry')}</button></div>
    {/if}
    <div class="provider-quota-summary">
      <div><strong>{quota.requests_last_60_seconds}</strong><span>{tr('requests in the last 60 seconds')}</span></div>
      <div class="provider-quota-target"><span>{tr('Target')}</span><strong>{quota.target_rpm} RPM</strong></div>
      <div class="provider-quota-target"><span>{tr('remaining')}</span><strong>{Math.round(remainingPercent)}%</strong></div>
    </div>
    <div class="provider-quota-track" role="progressbar" aria-label={`${tr('Local RPM tracking')} — ${Math.round(remainingPercent)}% ${tr('remaining')}`} aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(remainingPercent)}><span class:over={quotaExhausted || overTarget} style={`width: ${remainingPercent}%`}></span></div>
    <div class="provider-quota-meta"><span class:over={quotaExhausted || overTarget}>{quotaExhausted ? tr('Quota exhausted') : overTarget ? tr('Over target') : tr('Within target')}</span><span>{statusLabel(quota.status)}</span></div>
    {#if quota.status === 'warming_up'}<p class="provider-quota-note warmup">{tr('Tracking is warming up after restart ({seconds}/60 seconds covered).', { seconds: quota.coverage_seconds })}</p>
    {:else if quota.status === 'capacity_limited'}<p class="provider-quota-note error">{tr('Tracking capacity is full; this value may be incomplete.')}</p>{/if}
    <div class="provider-quota-source"><span>{tr('NVIDIA quota')}</span><strong>{tr('Not documented by NVIDIA')}</strong></div>
    <small class="provider-quota-sampled">{tr('Sampled {time}', { time: sampleTime(quota.sampled_at_ms) })}</small>
  {/if}

  <form class="quota-target-form" onsubmit={saveTarget}>
    <label>{tr('Local RPM target')}
      <input type="number" min="1" max="4294967295" step="1" required bind:value={targetDraft} oninput={() => { targetDirty = true; saveError = ''; }} />
    </label>
    <p>{tr('Display-only target for the ExoRoute local request estimate. It does not throttle or route requests.')}</p>
    {#if saveError}<small class="provider-usage-error" role="alert">{saveError}</small>{/if}
    <button type="submit" class="primary-button compact" disabled={saving || !targetDirty}>
      {#if saving}<LoaderCircle size={13} class="spin" />{:else}<Save size={13} />{/if}{tr('Save RPM target')}
    </button>
  </form>
</section>
