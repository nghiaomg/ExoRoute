<script lang="ts">
  import Button from '../../components/ui/Button.svelte';
  import { Check, Copy, LoaderCircle, PlugZap, Trash2 } from '@lucide/svelte';
  import { api } from '../../lib/api';
  import { localizedError } from '../../lib/errors';
  import { labelAuthType, labelProtocol, protocolBadgeClass } from '../../lib/labels';
  import { formatCount } from '../../lib/format';
  import { showDashboardToast } from '../../lib/dashboard-ui';
  import type { Translate } from '../../lib/format';
  import type { Locale } from '../../lib/i18n';
  import type { GatewayCombo, Provider, ProviderBreakdown } from '../../lib/types';
  import ProviderAvatar from './ProviderAvatar.svelte';
  interface Props { provider: Provider; tr: Translate; locale: Locale; combos: GatewayCombo[]; stats: ProviderBreakdown | null; onBack: () => void; onToggle: (enabled: boolean) => Promise<void>; onEdit: () => void; onManageKeys: () => void; onDelete: () => void; onNavigateToCombo: (id: string) => void }
  let { provider, tr, locale, combos, stats, onBack, onToggle, onEdit, onManageKeys, onDelete, onNavigateToCombo }: Props = $props();
  let copiedUrl = $state(false); let copyTimer: ReturnType<typeof setTimeout> | undefined;
  let testing = $state(false); let testResult = $state<'ok' | 'fail' | null>(null); let testDetail = $state(''); let toggling = $state(false);
  const prefix = $derived(provider.model_prefix || provider.id);
  const dependentCombos = $derived(combos.filter((combo) => combo.targets.some((target) => target.provider_id === provider.id)));
  const successRate = $derived(stats ? Math.round(stats.success_rate * 10) / 10 : null);
  async function copyBaseUrl(): Promise<void> {
    try { await navigator.clipboard.writeText(provider.base_url); copiedUrl = true; showToast(); } catch {}
  }
  function showToast(): void { if (copyTimer) clearTimeout(copyTimer); copyTimer = setTimeout(() => (copiedUrl = false), 2000); }
  async function testConnection(): Promise<void> {
    if (testing || toggling) return; testing = true; testResult = null; testDetail = '';
    try {
      // Reuse the existing model-probe endpoint with this provider’s first saved model.
      const catalog = await api.providerModels(provider.id, { limit: 1 });
      const model = catalog.models[0];
      if (!model) { testResult = 'fail'; testDetail = tr('No saved models for this provider. Import a list or add a model ID manually.'); return; }
      const response = await api.testModels([{ provider_id: provider.id, model }]);
      const result = response.results[0];
      testResult = result?.test_passed ? 'ok' : 'fail';
      testDetail = result ? `${tr(result.test_passed ? 'Test passed' : 'Test failed')} · ${result.latency_ms} ms${result.message ? ` · ${result.message}` : ''}` : tr('No test result was returned for this model.');
    } catch (error) { testResult = 'fail'; testDetail = localizedError(error, 'Connection failed', tr); }
    finally { testing = false; }
  }
  async function toggleEnabled(): Promise<void> {
    if (toggling || testing) return; toggling = true;
    try { await onToggle(!provider.enabled); } finally { toggling = false; }
  }
