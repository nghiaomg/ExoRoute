<script lang="ts">
  import Dialog from '../../components/ui/Dialog.svelte';
  import Button from '../../components/ui/Button.svelte';
  import { formatCount, formatDate, formatTokenCount } from '../../lib/format';
  import type { Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import type { DashboardPage } from '../../lib/navigation';
  import type { RequestLog } from '../../lib/types';
  import { filterRequests, type RequestFilter } from './overview.data';
  interface Props { tr: Translate; locale: Locale; requests: RequestLog[]; totalRequests?: number; onNavigate: (page: DashboardPage) => void }
  let { tr, locale, requests, totalRequests = requests.length, onNavigate }: Props = $props();
  let status = $state<RequestFilter>('all');
  let query = $state('');
  let selected = $state<RequestLog | null>(null);
  const visible = $derived(filterRequests(requests, status, query));
  /** Unique per rendered row even if several logs omit ids and share created_at. */
  const rowKey = (request: RequestLog, index: number): string =>
    `${request.id ?? request.request_id ?? ''}:${request.created_at}:${index}`;
  const statusText = (r: RequestLog) => r.status ? String(r.status) : '…';
  const outcome = (r: RequestLog) => r.status != null && r.status >= 200 && r.status < 300 ? 'complete' : 'error';
  const isCached = (r: RequestLog) => (r.cached_tokens ?? 0) > 0;
  const latency = (r: RequestLog) => r.duration_ms == null ? '—' : r.duration_ms + ' ms';
</script>
<section class="recent-card" aria-label={tr('Recent Requests')}>
  <div class="card-title-row"><h2>{tr('Recent Requests')}</h2><Button class="mini-add" type="button" onclick={() => onNavigate('requests')}>{tr('View all')}</Button></div>
  <div class="recent-filters" role="group" aria-label={tr('Filter by status')}>
    {#each [{ key: 'all', label: tr('All') }, { key: 'success', label: tr('Success') }, { key: 'errors', label: tr('Errors') }] as pill (pill.key)}
      <button type="button" class:active={status === pill.key} onclick={() => (status = pill.key as RequestFilter)}>{pill.label}</button>
    {/each}
    <input type="search" bind:value={query} aria-label={tr('Filter by model')} placeholder={tr('Filter by model')} />
  </div>
  <div class="recent-list" aria-live="polite">
    {#if !visible.length}<p class="card-subtitle">{totalRequests === 0 ? tr('No requests recorded yet') : tr('No requests match your filter')}</p>{/if}
    {#each visible as request, index (rowKey(request, index))}
      <button type="button" class="recent-row" onclick={() => (selected = request)} aria-label={tr('Inspect request')}>
        <span class="status {outcome(request)}">{statusText(request)}</span>
        <span class="recent-main"><strong>{request.model}</strong><span>{request.provider_id ?? '—'}{request.route_alias ? ' · ' + request.route_alias : ''}</span></span>
        <span class="recent-meta">
          {#if isCached(request)}<span class="cache-badge">{tr('Cache hit')}</span>{/if}
          <span title={tr('Tokens')}>{formatTokenCount(request.input_tokens, locale)} / {formatTokenCount(request.output_tokens, locale)}</span>
          <span>{latency(request)}</span>
        </span>
      </button>
    {/each}
  </div>
</section>
<Dialog open={selected !== null} title={tr('Request details')} closeLabel={tr('Close')} class="request-drawer-card" onClose={() => (selected = null)}>
  {#if selected}
    <dl class="request-drawer-fields">
      <div><dt>{tr('Status')}</dt><dd>{statusText(selected)}</dd></div>
      <div><dt>{tr('Time')}</dt><dd>{formatDate(selected.created_at, locale)}</dd></div>
      <div><dt>{tr('Duration')}</dt><dd>{latency(selected)}</dd></div>
      <div><dt>{tr('Model')}</dt><dd>{selected.model}</dd></div>
      <div><dt>{tr('Provider')}</dt><dd>{selected.provider_id ?? '—'}</dd></div>
      <div><dt>{tr('Combo alias')}</dt><dd>{selected.route_alias ?? '—'}</dd></div>
      <div><dt>{tr('Client protocol')}</dt><dd>{selected.client_protocol ?? '—'}</dd></div>
      <div><dt>{tr('Upstream protocol')}</dt><dd>{selected.upstream_protocol ?? '—'}</dd></div>
      <div><dt>{tr('Input tokens')}</dt><dd>{formatCount(selected.input_tokens, locale)}</dd></div>
      <div><dt>{tr('Output tokens')}</dt><dd>{formatCount(selected.output_tokens, locale)}</dd></div>
      <div><dt>{tr('Cached tokens')}</dt><dd>{formatCount(selected.cached_tokens, locale)}</dd></div>
      <div><dt>{tr('Cost')}</dt><dd>{selected.cost_micro_usd == null ? '—' : '$' + (selected.cost_micro_usd / 1e6).toFixed(6)}</dd></div>
      <div><dt>{tr('Request ID')}</dt><dd>{selected.request_id ?? selected.id ?? '—'}</dd></div>
    </dl>
    {#if selected.error}<section class="request-drawer-error"><h3>{tr('Error')}</h3><p>{selected.error}</p></section>{/if}
  {/if}
</Dialog>
