<script lang="ts">
import { Check, KeyRound, LoaderCircle } from '@lucide/svelte';
  import ArkDialog from '../../components/ArkDialog.svelte';
  import ArkField from '../../components/ArkField.svelte';
  import type { Locale } from '../../lib/i18n';
  import { type Translate } from '../../lib/format';
import { labelAuthType, labelProtocol, protocolBadgeClass } from '../../lib/labels';
import { localizedError } from '../../lib/errors';
  import { api } from '../../lib/api';
  import type { Provider } from '../../lib/types';
  import ProviderAvatar from './ProviderAvatar.svelte';
  import { resolveAuthPanel } from './auth-panel.registry';
  import ProviderModelCatalog from './ProviderModelCatalog.svelte';

  export let open = false;
  export let provider: Provider | null = null;
  export let tr: Translate;
  export let locale: Locale;
  export let onClose: () => void;
  export let onManageKeys: (provider: Provider) => void;
  export let onProviderChanged: () => void;
  export let onProviderUpdated: (provider: Provider) => void;

  let logoDraft = '';
  let logoNotice = '';
  let logoNoticeTone: 'success' | 'error' = 'success';
  let savingLogo = false;
  let modelPrefixDraft = '';
  let modelPrefixNotice = '';
  let modelPrefixNoticeTone: 'success' | 'error' = 'success';
  let savingModelPrefix = false;
  let accountRefresh = 0;
  let providerId = '';

  $: if (provider && provider.id !== providerId) {
    providerId = provider.id;
    logoDraft = provider.logo_url ?? '';
    logoNotice = '';
    logoNoticeTone = 'success';
    modelPrefixDraft = provider.model_prefix ?? provider.id;
    modelPrefixNotice = '';
    modelPrefixNoticeTone = 'success';
    accountRefresh = 0;
  }
  $: isOAuthProvider = provider?.capabilities?.oauth_accounts === true;
  $: authPanel = resolveAuthPanel(provider?.capabilities?.auth_panel);

  function handleAuthConnected(): void {
    accountRefresh += 1;
    onProviderChanged();
  }

  async function saveLogo(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!provider || savingLogo || savingModelPrefix) return;
    const currentProvider = provider;
    const logoUrl = logoDraft.trim() || null;
    logoNotice = '';
    savingLogo = true;
    try {
      await api.updateProvider(currentProvider.id, {
        id: currentProvider.id,
        adapter_id: currentProvider.adapter_id,
        name: currentProvider.name,
        base_url: currentProvider.base_url,
        logo_url: logoUrl,
        model_prefix: currentProvider.model_prefix,
        enabled: currentProvider.enabled,
        auth_type: currentProvider.auth_type,
        auth_header: currentProvider.auth_header,
        preferred_protocol: currentProvider.preferred_protocol,
        supported_protocols: currentProvider.supported_protocols,
      });
      const updated = { ...currentProvider, logo_url: logoUrl };
      logoDraft = logoUrl ?? '';
      logoNotice = tr('Provider logo saved.');
      logoNoticeTone = 'success';
      onProviderUpdated(updated);
    } catch (error) {
      logoNotice = localizedError(error, 'Could not update provider logo.', tr);
      logoNoticeTone = 'error';
    } finally {
      savingLogo = false;
    }
  }

  async function saveModelPrefix(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!provider || savingLogo || savingModelPrefix) return;
    const currentProvider = provider;
    const modelPrefix = modelPrefixDraft.trim().toLowerCase() || currentProvider.id.toLowerCase();
    modelPrefixNotice = '';
    savingModelPrefix = true;
    try {
      await api.updateProvider(currentProvider.id, {
        id: currentProvider.id,
        adapter_id: currentProvider.adapter_id,
        name: currentProvider.name,
        base_url: currentProvider.base_url,
        logo_url: currentProvider.logo_url,
        model_prefix: modelPrefix,
        enabled: currentProvider.enabled,
        auth_type: currentProvider.auth_type,
        auth_header: currentProvider.auth_header,
        preferred_protocol: currentProvider.preferred_protocol,
        supported_protocols: currentProvider.supported_protocols,
      });
      const updated = { ...currentProvider, model_prefix: modelPrefix };
      modelPrefixDraft = modelPrefix;
      modelPrefixNotice = tr('Provider model prefix saved.');
      modelPrefixNoticeTone = 'success';
      onProviderUpdated(updated);
    } catch (error) {
      modelPrefixNotice = localizedError(error, 'Could not update provider model prefix.', tr);
      modelPrefixNoticeTone = 'error';
    } finally {
      savingModelPrefix = false;
    }
  }