</script>
<nav class="detail-breadcrumb" aria-label={tr('Providers')}><button type="button" onclick={onBack}>{tr('Providers')}</button><span aria-hidden="true">/</span><strong aria-current="page">{provider.name}</strong></nav>
<header class="detail-header">
  <div class="detail-main">
    <ProviderAvatar name={provider.name} logoUrl={provider.logo_url} adapterId={provider.adapter_id} providerId={provider.id} className="detail-avatar" />
    <div class="detail-identity">
      <div class="detail-title-row"><h1>{provider.name}</h1><code>{provider.id}</code>
        <label class="switch-wrap"><input type="checkbox" role="switch" aria-checked={provider.enabled} aria-label={tr(provider.enabled ? 'Enabled' : 'Disabled')} checked={provider.enabled} disabled={toggling} onchange={toggleEnabled} /><span class="switch" class:on={provider.enabled} aria-hidden="true"><i></i></span><span>{tr(provider.enabled ? 'Enabled' : 'Disabled')}</span></label>
      </div>
      <div class="detail-url"><span title={provider.base_url}>{provider.base_url}</span><Button class="mini-add" type="button" aria-label={tr('Copy URL')} onclick={copyBaseUrl}><Copy size={13} />{copiedUrl ? tr('URL copied') : tr('Copy URL')}</Button></div>
      <div class="detail-badges"><span class="protocol-pill {protocolBadgeClass(provider.preferred_protocol)}">{labelProtocol(provider.preferred_protocol, tr)}</span><span class="detail-badge">{labelAuthType(provider.auth_type || 'none', tr)}</span><span class="detail-badge">{tr('Model prefix')}: <code>{prefix}</code></span>
        {#if provider.invalid_api_key_count}<span class="detail-badge warn">{provider.invalid_api_key_count} {tr('Invalid keys')}</span>{/if}
      </div>
    </div>
  </div>
  <div class="detail-actions">
    <Button class="secondary-button compact" type="button" disabled={testing || toggling} onclick={testConnection}><PlugZap size={14} />{testing ? tr('Testing connection') : tr('Test connection')}</Button>
    <Button class="secondary-button compact" type="button" onclick={onEdit}>{tr('Provider Configurations')}</Button>
    {#if provider.capabilities?.api_keys === true}<Button class="primary-button compact" type="button" onclick={onManageKeys}>{tr('Manage keys')}</Button>{/if}
    <Button class="detail-delete-btn" type="button" title={tr('Delete {name}', { name: provider.name })} onclick={onDelete}><Trash2 size={14} />{tr('Delete provider')}</Button>
  </div>
  {#if testResult}<p class="connection-result" class:ok={testResult === 'ok'} class:fail={testResult === 'fail'} role="status">{testResult === 'ok' ? '✓' : '✕'} {testDetail}</p>{/if}
  <div class="telemetry-strip" aria-label={tr('Provider telemetry')}>
    <div><span>{tr('Requests')}</span><strong>{stats ? formatCount(stats.requests, locale) : '—'}</strong></div>
    <div><span>{tr('Success rate')}</span><strong>{successRate == null ? '—' : successRate + '%'}</strong></div>
    <div><span>{tr('Average latency')}</span><strong>{stats ? formatCount(Math.round(stats.average_latency_ms), locale) + ' ms' : '—'}</strong></div>
    <div class="combo-cell"><span>{tr('Dependent combos')}</span>
      {#if dependentCombos.length}<div class="combo-pills">{#each dependentCombos as combo (combo.id)}<button type="button" onclick={() => onNavigateToCombo(combo.id)}>{combo.name}</button>{/each}</div>
      {:else}<strong class="none">{tr('No dependent combos')}</strong>{/if}
    </div>
  </div>
</header>
<style>
  .detail-breadcrumb { display:flex; align-items:center; gap:8px; font-size:12px; color:var(--muted); }
  .detail-breadcrumb button { border:0; background:transparent; color:var(--accent); cursor:pointer; font-size:12px; padding:0; }
  .detail-breadcrumb strong { color:var(--ink); }
  .detail-header { display:grid; gap:16px; padding:24px 28px; border:1px solid var(--line); border-radius:14px; background:var(--surface); }
  .detail-main { display:flex; gap:18px; align-items:flex-start; min-width:0; }
  .detail-identity { display:grid; gap:8px; min-width:0; flex:1; }
  .detail-title-row { display:flex; flex-wrap:wrap; align-items:center; gap:10px; }
  .detail-title-row h1 { margin:0; font:700 22px var(--font-heading); color:var(--ink); }
  .detail-title-row code { padding:3px 8px; border-radius:6px; background:color-mix(in srgb, var(--accent) 10%, var(--surface)); color:var(--accent-strong); font:12px var(--font-mono); }
  .detail-url { display:flex; flex-wrap:wrap; align-items:center; gap:8px; min-width:0; font-size:12px; color:var(--muted); }
  .detail-url span { max-width:520px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .detail-badges { display:flex; flex-wrap:wrap; gap:8px; }
  .detail-badge { display:inline-flex; gap:4px; align-items:center; padding:3px 9px; border:1px solid var(--line); border-radius:6px; background:var(--surface-sunken); color:var(--muted); font-size:12px; }
  .detail-badge.warn { color:var(--accent-strong); }
  .detail-badge code { font-family:var(--font-mono); }
  .detail-actions { display:flex; flex-wrap:wrap; gap:8px; }
  .switch-wrap { display:inline-flex; align-items:center; gap:6px; font-size:12px; color:var(--muted); cursor:pointer; }
  .switch-wrap input { position:absolute; width:1px; height:1px; opacity:0; }
  .switch { display:inline-flex; align-items:center; width:30px; height:17px; padding:2px; border:1px solid var(--line); border-radius:999px; background:var(--surface); }
  .switch i { width:11px; height:11px; border-radius:50%; background:var(--muted); transition:transform .15s; }
  .switch.on { border-color:var(--chart-good); background:color-mix(in srgb, var(--chart-good) 18%, var(--surface)); }
  .switch.on i { transform:translateX(13px); background:var(--chart-good); }
  .switch-wrap input:focus-visible + .switch { outline:2px solid var(--accent); outline-offset:2px; }
  .connection-result { margin:0; font-size:12px; } .connection-result.ok { color:var(--chart-good); } .connection-result.fail { color:var(--accent-strong); }
  .telemetry-strip { display:grid; grid-template-columns:repeat(3,minmax(0,1fr)) minmax(0,1.6fr); gap:10px; padding-top:14px; border-top:1px solid var(--line); }
  .telemetry-strip > div { display:grid; gap:3px; min-width:0; }
  .telemetry-strip span { color:var(--muted); font-size:11px; }
  .telemetry-strip strong { color:var(--ink); font-size:15px; font-variant-numeric:tabular-nums; }
  .telemetry-strip strong.none { font-size:11px; font-weight:500; color:var(--muted); }
  .combo-pills { display:flex; flex-wrap:wrap; gap:5px; }
  .combo-pills button { padding:2px 8px; border:1px solid var(--line); border-radius:999px; background:var(--surface-sunken); color:var(--accent); font-size:11px; cursor:pointer; }
  @media (max-width:900px){ .telemetry-strip { grid-template-columns:repeat(2,minmax(0,1fr)); } .detail-header { padding:18px; } }
  @media (max-width:560px){ .telemetry-strip { grid-template-columns:1fr; } .detail-main { flex-direction:column; } }

</style>
