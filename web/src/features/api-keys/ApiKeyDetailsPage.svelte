<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import {
    ArrowLeft,
    Calendar,
    Check,
    Clock3,
    Copy,
    KeyRound,
    ShieldAlert,
    SlidersHorizontal,
    Trash2,
    Zap,
  } from '@lucide/svelte';
  import { api } from '../../lib/api';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import { formatDate, type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { GatewayApiKey } from '../../lib/types';
  import ApiKeyScopeEditor from './ApiKeyScopeEditor.svelte';
  import ApiKeyStatisticsView from './ApiKeyStatisticsView.svelte';

  export let apiKey: GatewayApiKey;
  export let tr: Translate;
  export let locale: Locale;
  export let onBack: () => void;
  export let onRevoke: (key: GatewayApiKey) => void;
  export let onUpdated: () => void;

  let copiedKeyId = false;
  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  let scopeDialogOpen = false;

  function formatTimestamp(value: string | null): string {
    if (!value) return tr('Not available');
    const parsed = new Date(`${value.replace(' ', 'T')}Z`);
    if (Number.isNaN(parsed.getTime())) return value;
    return new Intl.DateTimeFormat(getIntlLocale(locale), {
      dateStyle: 'medium',
      timeStyle: 'short',
      timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    }).format(parsed);
  }

  async function copyId(): Promise<void> {
    if (!apiKey?.id) return;
    try {
      await navigator.clipboard.writeText(apiKey.id);
      copiedKeyId = true;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => {
        copiedKeyId = false;
      }, 2000);
    } catch {
      // Clipboard copy fallback
    }
  }

  onMount(() => {
    if (typeof window !== 'undefined') {
      window.scrollTo({ top: 0, behavior: 'instant' });
    }
  });

  onDestroy(() => {
    if (copyTimer) clearTimeout(copyTimer);
  });
</script>

<div class="api-key-detail-page">
  <!-- Top Navigation & Back Button -->
  <div class="detail-page-nav">
    <button type="button" class="back-link-btn" onclick={onBack}>
      <ArrowLeft size={16} />
      <span>{tr('Back to API keys')}</span>
    </button>
  </div>

  <!-- Hero Banner -->
  <header class="detail-hero-banner api-key-hero">
    <div class="detail-hero-main">
      <div class="detail-avatar-wrap">
        <div class="api-key-avatar">
          <KeyRound size={26} />
        </div>
      </div>

      <div class="detail-hero-info">
        <div class="detail-title-row">
          <h1 class="detail-title">{apiKey.name}</h1>
          <code class="detail-id-chip">{apiKey.id}</code>
          <button
            type="button"
            class="detail-copy-btn"
            title={copiedKeyId ? tr('Key ID copied') : tr('Copy Key ID')}
            onclick={copyId}
          >
            {#if copiedKeyId}
              <Check size={14} class="copy-success-icon" />
              <span>{tr('Key ID copied')}</span>
            {:else}
              <Copy size={14} />
              <span>{tr('Copy Key ID')}</span>
            {/if}
          </button>
          <span class="state-label" class:enabled={apiKey.enabled}>
            <i></i>{tr(apiKey.enabled ? 'Enabled' : 'Disabled')}
          </span>
        </div>

        <div class="detail-tags-row">
          <span class="detail-meta-pill">
            <Calendar size={13} />
            {tr('Created')}: {formatDate(apiKey.created_at, locale)}
          </span>
          <span class="detail-meta-pill">
            <Clock3 size={13} />
            {tr('Last used')}: {apiKey.last_used_at ? formatDate(apiKey.last_used_at, locale) : tr('Never used')}
          </span>
          <span class="detail-meta-pill">
            <Zap size={13} />
            {tr('API requests')}: {(apiKey.request_count ?? 0).toLocaleString(getIntlLocale(locale))}
          </span>
        </div>
      </div>
    </div>

    <div class="detail-hero-actions">
      <button
        type="button"
        class="detail-scope-btn"
        class:scope-invalid={apiKey.scope_valid === false}
        title={tr('Restrict this key to specific providers and models.')}
        onclick={() => { scopeDialogOpen = true; }}
      >
        {#if apiKey.scope_valid === false}<ShieldAlert size={15} />{:else}<SlidersHorizontal size={15} />{/if}
        {tr('Access scope')}
      </button>
      <button
        type="button"
        class="detail-delete-btn"
        title={tr('Revoke {name}', { name: apiKey.name })}
        onclick={() => onRevoke(apiKey)}
      >
        <Trash2 size={15} />
        {tr('Revoke key')}
      </button>
    </div>
  </header>

  {#if apiKey.scope_valid === false}
    <p class="scope-invalid-alert" role="alert">
      <ShieldAlert size={14} />
      {tr('The stored access scope is invalid, so every request with this key is rejected. Save a scope to repair it.')}
    </p>
  {/if}

  <ArkDialog
    open={scopeDialogOpen}
    closeLabel={tr('Close dialog')}
    title={tr('Access scope')}
    kicker={tr('EXOROUTE CONTROL PLANE')}
    wide
    onClose={() => { scopeDialogOpen = false; }}
  >
    <ApiKeyScopeEditor {apiKey} {tr} {onUpdated} embedded />
  </ArkDialog>

  <ApiKeyStatisticsView apiKeyId={apiKey.id} {tr} {locale} />

</div>

<style>
  .api-key-detail-page {
    display: flex;
    flex-direction: column;
    gap: 16px;
    width: 100%;
    margin-bottom: 32px;
  }
  .api-key-hero {
    margin-bottom: 4px;
  }
  /* Framed mini tile in the theme's orange-tint palette, matching
     `.combo-icon` / `.provider-avatar`. */
  .api-key-avatar {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 60px;
    height: 60px;
    border-radius: 14px;
    background: color-mix(in srgb, var(--accent) 10%, #fff);
    color: var(--accent-strong);
    border: var(--nb-border);
    box-shadow: var(--nb-shadow-md);
  }
  .detail-tags-row :global(svg) {
    color: var(--muted);
  }
  .detail-scope-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 9px 14px;
    border-radius: 8px;
    border: none !important;
    box-shadow: none !important;
    background: color-mix(in srgb, var(--accent) 10%, var(--surface));
    color: var(--accent-strong);
    font-size: 13px;
    font-weight: var(--font-semibold);
    cursor: pointer;
    transition: background 0.14s ease;
  }
  .detail-scope-btn:hover {
    background: color-mix(in srgb, var(--accent) 16%, var(--surface));
  }
  .detail-scope-btn.scope-invalid {
    background: color-mix(in srgb, #d97706 12%, var(--surface));
    color: #b45309;
  }
  .scope-invalid-alert {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    margin: 0;
    padding: 9px 11px;
    color: #b45309;
    border: 1px solid rgba(180, 83, 9, 0.28);
    border-radius: 9px;
    background: rgba(249, 115, 22, 0.08);
    font-size: 12px;
    line-height: 1.45;
  }
</style>
