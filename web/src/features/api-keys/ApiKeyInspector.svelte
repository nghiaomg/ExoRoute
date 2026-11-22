<script lang="ts">
  import Dialog from '../../components/ui/Dialog.svelte';
  import Button from '../../components/ui/Button.svelte';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import { formatCount, formatCostMicroUsd } from '../../lib/format';
  import type { Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import type { DashboardPage } from '../../lib/navigation';
  import { submitTopSearch } from '../../lib/dashboard-ui';
  import type { GatewayApiKey } from '../../lib/types';
  import { formatUtcTimestamp } from './format-utc';
  import { connectSnippet, EMPTY_METRICS, formatIdleLabel, GATEWAY_BASE_URL, KEY_PLACEHOLDER, type KeyMetrics, type SnippetKind } from './api-keys.data';
  interface Props { tr: Translate; locale: Locale; apiKey: GatewayApiKey | null; metrics: KeyMetrics; onClose: () => void; onNavigate: (page: DashboardPage) => void }
  let { tr, locale, apiKey, metrics, onClose, onNavigate }: Props = $props();
  let snippet = $state<'curl' | 'openai' | 'anthropic'>('curl');
  const model = $derived(apiKey?.allowed_models?.[0]?.replace(/\*$/, '') || 'gpt-4o-mini');
  const code = $derived(connectSnippet(snippet, model));
  const idleText = $derived(apiKey ? formatIdleLabel(apiKey, tr) : '');
  function viewRequests(): void {
    if (!apiKey) return;
    submitTopSearch(apiKey.name);
    onNavigate('requests');
    onClose();
  }
  async function copy(text: string, message: string): Promise<void> {
    try { await navigator.clipboard.writeText(text); showDashboardToast(message); }
    catch { showDashboardToast(tr('Snippet copy failed')); }
  }
</script>
<Dialog open={apiKey !== null} title={tr('Key details')} closeLabel={tr('Close dialog')} class="request-drawer-card" onClose={onClose}>
  {#if apiKey}
    <div class="inspector">
      <div class="inspector-head"><strong>{apiKey.name}</strong><code title={apiKey.id}>{apiKey.id.slice(0, 12)}…</code></div>
      {#if apiKey.scope_valid === false}<p class="inspector-alert" role="alert">{tr('The stored access scope is invalid, so every request with this key is rejected. Save a scope to repair it.')}</p>{/if}
      <div class="inspector-grid">
        <div><span>{tr('Status')}</span><strong>{apiKey.enabled ? tr('Active') : tr('Paused keys')}</strong></div>
        <div><span>{tr('Requests')}</span><strong>{metrics.loaded ? formatCount(metrics.requests, locale) : '—'}</strong></div>
        <div><span>{tr('Failed requests')}</span><strong>{metrics.loaded ? formatCount(metrics.failures, locale) : '—'}</strong></div>
        <div><span>{tr('Success rate')}</span><strong>{metrics.successRate == null ? '—' : metrics.successRate.toFixed(1) + '%'}</strong></div>
        <div><span>{tr('Tokens')}</span><strong>{metrics.loaded ? formatCount(metrics.tokens, locale) : '—'}</strong></div>
        <div><span>{tr('Last used')}</span><strong title={apiKey.last_used_at ?? ''}>{idleText}</strong></div>
      </div>
      <section class="inspector-section"><h3>{tr('Allowed models')}</h3>
        {#if apiKey.allowed_models?.length}
          <div class="inspector-chips">{#each apiKey.allowed_models as item}<code>{item}</code>{/each}</div>
        {:else}<p>{tr('Any model')}</p>{/if}
      </section>
      <section class="inspector-section"><h3>{tr('Allowed providers')}</h3>
        {#if apiKey.allowed_provider_ids?.length}
          <div class="inspector-chips">{#each apiKey.allowed_provider_ids as item}<code>{item}</code>{/each}</div>
        {:else}<p>{tr('All providers')}</p>{/if}
      </section>
      <section class="inspector-section"><h3>{tr('Key details')}</h3><p>{tr('Created')}: {formatUtcTimestamp(apiKey.created_at, locale) ?? apiKey.created_at}</p><p>{tr('API key ID')}: <code>{apiKey.id}</code></p></section>
      <section class="inspector-section"><h3>{tr('Quick connect')}</h3>
        <p class="inspector-endpoint">{GATEWAY_BASE_URL}</p>
        <div class="inspector-tabs" role="tablist">
          {#each [{ key: 'curl', label: tr('cURL') }, { key: 'openai', label: tr('OpenAI SDK') }, { key: 'anthropic', label: tr('Anthropic SDK') }] as tab (tab.key)}
            <button type="button" role="tab" aria-selected={snippet === tab.key} class:active={snippet === tab.key} onclick={() => (snippet = tab.key as SnippetKind)}>{tab.label}</button>
          {/each}
        </div>
        <pre class="inspector-code"><code>{code}</code></pre>
        <div class="inspector-actions">
          <Button class="mini-add" type="button" onclick={() => copy(code, tr('Snippet copied'))}>{tr('Copy snippet')}</Button>
          <Button class="mini-add" type="button" onclick={() => copy(KEY_PLACEHOLDER, tr('Prefix copied'))}>{tr('Copy placeholder')}</Button>
        </div>
      </section>
      <div class="inspector-actions"><Button class="page-action" type="button" onclick={viewRequests}>{tr('View requests for this key')}</Button></div>
      <p class="inspector-note">{tr('Rotation is not supported by this gateway version. Create a new key instead.')}</p>
    </div>
  {/if}
</Dialog>
<style>
  .inspector { display:grid; gap:12px; }
  .inspector-head { display:flex; align-items:center; justify-content:space-between; gap:8px; }
  .inspector-head code, .inspector-chips code, .inspector-code code { font-family:var(--font-mono); overflow-wrap:anywhere; }
  .inspector-alert { margin:0; padding:8px 10px; color:var(--accent-strong); border:1px solid var(--line); border-radius:8px; background:var(--surface-sunken); font-size:12px; }
  .inspector-grid { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:8px; }
  .inspector-grid > div { display:grid; gap:2px; padding:8px 10px; border:1px solid var(--line); border-radius:9px; background:var(--surface-sunken); min-width:0; }
  .inspector-grid span { color:var(--muted); font-size:11px; }
  .inspector-grid strong { color:var(--ink); font-size:13px; overflow-wrap:anywhere; }
  .inspector-section h3 { margin:0 0 6px; font-size:12px; color:var(--muted); }
  .inspector-section p { margin:2px 0; font-size:12px; color:var(--ink); overflow-wrap:anywhere; }
  .inspector-chips, .inspector-tabs { display:flex; flex-wrap:wrap; gap:6px; }
  .inspector-chips code { display:inline-block; max-width:100%; padding:3px 7px; border:1px solid var(--line); border-radius:999px; background:var(--surface-sunken); font-size:11px; }
  .inspector-tabs button { border:1px solid var(--line); border-radius:999px; padding:4px 9px; background:var(--surface-sunken); color:var(--muted); cursor:pointer; font-size:11px; }
  .inspector-tabs button.active { color:var(--accent); border-color:var(--accent); }
  .inspector-code { margin:8px 0; padding:11px; max-height:220px; overflow:auto; border:1px solid var(--line); border-radius:9px; background:var(--surface-sunken); font-size:11px; line-height:1.5; white-space:pre-wrap; }
  .inspector-actions { display:flex; flex-wrap:wrap; gap:8px; }
  .inspector-note { color:var(--muted); font-size:11px; }
  .inspector-endpoint { font-family:var(--font-mono); font-size:11px; color:var(--accent); }
</style>
