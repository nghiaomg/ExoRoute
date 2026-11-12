<script lang="ts">
  import Button from "../../components/Button.svelte";
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

<div class="flex w-full flex-col gap-[20px]">
  <!-- Top Navigation & Header -->
  <div class="flex items-center">
    <Button type="button" class="back-link-btn" onclick={onBack}>
      <ArrowLeft size={16} />
      <span>{tr('Back to providers')}</span>
    </Button>
  </div>

  <header class="flex flex-wrap items-center justify-between gap-[20px] rounded-[14px] bg-surface px-[28px] py-[24px] max-[860px]:flex-col max-[860px]:items-start [&_.codex-oauth-remote-help]:mt-[4px] [&_.codex-oauth-remote-help]:w-full [&_.codex-oauth-remote-help]:[flex-basis:100%]">
    <div class="flex min-w-0 flex-1 items-center gap-[20px]">
      <div class="shrink-0 grow-0 basis-[60px]">
        <ProviderAvatar name={provider.name} logoUrl={provider.logo_url} adapterId={provider.adapter_id} providerId={provider.id} className="detail-avatar" />
      </div>

      <div class="grid min-w-0 flex-1 gap-[7px]">
        <div class="flex flex-wrap items-center gap-[12px]">
          <h1 class="m-0 text-[22px] font-bold leading-[1.2] text-ink [font-family:var(--font-heading)]">{provider.name}</h1>
          <code class="rounded-[6px] px-[8px] py-[3px] text-[12px] text-accent-strong [background:color-mix(in_srgb,var(--accent)_10%,var(--surface))] [font-family:var(--font-mono)]">{provider.id}</code>
          <span class="state-label" class:enabled={provider.enabled}>
            <i></i>{tr(provider.enabled ? 'Enabled' : 'Disabled')}
          </span>
        </div>

        <div class="flex min-w-0 items-center gap-[10px]">
          <span class="max-w-[500px] truncate text-[12px] text-muted" title={provider.base_url}>{provider.base_url}</span>
          <Button
            type="button"
            class="detail-copy-btn"
            title={copiedUrl ? tr('URL copied') : tr('Copy URL')}
            onclick={copyBaseUrl}
          >
            {#if copiedUrl}
              <Check size={12} class="copy-success-icon" />
              <span>{tr('URL copied')}</span>
            {:else}
              <Copy size={12} />
              <span>{tr('Copy URL')}</span>
            {/if}
          </Button>
        </div>

        <div class="mt-[2px] flex flex-wrap items-center gap-[8px]">
          <span class="protocol-pill {protocolBadgeClass(provider.preferred_protocol)}">{labelProtocol(provider.preferred_protocol, tr)}</span>
          <span class="inline-flex max-w-full items-center gap-[5px] rounded-[6px] bg-surface-sunken px-[8px] py-[3px] text-[12px] text-muted">{labelAuthType(provider.auth_type || 'none', tr)}</span>
          <span class="inline-flex max-w-full items-center gap-[5px] rounded-[6px] bg-surface-sunken px-[8px] py-[3px] text-[12px] text-muted">
            {provider.api_key_count ?? (provider.api_key ? 1 : 0)} {tr(isOAuthProvider && provider.capabilities?.api_keys ? 'Credentials' : isOAuthProvider ? 'Connected accounts' : 'API keys')}
          </span>
          <span class="inline-flex max-w-full items-center gap-[5px] rounded-[6px] bg-surface-sunken px-[8px] py-[3px] text-[12px] text-muted">
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
      <Button
        type="button"
        class="secondary-button"
        onclick={() => configDialogOpen = true}
      >
        <Sliders size={15} />
        {tr('Provider Configurations')}
      </Button>
      {#if provider.capabilities?.api_keys === true}
        <Button
          type="button"
          class="primary-button"
          onclick={() => onManageKeys(provider)}
        >
          <KeyRound size={15} />
          {tr('Manage keys')}
        </Button>
      {/if}
      <Button
        type="button"
        class="detail-delete-btn"
        title={tr('Delete {name}', { name: provider.name })}
        onclick={() => onDelete(provider)}
      >
        <Trash2 size={15} />
        {tr('Delete provider')}
      </Button>
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
  <div class="grid grid-cols-[1fr] items-start gap-[20px]">
    <!-- Column: Saved Models, Credentials / Connected Accounts & Testing Bench -->
    <div class="flex min-w-0 min-h-0 flex-col gap-[20px] [&_.provider-model-list]:max-h-[min(55vh,520px)] [&_.provider-model-list]:overflow-y-auto [&_.provider-model-list]:overscroll-contain">
      {#if isOAuthProvider && authPanel?.oauth && authPanel.accounts}
        <section class="flex min-h-0 flex-col gap-[16px] rounded-[14px] bg-surface px-[24px] py-[22px]">
          <div class="grid gap-[12px]">
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
        <section class="flex min-h-0 flex-col gap-[16px] rounded-[14px] bg-surface px-[24px] py-[22px]">
          <ProviderKeysCard {provider} {tr} {locale} refreshToken={keysRefresh} onProviderChanged={handleProviderChanged} />
        </section>
      {/if}

      <section class="flex min-h-0 flex-col gap-[16px] rounded-[14px] bg-surface px-[24px] py-[22px]">
        <ProviderModelCatalog {provider} open={true} {tr} onProviderChanged={onProviderChanged} />
      </section>

      {#if showModelRouting}
        <section class="flex min-h-0 flex-col gap-[16px] rounded-[14px] bg-surface px-[24px] py-[22px]">
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
