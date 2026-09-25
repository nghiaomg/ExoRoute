<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import {
    ArrowLeft,
    Calendar,
    Check,
    Clock3,
    Copy,
    KeyRound,
    Trash2,
    Zap,
  } from '@lucide/svelte';
  import { api } from '../../lib/api';
  import { getIntlLocale, type Locale } from '../../lib/i18n';
  import { formatDate, type Translate } from '../../lib/format';
import { localizedError } from '../../lib/errors';
  import type { GatewayApiKey } from '../../lib/types';
  import ApiKeyStatisticsView from './ApiKeyStatisticsView.svelte';

  export let apiKey: GatewayApiKey;
  export let tr: Translate;
  export let locale: Locale;
  export let onBack: () => void;
  export let onRevoke: (key: GatewayApiKey) => void;

  let copiedKeyId = false;
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

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
        class="detail-delete-btn"
        title={tr('Revoke {name}', { name: apiKey.name })}
        onclick={() => onRevoke(apiKey)}
      >
        <Trash2 size={15} />
        {tr('Revoke key')}
      </button>
    </div>
  </header>

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
  .api-key-avatar {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 60px;
    height: 60px;
    border-radius: 14px;
    background: linear-gradient(135deg, rgba(124, 58, 237, 0.12) 0%, rgba(109, 40, 217, 0.22) 100%);
    color: var(--violet, #7c3aed);
    border: 1px solid rgba(124, 58, 237, 0.18);
  }
  .detail-tags-row :global(svg) {
    color: var(--muted);
  }
</style>
