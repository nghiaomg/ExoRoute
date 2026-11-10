<script lang="ts">
  import { onMount } from 'svelte';
  import { AlertTriangle, ArrowLeft, Check, Copy, KeyRound, Sliders, Trash2 } from '@lucide/svelte';
  import type { Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
import { labelAuthType, labelProtocol, protocolBadgeClass } from '../../lib/labels';
  import type { Provider } from '../../lib/types';
  import ProviderAvatar from './ProviderAvatar.svelte';
  import { resolveAuthPanel } from './auth-panel.registry';
  import ProviderConfigDialog from './ProviderConfigDialog.svelte';
  import ProviderKeysCard from './ProviderKeysCard.svelte';
  import ProviderModelCatalog from './ProviderModelCatalog.svelte';
  import ProviderModelRouting from './ProviderModelRouting.svelte';

  export let provider: Provider;
  export let tr: Translate;
  export let locale: Locale;
  export let onBack: () => void;
  export let onManageKeys: (provider: Provider) => void;
  export let onProviderChanged: () => void;
  export let onProviderUpdated: (provider: Provider) => void;
  export let onDelete: (provider: Provider) => void;

  let configDialogOpen = false;
  let accountRefresh = 0;
  let keysRefresh = 0;
  let lastProvider = provider;
  let copiedUrl = false;
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  $: if (provider !== lastProvider) {
    lastProvider = provider;
    keysRefresh += 1;
  }

  $: isOAuthProvider = provider?.capabilities?.oauth_accounts === true;
  $: isFreebuff = provider?.id === 'freebuff' || provider?.adapter_id === 'freebuff';
  $: authPanel = resolveAuthPanel(provider?.capabilities?.auth_panel);
  $: isOpencode = (() => {
    const id = provider?.id?.toLowerCase();
    const adapterId = provider?.adapter_id?.toLowerCase();
    return id === 'opencode-go' || id === 'opencode_go' || id === 'opencode-zen' || id === 'opencode_zen'
      || adapterId === 'opencode_go' || adapterId === 'opencode_zen' || adapterId === 'opencode-go' || adapterId === 'opencode-zen';
  })();
  $: showModelRouting = provider?.capabilities?.model_protocol_routing === true && !isOpencode;

  onMount(() => {
    if (typeof window !== 'undefined') {
      window.scrollTo({ top: 0, behavior: 'instant' });
    }
  });

  async function copyBaseUrl(): Promise<void> {
    if (!provider?.base_url) return;
    try {
      await navigator.clipboard.writeText(provider.base_url);
      copiedUrl = true;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => {
        copiedUrl = false;
      }, 2000);
    } catch {
      // Clipboard copy fallback
    }
  }

  function handleAuthConnected(): void {
    accountRefresh += 1;
    keysRefresh += 1;
    onProviderChanged();
  }

  function handleProviderChanged(): void {
    keysRefresh += 1;
    onProviderChanged();
  }
</script>

