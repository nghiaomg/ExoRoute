<script lang="ts">
  import Button from "../../components/ui/Button.svelte";
  import { onMount } from 'svelte';
  import { AlertTriangle } from '@lucide/svelte';
  import { api } from '../../lib/api';
  import type { Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
import { labelAuthType, labelProtocol, protocolBadgeClass } from '../../lib/labels';
  import type { GatewayCombo, Provider, ProviderBreakdown } from '../../lib/types';
  import ProviderDetailHeader from './ProviderDetailHeader.svelte';
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
  export let onNavigateToCombo: (id: string) => void = () => {};

  let configDialogOpen = false;
  let accountRefresh = 0;
  let keysRefresh = 0;
  let lastProvider = provider;
  let combos = [] as GatewayCombo[];
  let providerStats = null as ProviderBreakdown | null;

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

  async function loadContext(): Promise<void> {
    try {
      const [comboList, statistics] = await Promise.all([api.combos().catch(() => []), api.statistics('1d').catch(() => null)]);
      combos = comboList;
      providerStats = statistics?.providers.rows.find((row) => row.provider === provider.id) ?? null;
    } catch { /* telemetry is optional on this page */ }
  }
  async function toggleProvider(enabled: boolean): Promise<void> {
    await api.updateProvider(provider.id, { enabled });
    onProviderUpdated({ ...provider, enabled });
    onProviderChanged();
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
  <ProviderDetailHeader {provider} {tr} {locale} {combos} stats={providerStats} onBack={onBack} onToggle={toggleProvider} onEdit={() => (configDialogOpen = true)} onManageKeys={() => onManageKeys(provider)} onDelete={() => onDelete(provider)} {onNavigateToCombo} />
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
          <ProviderKeysCard {provider} {tr} {locale} refreshToken={keysRefresh} onProviderChanged={handleProviderChanged} onAddKey={() => onManageKeys(provider)} />
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