</script>

<ArkDialog {open} closeLabel={tr('Close dialog')} title={provider?.name ?? tr('Provider details')} kicker={tr('Provider details')} wide onClose={onClose}>
  {#if provider}
    <div class="provider-detail-content">
      <section class="provider-detail-summary">
        <div class="provider-detail-summary-top">
          <ProviderAvatar name={provider.name} logoUrl={provider.logo_url} adapterId={provider.adapter_id} providerId={provider.id} />
          <div><strong>{provider.base_url}</strong><small>{tr('PROVIDER ID')}: {provider.id}</small></div>
          <span class="state-label" class:enabled={provider.enabled}><i></i>{tr(provider.enabled ? 'Enabled' : 'Disabled')}</span>
        </div>
        <div class="provider-detail-summary-meta"><span class="protocol-pill {protocolBadgeClass(provider.preferred_protocol)}">{labelProtocol(provider.preferred_protocol, tr)}</span><span>{labelAuthType(provider.auth_type || 'none', tr)}</span><span>{provider.api_key_count ?? (provider.api_key ? 1 : 0)} {tr(isOAuthProvider ? 'Connected accounts' : 'API keys')}</span><span>{tr('Model prefix')}: <code>{provider.model_prefix ?? provider.id}</code></span>{#if provider.invalid_api_key_count}<small class="provider-key-warning">{provider.invalid_api_key_count} {tr('invalid')}</small>{/if}</div>
        {#if !isOAuthProvider}<button class="secondary-button compact" onclick={() => onManageKeys(provider)}><KeyRound size={14} />{tr('Manage keys')}</button>{/if}
      </section>

      {#if isOAuthProvider && authPanel?.oauth && authPanel.accounts}
        <svelte:component this={authPanel.oauth} providerId={provider.id} providerAdapterId={provider.adapter_id} providerName={provider.name} {open} {tr} onConnected={handleAuthConnected} />
        <svelte:component this={authPanel.accounts} providerId={provider.id} {tr} {locale} refreshToken={accountRefresh} onChanged={onProviderChanged} />
      {/if}

      <section class="provider-model-section">
        <form class="provider-logo-form" onsubmit={saveLogo}>
          <ArkField label={tr('Logo image URL')} bind:value={logoDraft} type="url" placeholder="https://example.com/logo.png" />
          {#if logoNotice}<div class="provider-logo-notice" class:error={logoNoticeTone === 'error'} role={logoNoticeTone === 'error' ? 'alert' : 'status'}>{logoNotice}</div>{/if}
          <button class="secondary-button compact" disabled={savingLogo || savingModelPrefix}>{#if savingLogo}<LoaderCircle size={13} class="spin" />{:else}<Check size={13} />{/if}{tr('Save logo')}</button>
        </form>
        <form class="provider-prefix-form" onsubmit={saveModelPrefix}>
          <ArkField label={tr('Model prefix')} bind:value={modelPrefixDraft} maxlength={64} placeholder={tr('Defaults to the provider ID')} />
          {#if modelPrefixNotice}<div class="provider-logo-notice" class:error={modelPrefixNoticeTone === 'error'} role={modelPrefixNoticeTone === 'error' ? 'alert' : 'status'}>{modelPrefixNotice}</div>{/if}
          <button class="secondary-button compact" disabled={savingLogo || savingModelPrefix}>{#if savingModelPrefix}<LoaderCircle size={13} class="spin" />{:else}<Check size={13} />{/if}{tr('Save model prefix')}</button>
        </form>
      </section>

      <ProviderModelCatalog {provider} {open} {tr} onProviderChanged={onProviderChanged} />
    </div>
  {/if}
</ArkDialog>