<div class="provider-detail-page">
  <!-- Top Navigation & Header -->
  <div class="detail-page-nav">
    <button type="button" class="back-link-btn" onclick={onBack}>
      <ArrowLeft size={16} />
      <span>{tr('Back to providers')}</span>
    </button>
  </div>

  <header class="detail-hero-banner">
    <div class="detail-hero-main">
      <div class="detail-avatar-wrap">
        <ProviderAvatar name={provider.name} logoUrl={provider.logo_url} adapterId={provider.adapter_id} providerId={provider.id} className="detail-avatar" />
      </div>

      <div class="detail-hero-info">
        <div class="detail-title-row">
          <h1 class="detail-title">{provider.name}</h1>
          <code class="detail-id-chip">{provider.id}</code>
          <span class="state-label" class:enabled={provider.enabled}>
            <i></i>{tr(provider.enabled ? 'Enabled' : 'Disabled')}
          </span>
        </div>

        <div class="detail-url-row">
          <span class="detail-url-text" title={provider.base_url}>{provider.base_url}</span>
          <button
            type="button"
            class="detail-copy-btn"
            title={copiedUrl ? tr('URL copied') : tr('Copy URL')}
            onclick={copyBaseUrl}
          >
            {#if copiedUrl}
              <Check size={14} class="copy-success-icon" />
              <span>{tr('URL copied')}</span>
            {:else}
              <Copy size={14} />
              <span>{tr('Copy URL')}</span>
            {/if}
          </button>
        </div>

        <div class="detail-tags-row">
          <span class="protocol-pill {protocolBadgeClass(provider.preferred_protocol)}">{labelProtocol(provider.preferred_protocol, tr)}</span>
          <span class="detail-meta-pill">{labelAuthType(provider.auth_type || 'none', tr)}</span>
          <span class="detail-meta-pill">
            {provider.api_key_count ?? (provider.api_key ? 1 : 0)} {tr(isOAuthProvider && provider.capabilities?.api_keys ? 'Credentials' : isOAuthProvider ? 'Connected accounts' : 'API keys')}
          </span>
          <span class="detail-meta-pill">
            {tr('Model prefix')}: <code>{provider.model_prefix ?? provider.id}</code>
          </span>
          {#if provider.invalid_api_key_count}
            <span class="provider-key-warning">
              {provider.invalid_api_key_count} {tr('invalid')}
            </span>
          {/if}
          {#if isFreebuff}
            <span class="provider-ban-risk-badge" title={tr('Account Ban Risk Warning')}>
              <AlertTriangle size={12} />
              {tr('High ban risk')}
            </span>
          {/if}
        </div>
      </div>
    </div>

    <div class="detail-hero-actions">
      <button
        type="button"
        class="secondary-button"
        onclick={() => configDialogOpen = true}
      >
        <Sliders size={15} />
        {tr('Provider Configurations')}
      </button>
      {#if provider.capabilities?.api_keys === true}
        <button
          type="button"
          class="primary-button"
          onclick={() => onManageKeys(provider)}
        >
          <KeyRound size={15} />
          {tr('Manage keys')}
        </button>
      {/if}
      <button
        type="button"
        class="detail-delete-btn"
        title={tr('Delete {name}', { name: provider.name })}
        onclick={() => onDelete(provider)}
      >
        <Trash2 size={15} />
        {tr('Delete provider')}
      </button>
    </div>
  </header>

  {#if isFreebuff}
    <aside class="provider-ban-warning-banner" role="alert">
      <div class="ban-warning-icon">
        <AlertTriangle size={20} />
      </div>
      <div class="ban-warning-body">
        <strong class="ban-warning-title">{tr('Account Ban Risk Warning')}</strong>
        <p class="ban-warning-desc">
          {tr('Using Freebuff with a CodeBuff auth token carries a high risk of upstream account suspension or ban. Session rejections (such as HTTP 409) often indicate provider restrictions. We strongly recommend using secondary or disposable accounts.')}
        </p>
      </div>
    </aside>
  {/if}

  <!-- Main Body Grid -->
  <div class="detail-page-grid">
    <!-- Column: Saved Models, Credentials / Connected Accounts & Testing Bench -->
    <div class="detail-col-models">
      {#if isOAuthProvider && authPanel?.oauth && authPanel.accounts}
        <section class="detail-card">
          <div class="detail-oauth-body">
            <svelte:component
              this={authPanel.oauth}
              providerId={provider.id}
              providerAdapterId={provider.adapter_id}
              providerName={provider.name}
              open={true}
              {tr}
              onConnected={handleAuthConnected}
            />
            <svelte:component
              this={authPanel.accounts}
              providerId={provider.id}
              providerName={provider.name}
              {tr}
              {locale}
              refreshToken={accountRefresh}
              onChanged={onProviderChanged}
            />
          </div>
        </section>
      {/if}

      {#if provider.capabilities?.api_keys === true}
        <section class="detail-card">
          <ProviderKeysCard {provider} {tr} {locale} refreshToken={keysRefresh} onProviderChanged={handleProviderChanged} />
        </section>
      {/if}

      <section class="detail-card">
        <ProviderModelCatalog {provider} open={true} {tr} onProviderChanged={onProviderChanged} />
      </section>

      {#if showModelRouting}
        <section class="detail-card">
          <ProviderModelRouting {provider} {tr} />
        </section>
      {/if}
    </div>
  </div>
</div>

<ProviderConfigDialog
  bind:open={configDialogOpen}
  {provider}
  {tr}
  onClose={() => configDialogOpen = false}
  onProviderUpdated={onProviderUpdated}
/>
