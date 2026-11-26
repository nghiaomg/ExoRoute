<script lang="ts">
  import { onMount } from 'svelte';
  import Button from '../../components/ui/Button.svelte';
  import { keyPortalApi, KeyPortalError, type KeyPortalRange, type KeyPortalSession, type KeyPortalStats } from '../../lib/api/keyPortal';
  import { formatCostMicroUsd, formatCount, type Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';

  export let tr: Translate;
  export let locale: Locale = 'en';
  export let session: KeyPortalSession;
  export let onSignOut: () => void;

  const ranges: KeyPortalRange[] = ['24h', '7d', '30d'];
  let range: KeyPortalRange = '24h';
  let stats: KeyPortalStats | null = null;
  let loading = false;
  let errorMessage = '';

  async function load(): Promise<void> {
    loading = true;
    errorMessage = '';
    try {
      stats = await keyPortalApi.stats(range);
    } catch (error) {
      errorMessage = error instanceof KeyPortalError
        ? error.message
        : tr('Could not load key statistics. Try again.');
    } finally {
      loading = false;
    }
  }

  function pickRange(next: KeyPortalRange): void {
    if (next === range) return;
    range = next;
    void load();
  }

  function statusTone(status: number | null): string {
    if (status === null) return 'text-muted border-line';
    if (status >= 500) return 'text-chart-critical border-chart-critical';
    if (status >= 400) return 'text-accent-strong border-accent';
    return 'text-chart-good border-chart-good';
  }

  onMount(() => { void load(); });
</script>

<div class="page-heading">
  <div class="heading-copy">
    <h1>{session.key.name}</h1>
    <p>{tr('Usage and statistics for this API key.')}</p>
    <div class="mt-2 flex flex-wrap items-center gap-2">
      <code class="rounded border border-line bg-surface-sunken px-2 py-0.5 font-mono text-xs text-muted">{session.key.prefix}</code>
      <span class="rounded-full border border-chart-good px-2 py-0.5 text-xs font-semibold text-chart-good">{session.key.enabled ? tr('Active') : tr('Paused')}</span>
      {#each session.key.allowed_providers ?? [] as provider (provider)}<span class="rounded-full border border-line bg-surface-sunken px-2 py-0.5 text-xs text-muted">{provider}</span>{/each}
      {#each session.key.allowed_models ?? [] as model (model)}<span class="rounded-full border border-line bg-surface-sunken px-2 py-0.5 text-xs text-muted">{model}</span>{/each}
    </div>
  </div>
  <div class="heading-actions">
    <div class="inline-flex gap-1 rounded-lg border border-line bg-surface-sunken p-1" role="group" aria-label={tr('Time range')}>
      {#each ranges as option (option)}
        <button type="button" aria-pressed={range === option} class="rounded-md px-2.5 py-1 text-xs font-semibold {range === option ? 'bg-accent text-accent-contrast' : 'bg-transparent text-muted'}" onclick={() => pickRange(option)}>{tr(option)}</button>
      {/each}
    </div>
    <Button class="action-button" type="button" onclick={load} disabled={loading}>{tr('Refresh')}</Button>
    <Button class="action-button" type="button" onclick={onSignOut}>{tr('Sign out')}</Button>
  </div>
</div>

{#if errorMessage}
  <section class="course-card surface" role="alert"><p class="card-subtitle">{errorMessage}</p><div class="modal-actions"><Button class="page-action" type="button" onclick={load}>{tr('Retry')}</Button></div></section>
{:else if loading && !stats}
  <section class="grid grid-cols-2 gap-3 max-nav:grid-cols-1" aria-label={tr('Loading')}><div class="h-24 rounded-lg border border-line bg-surface"></div><div class="h-24 rounded-lg border border-line bg-surface"></div></section>
{:else if stats}
  <div class="page-surface flex flex-col gap-3">
    <section class="grid grid-cols-5 gap-2.5 max-[1100px]:grid-cols-3 max-[640px]:grid-cols-1" aria-label={tr('Key statistics')}>
      <div class="rounded-lg border border-line bg-surface p-3"><p class="m-0 text-xs text-muted">{tr('Total Requests')}</p><strong class="block text-xl tabular-nums text-ink">{formatCount(stats.summary.total_requests, locale)}</strong></div>
      <div class="rounded-lg border border-line bg-surface p-3"><p class="m-0 text-xs text-muted">{tr('Success Rate')}</p><strong class="block text-xl tabular-nums text-ink">{Math.round(stats.summary.success_rate * 1000) / 10}%</strong>{#if stats.summary.failed_requests > 0}<small class="text-xs tabular-nums text-accent-strong">{formatCount(stats.summary.failed_requests, locale)} {tr('failed')}</small>{/if}</div>
      <div class="rounded-lg border border-line bg-surface p-3"><p class="m-0 text-xs text-muted">{tr('Total Tokens')}</p><strong class="block text-xl tabular-nums text-ink">{formatCount(stats.summary.total_tokens, locale)}</strong><small class="text-xs tabular-nums text-muted">{tr('In')} {formatCount(stats.summary.input_tokens, locale)} · {tr('Out')} {formatCount(stats.summary.output_tokens, locale)} · {tr('Cached')} {formatCount(stats.summary.cached_tokens, locale)}</small></div>
      <div class="rounded-lg border border-line bg-surface p-3"><p class="m-0 text-xs text-muted">{tr('Estimated Cost')}</p><strong class="block text-xl tabular-nums text-ink">{formatCostMicroUsd(stats.summary.total_cost_micro_usd)}</strong></div>
      <div class="rounded-lg border border-line bg-surface p-3"><p class="m-0 text-xs text-muted">{tr('Average Latency')}</p><strong class="block text-xl tabular-nums text-ink">{Math.round(stats.summary.avg_latency_ms)}</strong><small class="text-xs text-muted">ms</small></div>
    </section>

    <div class="grid grid-cols-2 gap-3 max-[900px]:grid-cols-1">
      <section class="rounded-lg border border-line bg-surface" aria-labelledby="portal-models">
        <h2 id="portal-models" class="m-0 border-b border-line px-3 py-2.5 text-sm font-semibold text-ink">{tr('Model breakdown')}</h2>
        {#if stats.models.length}
          <div class="grid">
            {#each stats.models as row (row.model)}
              <div class="grid grid-cols-[minmax(0,1fr)_auto] gap-2 border-b border-line px-3 py-2 last:border-b-0">
                <span class="min-w-0 truncate text-xs text-ink">{row.model}</span>
                <span class="tabular-nums text-xs text-muted">{formatCount(row.requests, locale)} · {formatCount(row.total_tokens, locale)}</span>
              </div>
            {/each}
          </div>
        {:else}
          <p class="m-0 px-3 py-6 text-center text-xs text-muted">{tr('No usage recorded yet.')}</p>
        {/if}
      </section>

      <section class="rounded-lg border border-line bg-surface" aria-labelledby="portal-requests">
        <h2 id="portal-requests" class="m-0 border-b border-line px-3 py-2.5 text-sm font-semibold text-ink">{tr('Recent requests')}</h2>
        {#if stats.recent_requests.length}
          <div class="grid">
            {#each stats.recent_requests as row (row.id)}
              <div class="grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2 border-b border-line px-3 py-2 last:border-b-0">
                <span class="rounded-full border px-1.5 py-0.5 text-xs font-semibold tabular-nums {statusTone(row.status_code)}">{row.status_code}</span>
                <span class="min-w-0 truncate text-xs text-ink">{row.model}</span>
                <span class="tabular-nums text-xs text-muted">{Math.round(row.latency_ms)} ms</span>
              </div>
            {/each}
          </div>
        {:else}
          <p class="m-0 px-3 py-6 text-center text-xs text-muted">{tr('No requests yet.')}</p>
        {/if}
      </section>
    </div>
  </div>
{/if}
